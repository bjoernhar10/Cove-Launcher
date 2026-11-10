//! GOG games without GOG Galaxy. The connector builds an install plan from
//! GOG's content system (`gog.plan`); this module downloads it.
//!
//! - Every file consists of zlib-compressed chunks on GOG's CDN. Each chunk is
//!   checked with MD5 before and after unpacking and written at its offset.
//! - Resuming, repairing and updating are the same thing: a chunk whose bytes
//!   are already on disk with the right MD5 is not downloaded again.
//! - Small files come packed into containers that are downloaded once and cut
//!   apart afterwards.
//! - Native Linux games come as GOG's installer (a shell script with a zip
//!   appended); it is downloaded like a normal file and its `data/noarch`
//!   folder becomes the game folder.

use std::collections::{HashMap, HashSet};
use std::io::{Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::Ordering;
use std::time::Duration;

use futures_util::{stream, StreamExt, TryStreamExt};
use md5::{Digest, Md5};
use serde::{Deserialize, Serialize};
use tokio::sync::watch;

use super::extract::safe_relative;
use super::limiter::RateLimiter;
use super::transfer::{http_error, status_error, Control, Outcome, Progress};
use crate::error::{AppError, AppResult};

/// Chunks downloaded at the same time.
const PARALLEL: usize = 6;
const RETRIES: u32 = 3;
/// Wait before retry n: base × 2ⁿ (short in tests).
const RETRY_BASE: Duration = if cfg!(test) {
    Duration::from_millis(5)
} else {
    Duration::from_secs(1)
};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GogChunk {
    pub md5: String,
    pub compressed_md5: String,
    pub size: u64,
    pub compressed_size: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GogSfcRef {
    pub container: usize,
    pub offset: u64,
    pub size: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GogFile {
    pub path: String,
    pub product_id: String,
    pub chunks: Vec<GogChunk>,
    pub executable: bool,
    pub sfc: Option<GogSfcRef>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GogContainer {
    pub product_id: String,
    pub chunks: Vec<GogChunk>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GogLink {
    pub path: String,
    pub target: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GogInstaller {
    pub url: String,
    pub file_name: String,
    pub size: u64,
}

/// Mirror of `GogInstallPlan` in the C# connector.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GogPlan {
    pub product_id: String,
    /// windows, linux or osx: what the files are built for.
    pub platform: String,
    pub build_id: String,
    pub version: String,
    pub download_bytes: u64,
    pub install_bytes: u64,
    pub directories: Vec<String>,
    pub files: Vec<GogFile>,
    pub links: Vec<GogLink>,
    pub containers: Vec<GogContainer>,
    /// Per product: CDN URL templates, best first; `{chunk}` = "ab/cd/abcd…".
    pub chunk_urls: HashMap<String, Vec<String>>,
    pub installer: Option<GogInstaller>,
    /// Owned DLCs whose depots are in the plan.
    #[serde(default)]
    pub dlcs: Vec<GogDlc>,
}

/// Mirror of `GogDlc` in the C# connector.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GogDlc {
    pub product_id: String,
    pub title: String,
}

impl GogPlan {
    /// Download and install bytes of the files that belong to `product`.
    pub fn product_size(&self, product: &str) -> (u64, u64) {
        let mut download = 0;
        let mut install = 0;
        for file in self.files.iter().filter(|f| f.product_id == product) {
            download += file.chunks.iter().map(|c| c.compressed_size).sum::<u64>();
            install += file
                .sfc
                .as_ref()
                .map_or_else(|| file.chunks.iter().map(|c| c.size).sum(), |s| s.size);
        }
        download += self
            .containers
            .iter()
            .filter(|c| c.product_id == product)
            .flat_map(|c| &c.chunks)
            .map(|c| c.compressed_size)
            .sum::<u64>();
        (download, install)
    }

    /// The plan without the left-out DLCs (the game itself always stays).
    /// Their containers stay as empty entries so file references keep their index.
    #[must_use]
    pub fn without(mut self, excluded: &HashSet<String>) -> Self {
        let dropped: HashSet<String> = self
            .dlcs
            .iter()
            .map(|d| d.product_id.clone())
            .filter(|p| excluded.contains(p) && *p != self.product_id)
            .collect();
        if dropped.is_empty() {
            return self;
        }
        for product in &dropped {
            let (download, install) = self.product_size(product);
            self.download_bytes = self.download_bytes.saturating_sub(download);
            self.install_bytes = self.install_bytes.saturating_sub(install);
        }
        self.files.retain(|f| !dropped.contains(&f.product_id));
        for container in &mut self.containers {
            if dropped.contains(&container.product_id) {
                container.chunks.clear();
            }
        }
        self.dlcs.retain(|d| !dropped.contains(&d.product_id));
        self
    }
}

/// Newest public build (`gog.latest`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GogLatest {
    pub build_id: String,
    pub version: String,
    pub platform: String,
}

/// The platform name GOG uses for this computer.
pub fn host_platform() -> &'static str {
    if cfg!(windows) {
        "windows"
    } else if cfg!(target_os = "macos") {
        "osx"
    } else {
        "linux"
    }
}

/// "abcdef…" → "ab/cd/abcdef…".
fn galaxy_path(hash: &str) -> AppResult<String> {
    if hash.len() < 4 || !hash.chars().all(|c| c.is_ascii_hexdigit()) {
        return Err(AppError::Download(tr!(
            "GOG hat eine ungültige Prüfsumme geliefert.",
            "GOG sent an invalid checksum."
        )));
    }
    Ok(format!("{}/{}/{hash}", &hash[..2], &hash[2..4]))
}

fn md5_hex(data: &[u8]) -> String {
    use std::fmt::Write as _;
    Md5::digest(data)
        .iter()
        .fold(String::with_capacity(32), |mut hex, b| {
            let _ = write!(hex, "{b:02x}");
            hex
        })
}

fn damaged() -> AppError {
    AppError::Download(tr!(
        "Ein Teil der Spieldateien kam beschädigt an. Cove versucht es beim Fortsetzen erneut.",
        "Part of the game files arrived damaged. Cove tries again when you resume."
    ))
}

/// Unpacks a chunk and checks it against both checksums.
pub fn unpack_chunk(compressed: &[u8], chunk: &GogChunk) -> AppResult<Vec<u8>> {
    if u64::try_from(compressed.len()).ok() != Some(chunk.compressed_size)
        || !md5_hex(compressed).eq_ignore_ascii_case(&chunk.compressed_md5)
    {
        return Err(damaged());
    }
    let mut data = Vec::with_capacity(usize::try_from(chunk.size).unwrap_or(0));
    flate2::read::ZlibDecoder::new(compressed)
        .read_to_end(&mut data)
        .map_err(|_| damaged())?;
    if u64::try_from(data.len()).ok() != Some(chunk.size)
        || !md5_hex(&data).eq_ignore_ascii_case(&chunk.md5)
    {
        return Err(damaged());
    }
    Ok(data)
}

/// One chunk to put at `offset` into `path`.
struct Piece {
    path: PathBuf,
    offset: u64,
    product: String,
    chunk: GogChunk,
}

/// Is the chunk already on disk (resume, repair, update)?
fn already_there(path: &Path, offset: u64, chunk: &GogChunk) -> bool {
    let Ok(mut file) = std::fs::File::open(path) else {
        return false;
    };
    let len = usize::try_from(chunk.size).unwrap_or(usize::MAX);
    let mut buffer = vec![0u8; len];
    file.seek(SeekFrom::Start(offset)).is_ok()
        && file.read_exact(&mut buffer).is_ok()
        && md5_hex(&buffer).eq_ignore_ascii_case(&chunk.md5)
}

fn write_at(path: &Path, offset: u64, data: &[u8]) -> std::io::Result<()> {
    let mut file = std::fs::OpenOptions::new().write(true).open(path)?;
    file.seek(SeekFrom::Start(offset))?;
    file.write_all(data)
}

/// Creates folders and sizes files so chunks can be written in any order.
fn prepare(path: &Path, size: u64) -> std::io::Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let file = std::fs::OpenOptions::new()
        .create(true)
        .write(true)
        .truncate(false)
        .open(path)?;
    if file.metadata()?.len() != size {
        file.set_len(size)?;
    }
    Ok(())
}

fn target(dir: &Path, relative: &str) -> AppResult<PathBuf> {
    safe_relative(relative).map(|r| dir.join(r)).ok_or_else(|| {
        AppError::Download(tr!(
            "GOG hat einen ungültigen Dateipfad geliefert.",
            "GOG sent an invalid file path."
        ))
    })
}

pub struct Job<'a> {
    pub client: &'a reqwest::Client,
    pub limiter: &'a RateLimiter,
    pub progress: &'a Progress,
    pub control: watch::Receiver<Control>,
    pub plan: &'a GogPlan,
    /// The game folder.
    pub dir: &'a Path,
    /// Temporary files (small-file containers).
    pub work: &'a Path,
}

