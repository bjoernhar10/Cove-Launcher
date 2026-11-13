//! Amazon games without the Amazon Games app.
//!
//! The connector hands out the download location of a game's current version
//! (`amazon.download`). Below it lie `manifest.proto` (a 4-byte big-endian
//! header length, a protobuf header that names the compression, then the
//! LZMA-compressed protobuf manifest) and every file under `files/<hash>`.
//! Each file carries its SHA-256 (or SHAKE-128), so resume, repair and update
//! are one mechanism: a file already on disk with the right hash is skipped.
//!
//! Games are started from `fuel.json` (`Main.Command`, `Args`,
//! `WorkingSubdirOverride`) with the variables of the Amazon Games SDK, whose
//! files Cove downloads once from Amazon's SDK channel.

use std::path::{Path, PathBuf};
use std::sync::atomic::Ordering;
use std::time::Duration;

use futures_util::{stream, StreamExt};
use serde::{Deserialize, Serialize};
use sha2::Digest as _;
use tokio::io::AsyncWriteExt;
use tokio::sync::watch;

use super::extract::safe_relative;
use super::limiter::RateLimiter;
use super::transfer::{http_error, status_error, Control, Outcome, Progress};
use crate::error::{AppError, AppResult};

const PARALLEL: usize = 6;
const RETRIES: u32 = 3;
const RETRY_BASE: Duration = if cfg!(test) {
    Duration::from_millis(5)
} else {
    Duration::from_secs(1)
};

/// Mirror of `AmazonDownload` in the C# connector.
#[allow(clippy::struct_field_names)]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Download {
    pub product_id: String,
    pub entitlement_id: String,
    pub sku: String,
    pub version_id: String,
    pub download_url: String,
}

/// Mirror of `AmazonSdk` in the C# connector.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Sdk {
    pub version_id: String,
    pub download_url: String,
}

