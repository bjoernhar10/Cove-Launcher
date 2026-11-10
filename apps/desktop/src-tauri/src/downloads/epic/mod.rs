//! Epic games without the Epic Games Launcher.
//!
//! The connector hands out the signed manifest links (`epic.installInfo`); this
//! module downloads the manifest, decides which files are missing or changed
//! (SHA-1 per file, so resume, repair and update are the same), downloads the
//! chunks they need in parallel and assembles the files in order. Chunks used
//! by several files stay in memory (or on disk when many pile up) until their
//! last part is written.
//!
//! Games with anti-cheat or a third-party launcher are not installed: they
//! have to be started through the Epic Launcher.

pub mod format;
pub mod saves;

use std::collections::{HashMap, HashSet, VecDeque};
use std::path::{Path, PathBuf};
use std::sync::atomic::Ordering;
use std::time::Duration;

use futures_util::{stream, StreamExt};
use serde::{Deserialize, Serialize};
use tokio::io::AsyncWriteExt;
use tokio::sync::watch;

use super::extract::safe_relative;
use super::limiter::RateLimiter;
use super::transfer::{http_error, status_error, Control, Outcome, Progress};
use crate::error::{AppError, AppResult};
use format::{read_chunk, ChunkInfo, FileEntry, Guid, Manifest, FLAG_EXECUTABLE};

const PARALLEL: usize = 8;
const RETRIES: u32 = 3;
const RETRY_BASE: Duration = if cfg!(test) {
    Duration::from_millis(5)
} else {
    Duration::from_secs(1)
};
/// Chunks waiting for a later file beyond this go to disk.
const MEMORY_LIMIT: usize = 256 << 20;

/// Mirror of `EpicInstallInfo` in the C# connector.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct InstallInfo {
    pub app_name: String,
    pub namespace: String,
    pub catalog_item_id: String,
    pub build_version: String,
    pub manifest_urls: Vec<String>,
    pub launcher_only: Option<String>,
    pub cloud_save_folder: Option<String>,
    pub ownership_token: bool,
    /// Owned DLCs with files of their own (DLCs that only unlock content are not listed).
    #[serde(default)]
    pub dlcs: Vec<DlcInfo>,
}

/// Mirror of `EpicDlc` in the C# connector: a DLC with its own build.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DlcInfo {
    pub app_name: String,
    pub catalog_item_id: String,
    pub title: String,
    pub build_version: String,
    pub manifest_urls: Vec<String>,
}

/// Mirror of `EpicLaunch` in the C# connector.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LaunchAuth {
    pub exchange_code: String,
    pub account_id: String,
    pub display_name: String,
    /// Base64.
    pub ownership_token: Option<String>,
}

/// Start data kept in `store_installs.meta`.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EpicMeta {
    pub app_name: String,
    pub namespace: String,
    pub launch_command: String,
    pub cloud_save_folder: Option<String>,
    pub ownership_token: bool,
}

/// Files of anti-cheat systems that only work with the Epic Launcher's setup.
const ANTI_CHEAT: [&str; 4] = [
    "easyanticheat/",
    "battleye/",
    "easyanticheat_eos_setup",
    "start_protected_game.exe",
];

/// Why Cove can't run the game itself, or None.
pub fn launcher_only(info: &InstallInfo, manifest: &Manifest) -> Option<String> {
    if let Some(reason) = &info.launcher_only {
        return Some(reason.clone());
    }
    let anti_cheat = manifest.files.iter().any(|f| {
        let name = f.name.replace('\\', "/").to_lowercase();
        ANTI_CHEAT.iter().any(|marker| name.contains(marker))
    });
    anti_cheat.then(|| {
        tr!(
            "Dieses Spiel muss über den Epic Launcher gestartet werden (es nutzt einen Anti-Cheat-Schutz).",
            "This game must be started through the Epic Launcher (it uses anti-cheat protection)."
        )
    })
}

/// Chunk folder of a manifest link: the URL without its file name and query.
pub fn base_url(manifest_url: &str) -> Option<String> {
    let without_query = manifest_url.split('?').next()?;
    let (base, _) = without_query.rsplit_once('/')?;
    Some(base.to_owned())
}