impl Job<'_> {
    fn stop_requested(&self) -> Option<Outcome> {
        match *self.control.borrow() {
            Control::Run => None,
            Control::Pause => Some(Outcome::Paused),
            Control::Cancel => Some(Outcome::Cancelled),
        }
    }

    async fn stopped(&self) -> Outcome {
        let mut control = self.control.clone();
        if control.wait_for(|c| *c != Control::Run).await.is_err() {
            std::future::pending::<()>().await;
        }
        self.stop_requested().unwrap_or(Outcome::Paused)
    }

    fn container_path(&self, index: usize) -> PathBuf {
        self.work.join(format!("container-{index}.bin"))
    }

    /// Downloads and installs the plan into the game folder. Files of the
    /// previous build that the new one doesn't have are removed (`old_files`).
    pub async fn run(&self, old_files: &[String]) -> AppResult<Outcome> {
        let plan = self.plan;
        let mut pieces = Vec::new();
        let mut sizes = Vec::new();
        for (index, container) in plan.containers.iter().enumerate() {
            let path = self.container_path(index);
            let mut offset = 0;
            for chunk in &container.chunks {
                pieces.push(Piece {
                    path: path.clone(),
                    offset,
                    product: container.product_id.clone(),
                    chunk: chunk.clone(),
                });
                offset += chunk.size;
            }
            sizes.push((path, offset));
        }
        for file in plan.files.iter().filter(|f| f.sfc.is_none()) {
            let path = target(self.dir, &file.path)?;
            let mut offset = 0;
            for chunk in &file.chunks {
                pieces.push(Piece {
                    path: path.clone(),
                    offset,
                    product: file.product_id.clone(),
                    chunk: chunk.clone(),
                });
                offset += chunk.size;
            }
            sizes.push((path, offset));
        }
        let directories: Vec<PathBuf> = plan
            .directories
            .iter()
            .map(|d| target(self.dir, d))
            .collect::<AppResult<_>>()?;
        tokio::task::spawn_blocking(move || -> std::io::Result<()> {
            for dir in directories {
                std::fs::create_dir_all(dir)?;
            }
            for (path, size) in sizes {
                prepare(&path, size)?;
            }
            Ok(())
        })
        .await
        .map_err(|_| {
            crate::error::AppError::Download(tr!(
                "Die Spieldateien konnten nicht angelegt werden.",
                "The game files could not be created."
            ))
        })??;

        self.progress.bytes.store(0, Ordering::Relaxed);
        let results: Vec<Option<Outcome>> = stream::iter(pieces)
            .map(|piece| async move { self.place(&piece).await })
            .buffer_unordered(PARALLEL)
            .try_collect()
            .await?;
        if let Some(stop) = results.into_iter().flatten().next() {
            return Ok(stop);
        }
        if let Some(stop) = self.stop_requested() {
            return Ok(stop);
        }
        self.finish(old_files).await?;
        Ok(Outcome::Completed)
    }

    /// Makes sure one chunk is on disk. `Ok(Some(_))` = stopped by the user.
    async fn place(&self, piece: &Piece) -> AppResult<Option<Outcome>> {
        if let Some(stop) = self.stop_requested() {
            return Ok(Some(stop));
        }
        let (path, offset, chunk) = (piece.path.clone(), piece.offset, piece.chunk.clone());
        let present = tokio::task::spawn_blocking(move || already_there(&path, offset, &chunk))
            .await
            .unwrap_or(false);
        if present {
            self.progress
                .bytes
                .fetch_add(piece.chunk.compressed_size, Ordering::Relaxed);
            return Ok(None);
        }
        let mut attempt = 0;
        let data = loop {
            match self.fetch(piece).await {
                Ok(Some(data)) => break data,
                Ok(None) => return Ok(Some(self.stop_requested().unwrap_or(Outcome::Paused))),
                Err(error)
                    if attempt < RETRIES
                        && matches!(error, AppError::Offline(_) | AppError::Download(_)) =>
                {
                    attempt += 1;
                    tokio::time::sleep(RETRY_BASE * (1 << attempt)).await;
                }
                Err(error) => return Err(error),
            }
        };
        let path = piece.path.clone();
        tokio::task::spawn_blocking(move || write_at(&path, offset, &data))
            .await
            .map_err(|_| damaged())??;
        Ok(None)
    }

    /// Downloads one chunk (trying every CDN), `Ok(None)` when stopped.
    async fn fetch(&self, piece: &Piece) -> AppResult<Option<Vec<u8>>> {
        let chunk_path = galaxy_path(&piece.chunk.compressed_md5)?;
        let urls = self
            .plan
            .chunk_urls
            .get(&piece.product)
            .filter(|u| !u.is_empty())
            .ok_or_else(|| {
                AppError::Download(tr!(
                    "GOG hat keine Download-Adresse geliefert.",
                    "GOG sent no download address."
                ))
            })?;
        let mut last = None;
        for template in urls {
            let url = template.replace("{chunk}", &chunk_path);
            match self.fetch_from(&url, &piece.chunk).await {
                Ok(result) => return Ok(result),
                Err(error) => last = Some(error),
            }
        }
        Err(last.unwrap_or_else(damaged))
    }

    async fn fetch_from(&self, url: &str, chunk: &GogChunk) -> AppResult<Option<Vec<u8>>> {
        let response = tokio::select! {
            response = self.client.get(url).send() => response.map_err(http_error)?,
            _ = self.stopped() => return Ok(None),
        };
        if !response.status().is_success() {
            return Err(status_error(response.status()));
        }
        let mut body = response.bytes_stream();
        let mut data = Vec::with_capacity(usize::try_from(chunk.compressed_size).unwrap_or(0));
        let mut counted = 0u64;
        let result: AppResult<Option<()>> = async {
            loop {
                let bytes = tokio::select! {
                    next = body.next() => match next {
                        Some(bytes) => bytes.map_err(http_error)?,
                        None => return Ok(Some(())),
                    },
                    _ = self.stopped() => return Ok(None),
                };
                let len = bytes.len() as u64;
                if counted + len > chunk.compressed_size {
                    return Err(damaged());
                }
                self.limiter.acquire(len).await;
                data.extend_from_slice(&bytes);
                counted += len;
                self.progress.bytes.fetch_add(len, Ordering::Relaxed);
            }
        }
        .await;
        let checked = match result {
            Ok(Some(())) => {
                let chunk = chunk.clone();
                tokio::task::spawn_blocking(move || unpack_chunk(&data, &chunk))
                    .await
                    .map_err(|_| damaged())
                    .and_then(|r| r)
                    .map(Some)
            }
            Ok(None) => Ok(None),
            Err(error) => Err(error),
        };
        if !matches!(checked, Ok(Some(_))) {
            // Not stored: its bytes count again on the next attempt.
            self.progress.bytes.fetch_sub(counted, Ordering::Relaxed);
        }
        checked
    }

    /// Small files, links, executable bits, files the new build dropped.
    async fn finish(&self, old_files: &[String]) -> AppResult<()> {
        let plan = self.plan.clone();
        let dir = self.dir.to_path_buf();
        let containers: Vec<PathBuf> = (0..plan.containers.len())
            .map(|i| self.container_path(i))
            .collect();
        let old: Vec<String> = old_files.to_vec();
        tokio::task::spawn_blocking(move || finish_files(&plan, &dir, &containers, &old))
            .await
            .map_err(|_| damaged())?
    }
}