/// What a Cove install of an Amazon game remembers (`store_installs.meta`).
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AmazonMeta {
    pub entitlement_id: String,
    pub sku: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HashAlgorithm {
    Sha256,
    Shake128,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ManifestFile {
    /// Relative path with `/`.
    pub path: String,
    pub size: u64,
    pub algorithm: HashAlgorithm,
    pub hash: Vec<u8>,
}

impl ManifestFile {
    fn hex(&self) -> String {
        hex(&self.hash)
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Manifest {
    pub files: Vec<ManifestFile>,
}

impl Manifest {
    pub fn install_size(&self) -> u64 {
        self.files.iter().map(|f| f.size).sum()
    }
}

fn hex(bytes: &[u8]) -> String {
    use std::fmt::Write as _;
    bytes.iter().fold(String::new(), |mut out, b| {
        let _ = write!(out, "{b:02x}");
        out
    })
}

fn bad_manifest() -> AppError {
    AppError::Download(tr!(
        "Amazon hat ein unlesbares Manifest geliefert.",
        "Amazon sent a manifest Cove can't read."
    ))
}

// ───────────── protobuf (only what the manifest uses) ─────────────

/// One field of a protobuf message: its number and either a number or bytes.
enum Field<'a> {
    Varint(u32, u64),
    Bytes(u32, &'a [u8]),
}

fn varint(data: &[u8], pos: &mut usize) -> Option<u64> {
    let mut value = 0u64;
    for shift in (0..64).step_by(7) {
        let byte = *data.get(*pos)?;
        *pos += 1;
        value |= u64::from(byte & 0x7f) << shift;
        if byte & 0x80 == 0 {
            return Some(value);
        }
    }
    None
}

/// The fields of a message; None when it is malformed.
fn fields(data: &[u8]) -> Option<Vec<Field<'_>>> {
    let mut out = Vec::new();
    let mut pos = 0;
    while pos < data.len() {
        let key = varint(data, &mut pos)?;
        let number = u32::try_from(key >> 3).ok()?;
        match key & 7 {
            0 => out.push(Field::Varint(number, varint(data, &mut pos)?)),
            1 => pos = pos.checked_add(8)?,
            2 => {
                let len = usize::try_from(varint(data, &mut pos)?).ok()?;
                let end = pos.checked_add(len)?;
                out.push(Field::Bytes(number, data.get(pos..end)?));
                pos = end;
            }
            5 => pos = pos.checked_add(4)?,
            _ => return None,
        }
    }
    (pos == data.len()).then_some(out)
}

fn varint_field(fields: &[Field<'_>], number: u32) -> Option<u64> {
    fields.iter().find_map(|f| match f {
        Field::Varint(n, v) if *n == number => Some(*v),
        _ => None,
    })
}

fn bytes_field<'a>(fields: &[Field<'a>], number: u32) -> Option<&'a [u8]> {
    fields.iter().find_map(|f| match f {
        Field::Bytes(n, v) if *n == number => Some(*v),
        _ => None,
    })
}

/// `manifest.proto`: header length, header (compression), manifest.
pub fn parse_manifest(data: &[u8]) -> AppResult<Manifest> {
    let len_bytes: [u8; 4] = data
        .get(..4)
        .and_then(|b| b.try_into().ok())
        .ok_or_else(bad_manifest)?;
    let header_len = usize::try_from(u32::from_be_bytes(len_bytes)).map_err(|_| bad_manifest())?;
    let header = data.get(4..4 + header_len).ok_or_else(bad_manifest)?;
    let body = data.get(4 + header_len..).ok_or_else(bad_manifest)?;
    let header = fields(header).ok_or_else(bad_manifest)?;
    // ManifestHeader.compression (1) → CompressionSettings.algorithm (1): 0 none, 1 lzma.
    let algorithm = bytes_field(&header, 1)
        .and_then(fields)
        .and_then(|c| varint_field(&c, 1))
        .unwrap_or(0);
    let manifest = match algorithm {
        0 => body.to_vec(),
        1 => decompress(body)?,
        _ => return Err(bad_manifest()),
    };
    parse_body(&manifest)
}

/// LZMA as `.xz` or as the older `.lzma` format (Python's `lzma` reads both).
fn decompress(data: &[u8]) -> AppResult<Vec<u8>> {
    let mut out = Vec::new();
    let mut input = std::io::BufReader::new(data);
    let result = if data.starts_with(&[0xfd, b'7', b'z', b'X', b'Z', 0]) {
        lzma_rs::xz_decompress(&mut input, &mut out)
    } else {
        lzma_rs::lzma_decompress(&mut input, &mut out)
    };
    result.map_err(|_| bad_manifest())?;
    Ok(out)
}

/// Manifest (packages 1) → Package (files 2) → File (path 1, size 3, hash 5).
fn parse_body(data: &[u8]) -> AppResult<Manifest> {
    let mut files = Vec::new();
    for field in fields(data).ok_or_else(bad_manifest)? {
        let Field::Bytes(1, package) = field else {
            continue;
        };
        for field in fields(package).ok_or_else(bad_manifest)? {
            let Field::Bytes(2, file) = field else {
                continue;
            };
            let file = fields(file).ok_or_else(bad_manifest)?;
            let path = bytes_field(&file, 1)
                .and_then(|p| std::str::from_utf8(p).ok())
                .ok_or_else(bad_manifest)?
                .replace('\\', "/");
            let hash = bytes_field(&file, 5)
                .and_then(fields)
                .ok_or_else(bad_manifest)?;
            let algorithm = match varint_field(&hash, 1).unwrap_or(0) {
                0 => HashAlgorithm::Sha256,
                1 => HashAlgorithm::Shake128,
                _ => return Err(bad_manifest()),
            };
            let value = bytes_field(&hash, 2).ok_or_else(bad_manifest)?.to_vec();
            if value.is_empty() || safe_relative(&path).is_none() {
                return Err(bad_manifest());
            }
            files.push(ManifestFile {
                path,
                size: varint_field(&file, 3).unwrap_or(0),
                algorithm,
                hash: value,
            });
        }
    }
    Ok(Manifest { files })
}

// ───────────── downloading ─────────────

/// `<download url>/<suffix>`, keeping the signed query.
pub fn below(download_url: &str, suffix: &str) -> String {
    match download_url.split_once('?') {
        Some((base, query)) => format!("{}/{suffix}?{query}", base.trim_end_matches('/')),
        None => format!("{}/{suffix}", download_url.trim_end_matches('/')),
    }
}

pub async fn fetch_manifest(client: &reqwest::Client, download_url: &str) -> AppResult<Manifest> {
    let response = client
        .get(below(download_url, "manifest.proto"))
        .send()
        .await
        .map_err(http_error)?;
    if !response.status().is_success() {
        return Err(status_error(response.status()));
    }
    let bytes = response.bytes().await.map_err(http_error)?;
    tokio::task::spawn_blocking(move || parse_manifest(&bytes))
        .await
        .map_err(|_| bad_manifest())?
}

enum Hasher {
    Sha256(sha2::Sha256),
    Shake128(shake::Shake128),
}

impl Hasher {
    fn new(algorithm: HashAlgorithm) -> Self {
        match algorithm {
            HashAlgorithm::Sha256 => Self::Sha256(sha2::Sha256::new()),
            HashAlgorithm::Shake128 => Self::Shake128(shake::Shake128::default()),
        }
    }

    fn update(&mut self, data: &[u8]) {
        match self {
            Self::Sha256(h) => h.update(data),
            Self::Shake128(h) => shake::Update::update(h, data),
        }
    }

    /// The digest, as long as the manifest's hash (SHAKE has any length).
    fn finish(self, len: usize) -> Vec<u8> {
        match self {
            Self::Sha256(h) => h.finalize().to_vec(),
            Self::Shake128(h) => {
                use shake::{ExtendableOutput as _, XofReader as _};
                let mut out = vec![0u8; len];
                h.finalize_xof().read(&mut out);
                out
            }
        }
    }
}

fn target(dir: &Path, path: &str) -> AppResult<PathBuf> {
    safe_relative(path)
        .map(|r| dir.join(r))
        .ok_or_else(bad_manifest)
}

/// Is the file already on disk with the right content?
fn up_to_date(path: &Path, file: &ManifestFile) -> bool {
    let Ok(meta) = std::fs::metadata(path) else {
        return false;
    };
    if meta.len() != file.size {
        return false;
    }
    let Ok(data) = std::fs::read(path) else {
        return false;
    };
    let mut hasher = Hasher::new(file.algorithm);
    hasher.update(&data);
    hasher.finish(file.hash.len()) == file.hash
}

pub struct Job<'a> {
    pub client: &'a reqwest::Client,
    pub limiter: &'a RateLimiter,
    pub progress: &'a Progress,
    pub control: watch::Receiver<Control>,
    pub manifest: &'a Manifest,
    pub download_url: &'a str,
    pub dir: &'a Path,
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

    /// Files that are missing or differ, checked in the background.
    pub async fn files_to_write(&self) -> AppResult<Vec<ManifestFile>> {
        let files = self.manifest.files.clone();
        let dir = self.dir.to_path_buf();
        tokio::task::spawn_blocking(move || -> AppResult<Vec<ManifestFile>> {
            let mut todo = Vec::new();
            for file in files {
                if !up_to_date(&target(&dir, &file.path)?, &file) {
                    todo.push(file);
                }
            }
            Ok(todo)
        })
        .await
        .map_err(|_| {
            AppError::Download(tr!(
                "Die Spieldateien konnten nicht geprüft werden.",
                "The game files could not be checked."
            ))
        })?
    }

    pub fn total(files: &[ManifestFile]) -> u64 {
        files.iter().map(|f| f.size).sum()
    }

    /// Downloads the files (from `files_to_write`), several at once.
    pub async fn run(&self, files: &[ManifestFile]) -> AppResult<Outcome> {
        let mut results = stream::iter(files.iter().cloned())
            .map(|file| async move { self.fetch(&file).await })
            .buffer_unordered(PARALLEL);
        while let Some(result) = results.next().await {
            if let Some(stop) = result? {
                return Ok(stop);
            }
        }
        Ok(self.stop_requested().unwrap_or(Outcome::Completed))
    }

    /// One file, retried; Some = stopped.
    async fn fetch(&self, file: &ManifestFile) -> AppResult<Option<Outcome>> {
        let path = target(self.dir, &file.path)?;
        if let Some(parent) = path.parent() {
            tokio::fs::create_dir_all(parent).await?;
        }
        let url = below(self.download_url, &format!("files/{}", file.hex()));
        let mut attempt = 0;
        loop {
            match self.fetch_once(&url, file, &path).await {
                Ok(result) => return Ok(result),
                Err(error) if attempt + 1 >= RETRIES => return Err(error),
                Err(_) => {}
            }
            attempt += 1;
            tokio::time::sleep(RETRY_BASE * (1 << attempt)).await;
        }
    }

    async fn fetch_once(
        &self,
        url: &str,
        file: &ManifestFile,
        path: &Path,
    ) -> AppResult<Option<Outcome>> {
        let response = tokio::select! {
            response = self.client.get(url).send() => response.map_err(http_error)?,
            stop = self.stopped() => return Ok(Some(stop)),
        };
        if !response.status().is_success() {
            return Err(status_error(response.status()));
        }
        let part = path.with_file_name(format!(
            "{}.cove-part",
            path.file_name()
                .map(|n| n.to_string_lossy())
                .unwrap_or_default()
        ));
        let mut out = tokio::fs::File::create(&part).await?;
        let mut hasher = Hasher::new(file.algorithm);
        let mut body = response.bytes_stream();
        let mut counted = 0u64;
        let result: AppResult<Option<Outcome>> = async {
            loop {
                let bytes = tokio::select! {
                    next = body.next() => match next {
                        Some(bytes) => bytes.map_err(http_error)?,
                        None => return Ok(None),
                    },
                    stop = self.stopped() => return Ok(Some(stop)),
                };
                let len = bytes.len() as u64;
                self.limiter.acquire(len).await;
                hasher.update(&bytes);
                out.write_all(&bytes).await?;
                counted += len;
                self.progress.bytes.fetch_add(len, Ordering::Relaxed);
            }
        }
        .await;
        out.flush().await?;
        drop(out);
        let damaged = || {
            AppError::Download(tr!(
                "Eine Spieldatei kam beschädigt an.",
                "A game file arrived damaged."
            ))
        };
        let outcome = match result {
            Ok(None) if hasher.finish(file.hash.len()) == file.hash => {
                tokio::fs::rename(&part, path).await?;
                #[cfg(unix)]
                if path.extension().is_none_or(|e| e == "sh") {
                    // Linux builds: keep start files runnable.
                    use std::os::unix::fs::PermissionsExt;
                    let _ = std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o755));
                }
                return Ok(None);
            }
            Ok(None) => Err(damaged()),
            other => other,
        };
        let _ = tokio::fs::remove_file(&part).await;
        self.progress.bytes.fetch_sub(counted, Ordering::Relaxed);
        outcome
    }
}