/// Downloads the manifest from the first CDN that answers.
pub async fn fetch_manifest(
    client: &reqwest::Client,
    manifest_urls: &[String],
) -> AppResult<(Manifest, Vec<String>)> {
    let mut last = None;
    for url in manifest_urls {
        let result = async {
            let response = client.get(url).send().await.map_err(http_error)?;
            if !response.status().is_success() {
                return Err(status_error(response.status()));
            }
            let bytes = response.bytes().await.map_err(http_error)?;
            Manifest::parse(&bytes)
        }
        .await;
        match result {
            Ok(manifest) => {
                let bases = manifest_urls.iter().filter_map(|u| base_url(u)).collect();
                return Ok((manifest, bases));
            }
            Err(error) => last = Some(error),
        }
    }
    Err(last.unwrap_or_else(|| {
        AppError::Download(tr!(
            "Epic hat keinen Download-Link geliefert.",
            "Epic sent no download link."
        ))
    }))
}

fn target(dir: &Path, name: &str) -> AppResult<PathBuf> {
    safe_relative(&name.replace('\\', "/"))
        .map(|r| dir.join(r))
        .ok_or_else(|| {
            AppError::Download(tr!(
                "Epic hat einen ungültigen Dateipfad geliefert.",
                "Epic sent an invalid file path."
            ))
        })
}

/// Is the file already on disk with the right content?
fn up_to_date(path: &Path, file: &FileEntry) -> bool {
    let Ok(meta) = std::fs::metadata(path) else {
        return false;
    };
    meta.len() == file.size()
        && std::fs::read(path).is_ok_and(|data| format::sha1(&data) == file.sha)
}

/// The executable Cove starts.
pub fn launch_file(dir: &Path, manifest: &Manifest) -> Option<PathBuf> {
    (!manifest.launch_exe.is_empty())
        .then(|| target(dir, &manifest.launch_exe).ok())
        .flatten()
}

/// Arguments Epic's launcher passes to a game (login through a one-time exchange code).
pub fn launch_args(
    meta: &EpicMeta,
    auth: &LaunchAuth,
    locale: &str,
    ovt: Option<&str>,
) -> Vec<String> {
    let mut args = shlex::split(&meta.launch_command).unwrap_or_default();
    args.extend([
        "-AUTH_LOGIN=unused".to_owned(),
        format!("-AUTH_PASSWORD={}", auth.exchange_code),
        "-AUTH_TYPE=exchangecode".to_owned(),
        format!("-epicapp={}", meta.app_name),
        "-epicenv=Prod".to_owned(),
        "-EpicPortal".to_owned(),
        format!("-epicusername={}", auth.display_name),
        format!("-epicuserid={}", auth.account_id),
        format!("-epiclocale={locale}"),
        format!("-epicsandboxid={}", meta.namespace),
    ]);
    if let Some(path) = ovt {
        args.push(format!("-epicovt={path}"));
    }
    args
}

/// A downloaded chunk waiting for its next file.
enum Cached {
    Memory(Vec<u8>),
    Disk(PathBuf),
}