fn finish_files(
    plan: &GogPlan,
    dir: &Path,
    containers: &[PathBuf],
    old: &[String],
) -> AppResult<()> {
    for file in &plan.files {
        let path = target(dir, &file.path)?;
        if let Some(sfc) = &file.sfc {
            let container = containers.get(sfc.container).ok_or_else(damaged)?;
            let mut source = std::fs::File::open(container)?;
            source.seek(SeekFrom::Start(sfc.offset))?;
            let mut data = vec![0u8; usize::try_from(sfc.size).unwrap_or(0)];
            source.read_exact(&mut data)?;
            if let Some(parent) = path.parent() {
                std::fs::create_dir_all(parent)?;
            }
            std::fs::write(&path, data)?;
        }
        #[cfg(unix)]
        if file.executable {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755))?;
        }
    }
    for link in &plan.links {
        let path = target(dir, &link.path)?;
        let _ = std::fs::remove_file(&path);
        #[cfg(unix)]
        std::os::unix::fs::symlink(&link.target, &path)?;
    }
    for container in containers {
        let _ = std::fs::remove_file(container);
    }
    let keep: HashSet<String> = plan
        .files
        .iter()
        .map(|f| f.path.to_lowercase())
        .chain(plan.links.iter().map(|l| l.path.to_lowercase()))
        .collect();
    for gone in old.iter().filter(|f| !keep.contains(&f.to_lowercase())) {
        if let Ok(path) = target(dir, gone) {
            let _ = std::fs::remove_file(path);
        }
    }
    Ok(())
}