/// Relative file names of a version (stored to clean up after the next update).
pub fn installed_files(manifest: &Manifest) -> Vec<String> {
    manifest.files.iter().map(|f| f.path.clone()).collect()
}

// ───────────── starting ─────────────

/// The start of an Amazon game from its `fuel.json`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FuelStart {
    pub command: PathBuf,
    pub args: Vec<String>,
    pub working_dir: Option<PathBuf>,
}

/// `fuel.json` allows comments and trailing commas (JSON5); this reads the subset games use.
fn lenient_json(text: &str) -> Option<serde_json::Value> {
    if let Ok(value) = serde_json::from_str(text) {
        return Some(value);
    }
    let mut cleaned = String::with_capacity(text.len());
    let mut in_string = false;
    let mut escaped = false;
    let mut chars = text.chars().peekable();
    while let Some(c) = chars.next() {
        if in_string {
            cleaned.push(c);
            match c {
                _ if escaped => escaped = false,
                '\\' => escaped = true,
                '"' => in_string = false,
                _ => {}
            }
            continue;
        }
        match c {
            '"' => {
                in_string = true;
                cleaned.push(c);
            }
            '/' if chars.peek() == Some(&'/') => {
                for next in chars.by_ref() {
                    if next == '\n' {
                        cleaned.push('\n');
                        break;
                    }
                }
            }
            _ => cleaned.push(c),
        }
    }
    let mut out = String::with_capacity(cleaned.len());
    let chars: Vec<char> = cleaned.chars().collect();
    let mut in_string = false;
    let mut escaped = false;
    for (i, &c) in chars.iter().enumerate() {
        if in_string {
            match c {
                _ if escaped => escaped = false,
                '\\' => escaped = true,
                '"' => in_string = false,
                _ => {}
            }
        } else if c == '"' {
            in_string = true;
        } else if c == ',' {
            let next = chars[i + 1..].iter().find(|n| !n.is_whitespace());
            if matches!(next, Some('}' | ']')) {
                continue;
            }
        }
        out.push(c);
    }
    serde_json::from_str(&out).ok()
}