pub struct Job<'a> {
    pub client: &'a reqwest::Client,
    pub limiter: &'a RateLimiter,
    pub progress: &'a Progress,
    pub control: watch::Receiver<Control>,
    pub manifest: &'a Manifest,
    pub base_urls: &'a [String],
    pub dir: &'a Path,
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

    /// Files that are missing or differ, checked in the background.
    pub async fn files_to_write(&self) -> AppResult<Vec<FileEntry>> {
        let manifest = self.manifest.clone();
        let dir = self.dir.to_path_buf();
        tokio::task::spawn_blocking(move || -> AppResult<Vec<FileEntry>> {
            let mut todo = Vec::new();
            for file in &manifest.files {
                if !up_to_date(&target(&dir, &file.name)?, file) {
                    todo.push(file.clone());
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

    /// Bytes the download needs (for the progress bar).
    pub fn total(&self, files: &[FileEntry]) -> u64 {
        let guids: HashSet<&Guid> = files
            .iter()
            .flat_map(|f| f.parts.iter().map(|p| &p.guid))
            .collect();
        self.manifest.download_size(guids)
    }

    /// Writes the files (from `files_to_write`) of this manifest.
    pub async fn run(&self, files: &[FileEntry]) -> AppResult<Outcome> {
        // Chunks in the order files first need them, and how often each is used.
        let mut order: Vec<Guid> = Vec::new();
        let mut uses: HashMap<Guid, usize> = HashMap::new();
        for part in files.iter().flat_map(|f| &f.parts) {
            let n = uses.entry(part.guid).or_insert(0);
            if *n == 0 {
                order.push(part.guid);
            }
            *n += 1;
        }
        let mut fetched = stream::iter(order)
            .map(|guid| async move { (guid, self.fetch(guid).await) })
            .buffered(PARALLEL);
        let mut cache: HashMap<Guid, Cached> = HashMap::new();
        let mut in_memory = 0usize;
        let mut pending: VecDeque<Guid> = VecDeque::new();

        for file in files {
            if let Some(stop) = self.stop_requested() {
                return Ok(stop);
            }
            let path = target(self.dir, &file.name)?;
            if let Some(parent) = path.parent() {
                tokio::fs::create_dir_all(parent).await?;
            }
            let mut out = tokio::fs::File::create(&path).await?;
            for part in &file.parts {
                while !cache.contains_key(&part.guid) {
                    let next = tokio::select! {
                        next = fetched.next() => next,
                        stop = self.stopped() => return Ok(stop),
                    };
                    let Some((guid, data)) = next else {
                        return Err(AppError::Download(tr!(
                            "Ein Teil der Spieldateien fehlt.",
                            "Part of the game files is missing."
                        )));
                    };
                    match data? {
                        Some(data) => {
                            in_memory += data.len();
                            cache.insert(guid, Cached::Memory(data));
                            pending.push_back(guid);
                        }
                        None => return Ok(self.stop_requested().unwrap_or(Outcome::Paused)),
                    }
                }
                let data = match cache.get(&part.guid) {
                    Some(Cached::Memory(data)) => std::borrow::Cow::Borrowed(data.as_slice()),
                    Some(Cached::Disk(path)) => {
                        std::borrow::Cow::Owned(tokio::fs::read(path).await?)
                    }
                    None => unreachable!("loaded above"),
                };
                let start = usize::try_from(part.offset).unwrap_or(usize::MAX);
                let end = start.saturating_add(usize::try_from(part.size).unwrap_or(usize::MAX));
                let slice = data.get(start..end).ok_or_else(|| AppError::Download(tr!(
                    "Epic hat beschädigte Daten geliefert. Cove versucht es beim Fortsetzen erneut.",
                    "Epic sent damaged data. Cove tries again when you resume."
                )))?;
                out.write_all(slice).await?;
                let left = uses.get_mut(&part.guid).map_or(0, |n| {
                    *n -= 1;
                    *n
                });
                if left == 0 {
                    match cache.remove(&part.guid) {
                        Some(Cached::Memory(data)) => in_memory -= data.len(),
                        Some(Cached::Disk(path)) => {
                            let _ = tokio::fs::remove_file(path).await;
                        }
                        None => {}
                    }
                }
            }
            out.flush().await?;
            drop(out);
            #[cfg(unix)]
            if file.flags & FLAG_EXECUTABLE != 0 {
                use std::os::unix::fs::PermissionsExt;
                tokio::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).await?;
            }
            // Too many chunks waiting for later files: park the oldest on disk.
            while in_memory > MEMORY_LIMIT {
                let Some(guid) = pending.pop_front() else {
                    break;
                };
                if let Some(Cached::Memory(data)) = cache.remove(&guid) {
                    let parked = self.work.join(format!("{}.bin", format::guid_hex(&guid)));
                    tokio::fs::write(&parked, &data).await?;
                    in_memory -= data.len();
                    cache.insert(guid, Cached::Disk(parked));
                }
            }
        }
        Ok(self.stop_requested().unwrap_or(Outcome::Completed))
    }

    /// One chunk, unpacked and checked; `Ok(None)` when stopped.
    async fn fetch(&self, guid: Guid) -> AppResult<Option<Vec<u8>>> {
        let info = self.manifest.chunks.get(&guid).ok_or_else(|| {
            AppError::Download(tr!(
                "Epic hat beschädigte Daten geliefert. Cove versucht es beim Fortsetzen erneut.",
                "Epic sent damaged data. Cove tries again when you resume."
            ))
        })?;
        let path = self.manifest.chunk_path(info);
        let mut attempt = 0;
        loop {
            let mut last = None;
            for base in self.base_urls {
                match self.fetch_from(&format!("{base}/{path}"), info).await {
                    Ok(result) => return Ok(result),
                    Err(error) => last = Some(error),
                }
            }
            let error = last.unwrap_or_else(|| {
                AppError::Download(tr!(
                    "Epic hat keinen Download-Link geliefert.",
                    "Epic sent no download link."
                ))
            });
            if attempt >= RETRIES || !matches!(error, AppError::Offline(_) | AppError::Download(_))
            {
                return Err(error);
            }
            attempt += 1;
            tokio::time::sleep(RETRY_BASE * (1 << attempt)).await;
        }
    }

    async fn fetch_from(&self, url: &str, info: &ChunkInfo) -> AppResult<Option<Vec<u8>>> {
        let response = tokio::select! {
            response = self.client.get(url).send() => response.map_err(http_error)?,
            _ = self.stopped() => return Ok(None),
        };
        if !response.status().is_success() {
            return Err(status_error(response.status()));
        }
        let mut body = response.bytes_stream();
        let mut data = Vec::with_capacity(usize::try_from(info.file_size).unwrap_or(0));
        let mut counted = 0u64;
        let result: AppResult<bool> = async {
            loop {
                let bytes = tokio::select! {
                    next = body.next() => match next {
                        Some(bytes) => bytes.map_err(http_error)?,
                        None => return Ok(true),
                    },
                    _ = self.stopped() => return Ok(false),
                };
                let len = bytes.len() as u64;
                self.limiter.acquire(len).await;
                data.extend_from_slice(&bytes);
                counted += len;
                self.progress.bytes.fetch_add(len, Ordering::Relaxed);
            }
        }
        .await;
        let checked = match result {
            Ok(true) => {
                let info = info.clone();
                tokio::task::spawn_blocking(move || read_chunk(&data, &info))
                    .await
                    .map_err(|_| {
                        AppError::Download(tr!(
                            "Ein Teil der Spieldateien kam beschädigt an.",
                            "Part of the game files arrived damaged."
                        ))
                    })
                    .and_then(|r| r)
                    .map(Some)
            }
            Ok(false) => Ok(None),
            Err(error) => Err(error),
        };
        if !matches!(checked, Ok(Some(_))) {
            self.progress.bytes.fetch_sub(counted, Ordering::Relaxed);
        }
        checked
    }
}

/// Relative file names of a build (stored to clean up after the next update).
pub fn installed_files(manifest: &Manifest) -> Vec<String> {
    manifest
        .files
        .iter()
        .map(|f| f.name.replace('\\', "/"))
        .collect()
}

/// After an update: removes files the old builds had and none of the new
/// ones (game and DLCs together, so a file moving between them stays).
pub async fn remove_old<'a>(
    dir: &Path,
    old_files: impl IntoIterator<Item = &'a String>,
    new_files: impl IntoIterator<Item = &'a String>,
) {
    let keep: HashSet<String> = new_files.into_iter().map(|f| f.to_lowercase()).collect();
    for gone in old_files {
        if keep.contains(&gone.to_lowercase()) {
            continue;
        }
        if let Ok(path) = target(dir, gone) {
            let _ = tokio::fs::remove_file(path).await;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn auth_arguments_like_the_epic_launcher() {
        let meta = EpicMeta {
            app_name: "Sugar".into(),
            namespace: "ns1".into(),
            launch_command: "-nosplash \"-x y\"".into(),
            cloud_save_folder: None,
            ownership_token: true,
        };
        let auth = LaunchAuth {
            exchange_code: "ex1".into(),
            account_id: "acc".into(),
            display_name: "Tiny".into(),
            ownership_token: None,
        };
        let args = launch_args(&meta, &auth, "de-DE", Some("Z:\\tmp\\Sugar.ovt"));
        assert_eq!(&args[..2], ["-nosplash", "-x y"]);
        for expected in [
            "-AUTH_PASSWORD=ex1",
            "-AUTH_TYPE=exchangecode",
            "-epicapp=Sugar",
            "-epicuserid=acc",
            "-epicusername=Tiny",
            "-epiclocale=de-DE",
            "-epicsandboxid=ns1",
            "-epicovt=Z:\\tmp\\Sugar.ovt",
        ] {
            assert!(
                args.iter().any(|a| a == expected),
                "{expected} missing in {args:?}"
            );
        }
    }

    #[test]
    fn anti_cheat_and_third_party_games_stay_with_the_launcher() {
        let (mut manifest, _, _) = format::tests::sample();
        let info = InstallInfo {
            app_name: "Sugar".into(),
            namespace: "ns".into(),
            catalog_item_id: "c".into(),
            build_version: "1".into(),
            manifest_urls: vec![],
            launcher_only: None,
            cloud_save_folder: None,
            ownership_token: false,
            dlcs: vec![],
        };
        assert_eq!(launcher_only(&info, &manifest), None);
        manifest.files[1].name = "EasyAntiCheat\\Settings.json".into();
        assert!(launcher_only(&info, &manifest)
            .unwrap()
            .contains("Anti-Cheat"));
        let third = InstallInfo {
            launcher_only: Some("needs the EA app".into()),
            ..info
        };
        assert_eq!(
            launcher_only(&third, &format::tests::sample().0).as_deref(),
            Some("needs the EA app")
        );
        assert_eq!(
            base_url("https://cdn.test/Builds/Org/o/x/default/abc.manifest?f_token=1").as_deref(),
            Some("https://cdn.test/Builds/Org/o/x/default")
        );
    }
}