/// Relative paths the plan installs (stored to clean up after the next update).
pub fn installed_files(plan: &GogPlan) -> Vec<String> {
    plan.files
        .iter()
        .map(|f| f.path.clone())
        .chain(plan.links.iter().map(|l| l.path.clone()))
        .collect()
}

/// Unpacks GOG's Linux installer: its zip part's `data/noarch` becomes the game folder.
pub fn extract_installer(installer: &Path, dir: &Path) -> AppResult<()> {
    let unpack_error = |e: &dyn std::fmt::Display| {
        eprintln!("[cove] GOG-Installer entpacken: {e}");
        AppError::Download(tr!(
            "Der GOG-Installer konnte nicht entpackt werden.",
            "The GOG installer could not be unpacked."
        ))
    };
    let mut archive =
        zip::ZipArchive::new(std::fs::File::open(installer)?).map_err(|e| unpack_error(&e))?;
    let mut found = false;
    for index in 0..archive.len() {
        let mut entry = archive.by_index(index).map_err(|e| unpack_error(&e))?;
        let Some(rest) = entry.name().strip_prefix("data/noarch/").map(str::to_owned) else {
            continue;
        };
        if rest.is_empty() {
            continue;
        }
        let Some(relative) = safe_relative(&rest) else {
            continue;
        };
        let path = dir.join(relative);
        if entry.is_dir() {
            std::fs::create_dir_all(&path)?;
            continue;
        }
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let mut out = std::fs::File::create(&path)?;
        std::io::copy(&mut entry, &mut out)?;
        found = true;
        #[cfg(unix)]
        if let Some(mode) = entry.unix_mode() {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&path, std::fs::Permissions::from_mode(mode & 0o777))?;
        }
    }
    if !found {
        return Err(unpack_error(&"no data/noarch"));
    }
    Ok(())
}