pub fn fuel_start(dir: &Path) -> Option<FuelStart> {
    let text = std::fs::read_to_string(dir.join("fuel.json")).ok()?;
    let fuel = lenient_json(&text)?;
    let main = fuel.get("Main")?;
    let command = main.get("Command")?.as_str()?.replace('\\', "/");
    let command = dir.join(safe_relative(&command)?);
    let args = main
        .get("Args")
        .and_then(serde_json::Value::as_array)
        .map(|a| {
            a.iter()
                .filter_map(|v| v.as_str().map(str::to_owned))
                .collect()
        })
        .unwrap_or_default();
    let working_dir = main
        .get("WorkingSubdirOverride")
        .and_then(serde_json::Value::as_str)
        .filter(|s| !s.trim().is_empty())
        .and_then(|s| safe_relative(&s.replace('\\', "/")))
        .map(|s| dir.join(s));
    Some(FuelStart {
        command,
        args,
        working_dir,
    })
}

/// Files of the Amazon Games SDK Cove keeps (the rest of the channel is the app).
pub fn is_sdk_file(path: &str) -> bool {
    path.contains("FuelSDK_x64.dll") || path.contains("AmazonGamesSDK_")
}

/// The variables an Amazon game reads to find the SDK and its entitlement.
pub fn launch_env(sdk_dir: &Path, meta: &AmazonMeta, display_name: &str) -> Vec<(String, String)> {
    let services = sdk_dir.join("Amazon Games Services");
    vec![
        (
            "FUEL_DIR".into(),
            services.join("Legacy").to_string_lossy().into_owned(),
        ),
        (
            "AMAZON_GAMES_SDK_PATH".into(),
            services
                .join("AmazonGamesSDK")
                .to_string_lossy()
                .into_owned(),
        ),
        (
            "AMAZON_GAMES_FUEL_ENTITLEMENT_ID".into(),
            meta.entitlement_id.clone(),
        ),
        ("AMAZON_GAMES_FUEL_PRODUCT_SKU".into(), meta.sku.clone()),
        (
            "AMAZON_GAMES_FUEL_DISPLAY_NAME".into(),
            display_name.to_owned(),
        ),
    ]
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    fn key(number: u32, wire: u64) -> Vec<u8> {
        encode_varint((u64::from(number) << 3) | wire)
    }

    fn encode_varint(mut value: u64) -> Vec<u8> {
        let mut out = Vec::new();
        loop {
            let byte = u8::try_from(value & 0x7f).unwrap();
            value >>= 7;
            if value == 0 {
                out.push(byte);
                return out;
            }
            out.push(byte | 0x80);
        }
    }

    fn bytes(number: u32, data: &[u8]) -> Vec<u8> {
        let mut out = key(number, 2);
        out.extend(encode_varint(data.len() as u64));
        out.extend_from_slice(data);
        out
    }

    fn number(number: u32, value: u64) -> Vec<u8> {
        let mut out = key(number, 0);
        out.extend(encode_varint(value));
        out
    }

    /// A `manifest.proto` with these files (path, content), LZMA-compressed or not.
    pub fn manifest_proto(files: &[(&str, &[u8])], lzma: bool) -> Vec<u8> {
        let mut package = bytes(1, b"main");
        for (path, content) in files {
            let digest = sha2::Sha256::digest(content);
            let hash = [number(1, 0), bytes(2, &digest)].concat();
            let file = [
                bytes(1, path.as_bytes()),
                number(2, 0o644),
                number(3, content.len() as u64),
                bytes(4, b"2026-11-01"),
                bytes(5, &hash),
            ]
            .concat();
            package.extend(bytes(2, &file));
        }
        let manifest = bytes(1, &package);
        let body = if lzma {
            let mut out = Vec::new();
            lzma_rs::xz_compress(&mut std::io::BufReader::new(&manifest[..]), &mut out).unwrap();
            out
        } else {
            manifest
        };
        let header = bytes(1, &number(1, u64::from(lzma)));
        let mut data = u32::try_from(header.len()).unwrap().to_be_bytes().to_vec();
        data.extend(header);
        data.extend(body);
        data
    }

    #[test]
    fn manifests_are_read_compressed_or_not() {
        for lzma in [false, true] {
            let data = manifest_proto(
                &[("Game.exe", b"MZ game"), ("data\\level1.pak", b"level")],
                lzma,
            );
            let manifest = parse_manifest(&data).unwrap();
            let paths: Vec<&str> = manifest.files.iter().map(|f| f.path.as_str()).collect();
            assert_eq!(paths, ["Game.exe", "data/level1.pak"]);
            assert_eq!(manifest.files[0].size, 7);
            assert_eq!(
                manifest.files[0].hash,
                sha2::Sha256::digest(b"MZ game").to_vec()
            );
            assert_eq!(manifest.install_size(), 12);
        }
        // The legacy .lzma format too.
        let plain = manifest_proto(&[("a.txt", b"a")], false);
        let header_len =
            4 + usize::try_from(u32::from_be_bytes(plain[..4].try_into().unwrap())).unwrap();
        let mut packed = Vec::new();
        lzma_rs::lzma_compress(
            &mut std::io::BufReader::new(&plain[header_len..]),
            &mut packed,
        )
        .unwrap();
        let header = bytes(1, &number(1, 1));
        let mut data = u32::try_from(header.len()).unwrap().to_be_bytes().to_vec();
        data.extend(header);
        data.extend(packed);
        assert_eq!(parse_manifest(&data).unwrap().files[0].path, "a.txt");
    }

    #[test]
    fn broken_or_unsafe_manifests_are_refused() {
        assert!(parse_manifest(b"\x00\x00").is_err());
        assert!(parse_manifest(&[0, 0, 0, 9, 1, 2]).is_err());
        let escape = manifest_proto(&[("../outside.txt", b"x")], false);
        assert!(parse_manifest(&escape).is_err());
    }

    #[test]
    fn urls_below_the_download_keep_the_signature() {
        assert_eq!(
            below("https://cdn.test/v/7?sig=1", "manifest.proto"),
            "https://cdn.test/v/7/manifest.proto?sig=1"
        );
        assert_eq!(
            below("https://cdn.test/v/7/", "files/ab"),
            "https://cdn.test/v/7/files/ab"
        );
    }

    #[test]
    fn shake128_hashes_have_the_manifests_length() {
        let mut hasher = Hasher::new(HashAlgorithm::Shake128);
        hasher.update(b"");
        // SHAKE128("") starts with 7f9c2ba4e88f827d.
        assert_eq!(hex(&hasher.finish(8)), "7f9c2ba4e88f827d");
    }

    #[test]
    fn fuel_json_sets_command_arguments_and_folder() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(
            dir.path().join("fuel.json"),
            r#"{
              // written by hand
              "SchemaVersion": "2",
              "Main": {
                "Command": "Bin\\Game.exe",
                "Args": ["-fuel", "--lang=en",],
                "WorkingSubdirOverride": "Bin",
              },
            }"#,
        )
        .unwrap();
        let start = fuel_start(dir.path()).unwrap();
        assert_eq!(start.command, dir.path().join("Bin/Game.exe"));
        assert_eq!(start.args, ["-fuel", "--lang=en"]);
        assert_eq!(start.working_dir, Some(dir.path().join("Bin")));
        std::fs::write(
            dir.path().join("fuel.json"),
            r#"{"Main":{"Command":"../x.exe"}}"#,
        )
        .unwrap();
        assert_eq!(fuel_start(dir.path()), None);
        assert!(is_sdk_file("Amazon Games Services/Legacy/FuelSDK_x64.dll"));
        assert!(!is_sdk_file("Amazon Games.exe"));
    }
}
