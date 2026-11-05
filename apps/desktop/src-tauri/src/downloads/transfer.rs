//! HTTP transfer: probe, parallel range chunks with resume, or a single
//! stream when the server does not support ranges.

use std::collections::BTreeSet;
use std::path::Path;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Mutex;
use std::time::Duration;

use futures_util::{stream, StreamExt, TryStreamExt};
use reqwest::header::{
    ACCEPT_ENCODING, CONTENT_DISPOSITION, CONTENT_RANGE, ETAG, LAST_MODIFIED, RANGE,
};
use reqwest::StatusCode;
use tokio::io::{AsyncSeekExt, AsyncWriteExt};
use tokio::sync::watch;

use super::limiter::RateLimiter;
use crate::error::{AppError, AppResult};

/// Parallel connections per download (when the server supports ranges).
pub const PARALLEL_CHUNKS: usize = 4;
const CHUNK_RETRIES: u32 = 3;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Control {
    Run,
    Pause,
    Cancel,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Outcome {
    Completed,
    Paused,
    Cancelled,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Remote {
    pub total: Option<u64>,
    pub ranges: bool,
    /// `ETag` or `Last-Modified`: identifies the file version.
    pub version: Option<String>,
    pub file_name: Option<String>,
}

fn http_error(error: reqwest::Error) -> AppError {
    // Signed download links (itch.io) must not end up in messages or logs.
    let error = error.without_url();
    if error.is_timeout() || error.is_connect() {
        AppError::Offline(tr!(
            "Der Server ist nicht erreichbar.",
            "The server can't be reached."
        ))
    } else {
        AppError::Download(tr!(
            "Verbindung unterbrochen ({error})",
            "Connection interrupted ({error})"
        ))
    }
}

fn status_error(status: StatusCode) -> AppError {
    match status.as_u16() {
        404 | 410 => AppError::Download(tr!(
            "Die Datei gibt es unter diesem Link nicht (mehr).",
            "The file is not (or no longer) available at this link."
        )),
        401 | 403 => AppError::Download(tr!(
            "Der Server verweigert den Zugriff auf die Datei.",
            "The server refuses access to the file."
        )),
        code => AppError::Download(tr!(
            "Der Server antwortet mit HTTP {code}.",
            "The server answers with HTTP {code}."
        )),
    }
}

/// Safe file name: no folders, no control characters.
pub fn sanitize_file_name(raw: &str) -> Option<String> {
    let name = raw.rsplit(['/', '\\']).next()?.trim().trim_matches('"');
    let clean: String = name
        .chars()
        .filter(|c| !c.is_control() && !matches!(c, ':' | '*' | '?' | '<' | '>' | '|' | '"'))
        .collect();
    let clean = clean.trim_start_matches('.').trim().to_owned();
    (!clean.is_empty()).then_some(clean)
}

pub fn file_name_from_disposition(header: &str) -> Option<String> {
    // RFC 6266: filename*=UTF-8''name takes precedence over filename="name".
    let mut plain = None;
    for part in header.split(';').map(str::trim) {
        if let Some(value) = part.strip_prefix("filename*=") {
            let encoded = value.split("''").nth(1).unwrap_or(value);
            let decoded = percent_encoding::percent_decode_str(encoded).decode_utf8_lossy();
            return sanitize_file_name(&decoded);
        }
        if let Some(value) = part.strip_prefix("filename=") {
            plain = sanitize_file_name(value);
        }
    }
    plain
}

pub fn file_name_from_url(url: &str) -> Option<String> {
    let url: reqwest::Url = url.parse().ok()?;
    let last = url.path_segments()?.rev().find(|s| !s.is_empty())?;
    sanitize_file_name(&percent_encoding::percent_decode_str(last).decode_utf8_lossy())
}

/// Asks for the first byte: a `206` with `Content-Range` means ranges work.
pub async fn probe(client: &reqwest::Client, url: &str) -> AppResult<Remote> {
    let response = client
        .get(url)
        .header(RANGE, "bytes=0-0")
        .header(ACCEPT_ENCODING, "identity")
        .send()
        .await
        .map_err(http_error)?;
    let status = response.status();
    if !status.is_success() {
        return Err(status_error(status));
    }
    let headers = response.headers();
    let text = |name| {
        headers
            .get(name)
            .and_then(|v: &reqwest::header::HeaderValue| v.to_str().ok())
    };
    let version = text(ETAG)
        .or_else(|| text(LAST_MODIFIED))
        .map(str::to_owned);
    let file_name = text(CONTENT_DISPOSITION)
        .and_then(file_name_from_disposition)
        .or_else(|| file_name_from_url(response.url().as_str()));
    let (total, ranges) = if status == StatusCode::PARTIAL_CONTENT {
        let total = text(CONTENT_RANGE)
            .and_then(|r| r.rsplit('/').next())
            .and_then(|t| t.parse::<u64>().ok());
        (total, total.is_some())
    } else {
        (response.content_length(), false)
    };
    Ok(Remote {
        total,
        ranges,
        version,
        file_name,
    })
}

pub fn chunk_count(total: u64, chunk_size: u64) -> u64 {
    total.div_ceil(chunk_size.max(1))
}

/// Shared state between the transfer and the progress reporter.
#[derive(Default)]
pub struct Progress {
    pub bytes: AtomicU64,
    pub chunks_done: Mutex<BTreeSet<u64>>,
}

pub struct Job<'a> {
    pub client: &'a reqwest::Client,
    pub url: &'a str,
    pub part: &'a Path,
    pub remote: &'a Remote,
    pub chunk_size: u64,
    pub limiter: &'a RateLimiter,
    pub progress: &'a Progress,
    pub control: watch::Receiver<Control>,
}

impl Job<'_> {
    fn stop_requested(&self) -> Option<Outcome> {
        match *self.control.borrow() {
            Control::Run => None,
            Control::Pause => Some(Outcome::Paused),
            Control::Cancel => Some(Outcome::Cancelled),
        }
    }

    /// Resolves as soon as the user pauses or cancels (never, while running).
    async fn stopped(&self) -> Outcome {
        let mut control = self.control.clone();
        if control.wait_for(|c| *c != Control::Run).await.is_err() {
            std::future::pending::<()>().await;
        }
        self.stop_requested().unwrap_or(Outcome::Paused)
    }

    pub async fn run(&self) -> AppResult<Outcome> {
        match (self.remote.ranges, self.remote.total) {
            (true, Some(total)) => self.run_chunked(total).await,
            _ => self.run_single().await,
        }
    }

    async fn run_chunked(&self, total: u64) -> AppResult<Outcome> {
        let file = tokio::fs::OpenOptions::new()
            .create(true)
            .write(true)
            .truncate(false)
            .open(self.part)
            .await?;
        if file.metadata().await?.len() != total {
            file.set_len(total).await?;
        }
        drop(file);
        let done = self
            .progress
            .chunks_done
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .clone();
        let pending: Vec<u64> = (0..chunk_count(total, self.chunk_size))
            .filter(|i| !done.contains(i))
            .collect();
        let already: u64 = done.iter().map(|i| self.chunk_len(*i, total)).sum();
        self.progress.bytes.store(already, Ordering::Relaxed);

        let results: Vec<Option<Outcome>> = stream::iter(pending)
            .map(|index| self.fetch_chunk_with_retries(index, total))
            .buffer_unordered(PARALLEL_CHUNKS)
            .try_collect()
            .await?;
        if let Some(stop) = results.into_iter().flatten().next() {
            return Ok(stop);
        }
        Ok(self.stop_requested().unwrap_or(Outcome::Completed))
    }

    fn chunk_len(&self, index: u64, total: u64) -> u64 {
        let start = index * self.chunk_size;
        (start + self.chunk_size).min(total) - start
    }

    async fn fetch_chunk_with_retries(&self, index: u64, total: u64) -> AppResult<Option<Outcome>> {
        let mut attempt = 0;
        loop {
            match self.fetch_chunk(index, total).await {
                Ok(outcome) => return Ok(outcome),
                Err(error)
                    if attempt < CHUNK_RETRIES
                        && matches!(
                            error,
                            AppError::Offline(_) | AppError::Download(_) | AppError::Io(_)
                        ) =>
                {
                    attempt += 1;
                    tokio::time::sleep(Duration::from_secs(1 << attempt)).await;
                }
                Err(error) => return Err(error),
            }
        }
    }

    /// `Ok(Some(_))` = stopped by the user; the chunk stays unfinished.
    async fn fetch_chunk(&self, index: u64, total: u64) -> AppResult<Option<Outcome>> {
        if let Some(stop) = self.stop_requested() {
            return Ok(Some(stop));
        }
        let start = index * self.chunk_size;
        let end = start + self.chunk_len(index, total) - 1;
        let request = self
            .client
            .get(self.url)
            .header(RANGE, format!("bytes={start}-{end}"))
            .header(ACCEPT_ENCODING, "identity")
            .send();
        let response = tokio::select! {
            response = request => response.map_err(http_error)?,
            stop = self.stopped() => return Ok(Some(stop)),
        };
        if response.status() != StatusCode::PARTIAL_CONTENT {
            return Err(if response.status().is_success() {
                AppError::Download(tr!(
                    "Der Server unterstützt das Fortsetzen nicht mehr.",
                    "The server no longer supports resuming."
                ))
            } else {
                status_error(response.status())
            });
        }
        let mut file = tokio::fs::OpenOptions::new()
            .write(true)
            .open(self.part)
            .await?;
        file.seek(std::io::SeekFrom::Start(start)).await?;
        let mut written = 0u64;
        let mut body = response.bytes_stream();
        let result: AppResult<Option<Outcome>> = async {
            loop {
                let bytes = tokio::select! {
                    next = body.next() => match next {
                        Some(bytes) => bytes,
                        None => break,
                    },
                    stop = self.stopped() => return Ok(Some(stop)),
                };
                let bytes = bytes.map_err(http_error)?;
                let len = bytes.len() as u64;
                if written + len > end - start + 1 {
                    return Err(AppError::Download(tr!(
                        "Der Server hat zu viele Daten geschickt.",
                        "The server sent too much data."
                    )));
                }
                self.limiter.acquire(len).await;
                file.write_all(&bytes).await?;
                written += len;
                self.progress.bytes.fetch_add(len, Ordering::Relaxed);
            }
            Ok(None)
        }
        .await;
        file.flush().await?;
        match result {
            Ok(None) if written == end - start + 1 => {
                self.progress
                    .chunks_done
                    .lock()
                    .unwrap_or_else(std::sync::PoisonError::into_inner)
                    .insert(index);
                Ok(None)
            }
            other => {
                // An unfinished chunk is fetched again later.
                self.progress.bytes.fetch_sub(written, Ordering::Relaxed);
                match other {
                    Ok(None) => Err(AppError::Download(tr!(
                        "Die Verbindung wurde vorzeitig beendet.",
                        "The connection ended early."
                    ))),
                    rest => rest,
                }
            }
        }
    }

    /// No range support: one stream from the start (no resume possible).
    async fn run_single(&self) -> AppResult<Outcome> {
        self.progress.bytes.store(0, Ordering::Relaxed);
        let request = self
            .client
            .get(self.url)
            .header(ACCEPT_ENCODING, "identity")
            .send();
        let response = tokio::select! {
            response = request => response.map_err(http_error)?,
            stop = self.stopped() => return Ok(stop),
        };
        if !response.status().is_success() {
            return Err(status_error(response.status()));
        }
        let mut file = tokio::fs::File::create(self.part).await?;
        let mut body = response.bytes_stream();
        loop {
            let bytes = tokio::select! {
                next = body.next() => match next {
                    Some(bytes) => bytes,
                    None => break,
                },
                stop = self.stopped() => {
                    file.flush().await?;
                    return Ok(stop);
                }
            };
            let bytes = bytes.map_err(http_error)?;
            self.limiter.acquire(bytes.len() as u64).await;
            file.write_all(&bytes).await?;
            self.progress
                .bytes
                .fetch_add(bytes.len() as u64, Ordering::Relaxed);
        }
        file.flush().await?;
        if let Some(total) = self.remote.total {
            if self.progress.bytes.load(Ordering::Relaxed) != total {
                return Err(AppError::Download(tr!(
                    "Die Verbindung wurde vorzeitig beendet.",
                    "The connection ended early."
                )));
            }
        }
        Ok(Outcome::Completed)
    }
}