/// The primary play task of `goggame-<product>.info`: program, arguments, working folder.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PlayTask {
    pub path: PathBuf,
    pub arguments: Vec<String>,
    pub working_dir: Option<PathBuf>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct InfoFile {
    #[serde(default)]
    play_tasks: Vec<InfoTask>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct InfoTask {
    #[serde(default)]
    is_primary: bool,
    #[serde(default, rename = "type")]
    kind: String,
    #[serde(default)]
    path: String,
    #[serde(default)]
    arguments: String,
    #[serde(default)]
    working_dir: String,
}

fn relative(dir: &Path, raw: &str) -> Option<PathBuf> {
    safe_relative(&raw.replace('\\', "/")).map(|r| dir.join(r))
}

/// Reads the play task GOG defines for the game (also for Galaxy installs).
pub fn primary_task(dir: &Path, product_id: &str) -> Option<PlayTask> {
    let text = std::fs::read_to_string(dir.join(format!("goggame-{product_id}.info"))).ok()?;
    let info: InfoFile = serde_json::from_str(&text).ok()?;
    let task = info
        .play_tasks
        .iter()
        .filter(|t| t.kind.is_empty() || t.kind == "FileTask")
        .find(|t| t.is_primary)
        .or_else(|| info.play_tasks.iter().find(|t| t.kind == "FileTask"))?;
    Some(PlayTask {
        path: relative(dir, &task.path)?,
        arguments: shlex::split(&task.arguments).unwrap_or_default(),
        working_dir: Some(task.working_dir.as_str())
            .filter(|w| !w.is_empty())
            .and_then(|w| relative(dir, w)),
    })
}

/// What Cove starts after the install: the play task, else `start.sh` (Linux installer).
pub fn launch_file(dir: &Path, product_id: &str) -> Option<PathBuf> {
    primary_task(dir, product_id)
        .map(|t| t.path)
        .or_else(|| Some(dir.join("start.sh")).filter(|p| p.is_file()))
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    pub(crate) fn compress(data: &[u8]) -> Vec<u8> {
        let mut encoder = flate2::write::ZlibEncoder::new(Vec::new(), flate2::Compression::fast());
        encoder.write_all(data).unwrap();
        encoder.finish().unwrap()
    }

    pub(crate) fn chunk_of(data: &[u8]) -> (GogChunk, Vec<u8>) {
        let compressed = compress(data);
        (
            GogChunk {
                md5: md5_hex(data),
                compressed_md5: md5_hex(&compressed),
                size: data.len() as u64,
                compressed_size: compressed.len() as u64,
            },
            compressed,
        )
    }

    #[test]
    fn left_out_dlcs_leave_the_plan() {
        let chunk = |size, compressed_size| GogChunk {
            md5: "m".into(),
            compressed_md5: "c".into(),
            size,
            compressed_size,
        };
        let file =
            |path: &str, product: &str, chunks: Vec<GogChunk>, sfc: Option<GogSfcRef>| GogFile {
                path: path.into(),
                product_id: product.into(),
                chunks,
                executable: false,
                sfc,
            };
        let plan = GogPlan {
            product_id: "100".into(),
            platform: "windows".into(),
            build_id: "8".into(),
            version: "1.0".into(),
            download_bytes: 10 + 4 + 3,
            install_bytes: 20 + 6 + 2,
            directories: vec![],
            files: vec![
                file("game.exe", "100", vec![chunk(20, 10)], None),
                file("dlc/a.pak", "200", vec![chunk(6, 4)], None),
                file(
                    "dlc/b.txt",
                    "200",
                    vec![],
                    Some(GogSfcRef {
                        container: 1,
                        offset: 0,
                        size: 2,
                    }),
                ),
            ],
            links: vec![],
            containers: vec![
                GogContainer {
                    product_id: "100".into(),
                    chunks: vec![],
                },
                GogContainer {
                    product_id: "200".into(),
                    chunks: vec![chunk(2, 3)],
                },
            ],
            chunk_urls: HashMap::new(),
            installer: None,
            dlcs: vec![GogDlc {
                product_id: "200".into(),
                title: "Deep Water".into(),
            }],
        };
        assert_eq!(plan.product_size("200"), (4 + 3, 6 + 2));
        let same = plan.clone().without(&HashSet::from(["300".to_owned()]));
        assert_eq!(same, plan);
        // The game itself can't be left out.
        let game = plan.without(&HashSet::from(["200".to_owned(), "100".to_owned()]));
        assert_eq!(
            game.files
                .iter()
                .map(|f| f.path.as_str())
                .collect::<Vec<_>>(),
            ["game.exe"]
        );
        assert_eq!((game.download_bytes, game.install_bytes), (10, 20));
        assert_eq!(game.containers.len(), 2, "indexes stay valid");
        assert_eq!(game.containers[1].chunks, Vec::new());
        assert_eq!(game.dlcs, Vec::new());
    }

    #[test]
    fn chunks_are_checked_before_and_after_unpacking() {
        let (chunk, compressed) = chunk_of(b"hello gog");
        assert_eq!(unpack_chunk(&compressed, &chunk).unwrap(), b"hello gog");
        let mut broken = compressed.clone();
        broken[3] ^= 1;
        assert!(unpack_chunk(&broken, &chunk).is_err());
        let wrong = GogChunk {
            md5: "0".repeat(32),
            ..chunk
        };
        assert!(unpack_chunk(&compressed, &wrong).is_err());
        assert_eq!(galaxy_path("abcdef").unwrap(), "ab/cd/abcdef");
        assert!(galaxy_path("../x").is_err());
    }

    #[test]
    fn reads_the_primary_play_task() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(
            dir.path().join("goggame-100.info"),
            r#"{ "gameId": "100", "playTasks": [
                { "category": "document", "type": "URLTask", "link": "https://x", "isPrimary": false },
                { "isPrimary": true, "type": "FileTask", "path": "bin\\Saltmarsh.exe",
                  "arguments": "-lang de \"-name x y\"", "workingDir": "bin" } ] }"#,
        )
        .unwrap();
        let task = primary_task(dir.path(), "100").unwrap();
        assert_eq!(task.path, dir.path().join("bin").join("Saltmarsh.exe"));
        assert_eq!(task.arguments, ["-lang", "de", "-name x y"]);
        assert_eq!(task.working_dir, Some(dir.path().join("bin")));
        assert_eq!(launch_file(dir.path(), "100"), Some(task.path));
        assert_eq!(primary_task(dir.path(), "999"), None);
    }

    #[test]
    fn unpacks_the_linux_installer() {
        let dir = tempfile::tempdir().unwrap();
        let installer = dir.path().join("saltmarsh_1_0.sh");
        let mut zip_bytes = std::io::Cursor::new(Vec::new());
        {
            let mut zip = zip::ZipWriter::new(&mut zip_bytes);
            let exec = zip::write::SimpleFileOptions::default().unix_permissions(0o755);
            zip.start_file("data/noarch/start.sh", exec).unwrap();
            zip.write_all(b"#!/bin/sh\n").unwrap();
            zip.start_file(
                "data/noarch/game/data.pak",
                zip::write::SimpleFileOptions::default(),
            )
            .unwrap();
            zip.write_all(b"pak").unwrap();
            zip.start_file(
                "scripts/postinst.sh",
                zip::write::SimpleFileOptions::default(),
            )
            .unwrap();
            zip.write_all(b"ignored").unwrap();
            zip.finish().unwrap();
        }
        // A shell script in front of the zip, like GOG's installers.
        let mut file = b"#!/bin/sh\n# makeself header\nexit 0\n".repeat(50);
        file.extend(zip_bytes.into_inner());
        std::fs::write(&installer, file).unwrap();

        let game = dir.path().join("Saltmarsh");
        extract_installer(&installer, &game).unwrap();
        assert_eq!(std::fs::read(game.join("game/data.pak")).unwrap(), b"pak");
        assert!(!game.join("scripts").exists());
        assert_eq!(launch_file(&game, "100"), Some(game.join("start.sh")));
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = std::fs::metadata(game.join("start.sh"))
                .unwrap()
                .permissions()
                .mode();
            assert_eq!(mode & 0o111, 0o111);
        }
    }
}