/// Streams a file through SHA-256 (blocking; run in `spawn_blocking`).
pub fn sha256_file(path: &Path) -> AppResult<String> {
    use sha2::{Digest, Sha256};
    use std::io::Read;
    let mut file = std::fs::File::open(path)?;
    let mut hasher = Sha256::new();
    let mut buffer = vec![0u8; 1 << 20];
    loop {
        let n = file.read(&mut buffer)?;
        if n == 0 {
            break;
        }
        hasher.update(&buffer[..n]);
    }
    Ok(hasher
        .finalize()
        .iter()
        .fold(String::with_capacity(64), |mut hex, b| {
            use std::fmt::Write;
            let _ = write!(hex, "{b:02x}");
            hex
        }))
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use wiremock::matchers::{header, method, path};
    use wiremock::{Mock, MockServer, Request, Respond, ResponseTemplate};

    /// Serves `data` with real range support, like a CDN.
    pub(crate) struct RangeServer {
        pub data: Vec<u8>,
        pub etag: String,
    }

    impl Respond for RangeServer {
        fn respond(&self, request: &Request) -> ResponseTemplate {
            let total = self.data.len();
            let Some(range) = request.headers.get("range").and_then(|v| v.to_str().ok()) else {
                return ResponseTemplate::new(200)
                    .set_body_bytes(self.data.clone())
                    .insert_header("ETag", self.etag.as_str());
            };
            let (start, end) = range.trim_start_matches("bytes=").split_once('-').unwrap();
            let start: usize = start.parse().unwrap();
            let end: usize = end.parse::<usize>().unwrap().min(total - 1);
            ResponseTemplate::new(206)
                .set_body_bytes(self.data[start..=end].to_vec())
                .insert_header(
                    "Content-Range",
                    format!("bytes {start}-{end}/{total}").as_str(),
                )
                .insert_header("ETag", self.etag.as_str())
                .insert_header("Content-Disposition", "attachment; filename=\"game.zip\"")
        }
    }

    impl RangeServer {
        /// Slow responses, so a test can pause or cancel mid-transfer.
        pub(crate) fn delay(self) -> Delayed {
            Delayed(self)
        }
    }

    pub(crate) struct Delayed(RangeServer);

    impl Respond for Delayed {
        fn respond(&self, request: &Request) -> ResponseTemplate {
            self.0
                .respond(request)
                .set_delay(Duration::from_millis(400))
        }
    }

    pub(crate) fn data(len: usize) -> Vec<u8> {
        (0..len).map(|i| u8::try_from(i % 251).unwrap()).collect()
    }

    fn client() -> reqwest::Client {
        reqwest::Client::builder().gzip(false).build().unwrap()
    }

    #[tokio::test]
    async fn errors_never_contain_the_download_url() {
        let error = reqwest::get("http://127.0.0.1:1/file.zip?signature=supersecret")
            .await
            .unwrap_err();
        let message = http_error(error).to_string();
        assert!(!message.contains("supersecret"), "{message}");
    }

    #[test]
    fn file_names() {
        assert_eq!(
            file_name_from_disposition("attachment; filename=\"Lantern 1.2.zip\"").as_deref(),
            Some("Lantern 1.2.zip")
        );
        assert_eq!(
            file_name_from_disposition(
                "attachment; filename=a.zip; filename*=UTF-8''L%C3%A4mpchen.7z"
            )
            .as_deref(),
            Some("Lämpchen.7z")
        );
        assert_eq!(
            file_name_from_disposition("attachment; filename=\"../../etc/passwd\"").as_deref(),
            Some("passwd")
        );
        assert_eq!(
            file_name_from_url("https://cdn.test/files/My%20Game.tar.gz?token=1").as_deref(),
            Some("My Game.tar.gz")
        );
        assert_eq!(file_name_from_url("https://cdn.test/"), None);
        assert_eq!(sanitize_file_name("..hidden"), Some("hidden".into()));
    }

    #[tokio::test]
    async fn downloads_in_parallel_chunks_and_resumes() {
        let server = MockServer::start().await;
        let body = data(100_000);
        Mock::given(method("GET"))
            .and(path("/game.zip"))
            .respond_with(RangeServer {
                data: body.clone(),
                etag: "\"v1\"".into(),
            })
            .mount(&server)
            .await;
        let url = format!("{}/game.zip", server.uri());
        let client = client();
        let remote = probe(&client, &url).await.unwrap();
        assert_eq!(
            remote,
            Remote {
                total: Some(100_000),
                ranges: true,
                version: Some("\"v1\"".into()),
                file_name: Some("game.zip".into())
            }
        );

        let dir = tempfile::tempdir().unwrap();
        let part = dir.path().join("x.part");
        let limiter = RateLimiter::new(None);
        let progress = Progress::default();
        // Pretend chunk 1 was finished before a restart: it is not fetched again.
        std::fs::write(&part, vec![0u8; 100_000]).unwrap();
        let mut seeded = body.clone();
        seeded[..30_000].fill(0);
        seeded[60_000..].fill(0);
        std::fs::write(&part, &seeded).unwrap();
        progress.chunks_done.lock().unwrap().insert(1);
        let (_tx, rx) = watch::channel(Control::Run);
        let job = Job {
            client: &client,
            url: &url,
            part: &part,
            remote: &remote,
            chunk_size: 30_000,
            limiter: &limiter,
            progress: &progress,
            control: rx,
        };
        assert_eq!(job.run().await.unwrap(), Outcome::Completed);
        assert_eq!(std::fs::read(&part).unwrap(), body);
        assert_eq!(progress.bytes.load(Ordering::Relaxed), 100_000);
        assert_eq!(progress.chunks_done.lock().unwrap().len(), 4);
        let ranges: Vec<String> = server
            .received_requests()
            .await
            .unwrap()
            .iter()
            .filter_map(|r| {
                r.headers
                    .get("range")
                    .map(|v| v.to_str().unwrap().to_owned())
            })
            .collect();
        assert!(
            !ranges.contains(&"bytes=30000-59999".to_owned()),
            "finished chunk skipped: {ranges:?}"
        );
    }

    #[tokio::test]
    async fn servers_without_ranges_stream_once() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/patch.bin"))
            .and(header("accept-encoding", "identity"))
            .respond_with(ResponseTemplate::new(200).set_body_bytes(data(5000)))
            .mount(&server)
            .await;
        let url = format!("{}/patch.bin", server.uri());
        let client = client();
        let remote = probe(&client, &url).await.unwrap();
        assert!(!remote.ranges);
        assert_eq!(remote.file_name.as_deref(), Some("patch.bin"));
        let dir = tempfile::tempdir().unwrap();
        let part = dir.path().join("p.part");
        let (_tx, rx) = watch::channel(Control::Run);
        let limiter = RateLimiter::new(None);
        let progress = Progress::default();
        let job = Job {
            client: &client,
            url: &url,
            part: &part,
            remote: &remote,
            chunk_size: 1000,
            limiter: &limiter,
            progress: &progress,
            control: rx,
        };
        assert_eq!(job.run().await.unwrap(), Outcome::Completed);
        assert_eq!(std::fs::read(&part).unwrap(), data(5000));
    }

    #[tokio::test]
    async fn pause_and_errors() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/gone"))
            .respond_with(ResponseTemplate::new(404))
            .mount(&server)
            .await;
        Mock::given(method("GET"))
            .and(path("/big"))
            .respond_with(RangeServer {
                data: data(50_000),
                etag: "e".into(),
            })
            .mount(&server)
            .await;
        let client = client();
        let error = probe(&client, &format!("{}/gone", server.uri()))
            .await
            .unwrap_err();
        assert!(error
            .to_string()
            .contains("gibt es unter diesem Link nicht"));

        let url = format!("{}/big", server.uri());
        let remote = probe(&client, &url).await.unwrap();
        let dir = tempfile::tempdir().unwrap();
        let part = dir.path().join("b.part");
        let (tx, rx) = watch::channel(Control::Run);
        tx.send(Control::Pause).unwrap();
        let limiter = RateLimiter::new(None);
        let progress = Progress::default();
        let job = Job {
            client: &client,
            url: &url,
            part: &part,
            remote: &remote,
            chunk_size: 10_000,
            limiter: &limiter,
            progress: &progress,
            control: rx,
        };
        assert_eq!(job.run().await.unwrap(), Outcome::Paused);
        assert!(progress.chunks_done.lock().unwrap().is_empty());
    }

    #[test]
    fn sha256_of_a_file() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("abc");
        std::fs::write(&file, b"abc").unwrap();
        assert_eq!(
            sha256_file(&file).unwrap(),
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
    }
}
