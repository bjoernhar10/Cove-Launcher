//! Proton without Steam. Cove downloads GE-Proton (the builds of `GloriousEggroll`,
//! released on GitHub with a SHA-512 checksum) into its own folder, keeps
//! "latest" up to date and also offers the Proton builds Steam installed.
//! Valve's own Proton can only be downloaded through Steam.
//!
//! A game asks for a build by id: `latest` (the newest GE-Proton), a GE-Proton
//! release name (`GE-Proton10-25`) or `steam:<folder>`. A missing GE-Proton
//! build is installed before the game starts.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::Ordering;
use std::sync::{Arc, Mutex, PoisonError};
use std::time::Duration;

use chrono::{DateTime, Utc};
use reqwest::header::ACCEPT;
use serde::{Deserialize, Serialize};
use sqlx::SqlitePool;
use tokio::sync::watch;

use crate::db;
use crate::downloads::limiter::RateLimiter;
use crate::downloads::transfer::{self, Control, Job, Outcome, Progress};
use crate::error::{AppError, AppResult};
use crate::launcher::{find_proton_installs, version_key, HostEnv};
use crate::models::{
    AppSettings, CompatibilityTool, LaunchOptions, ProtonKind, ProtonOverview, ProtonPhase,
    ProtonProgress, ProtonVersion,
};

/// "Always the newest GE-Proton"; updated automatically.
pub const LATEST: &str = "latest";
/// Prefix of the ids of Proton builds Steam installed.
pub const STEAM_PREFIX: &str = "steam:";
const GE_PREFIX: &str = "GE-Proton";
pub const GE_RELEASES_URL: &str =
    "https://api.github.com/repos/GloriousEggroll/proton-ge-custom/releases?per_page=50";
const CACHE_KEY: &str = "proton:ge-releases";
const CACHE_PROVIDER: &str = "proton";
/// How long the release list counts as fresh (and how often "latest" is checked).
const LIST_TTL_HOURS: i64 = 6;
pub const CHECK_INTERVAL: Duration = Duration::from_secs(60 * 60 * LIST_TTL_HOURS as u64);
/// In a build Cove installed for "latest": it goes once a newer one is there and no game needs it.
const AUTO_MARKER: &str = ".cove-latest";
/// Downloads and unpacking happen here, next to the builds (same drive).
const WORK_DIR: &str = ".work";
const CHUNK_SIZE: u64 = 16 << 20;
const PROGRESS_INTERVAL: Duration = Duration::from_millis(250);

/// A Proton build on this machine.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProtonBuild {
    pub id: String,
    pub path: PathBuf,
}

/// Release names become folder names: only plain characters.
fn valid_name(name: &str) -> bool {
    !name.is_empty()
        && name.len() <= 64
        && !name.starts_with('.')
        && name
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.'))
}

pub fn is_ge(id: &str) -> bool {
    id.starts_with(GE_PREFIX) && valid_name(id)
}

/// The build an id stands for; `latest` = the newest GE-Proton on this machine.
pub fn resolve<'a>(builds: &'a [ProtonBuild], id: &str) -> Option<&'a ProtonBuild> {
    if id == LATEST {
        builds
            .iter()
            .filter(|b| is_ge(&b.id))
            .max_by_key(|b| version_key(&b.id))
    } else {
        builds.iter().find(|b| b.id == id)
    }
}

/// The Proton build a game asks for, or `None` when it does not use Proton.
pub fn wanted(options: &LaunchOptions, default: &str) -> Option<String> {
    (options.compatibility_tool == CompatibilityTool::Proton).then(|| {
        options
            .proton_version
            .as_deref()
            .filter(|v| !v.trim().is_empty())
            .unwrap_or(default)
            .to_owned()
    })
}

/// Readable name of an id ("steam:Proton 9.0" → "Proton 9.0").
pub fn display_name(id: &str) -> &str {
    id.strip_prefix(STEAM_PREFIX).unwrap_or(id)
}

/// GE-Proton builds in Cove's folder.
fn own_builds(root: &Path) -> Vec<ProtonBuild> {
    let Ok(entries) = std::fs::read_dir(root) else {
        return Vec::new();
    };
    entries
        .flatten()
        .filter_map(|entry| {
            let id = entry.file_name().to_string_lossy().into_owned();
            let path = entry.path();
            (is_ge(&id) && path.join("proton").is_file()).then_some(ProtonBuild { id, path })
        })
        .collect()
}

/// Cove's builds and the ones Steam installed.
pub fn installed_builds(root: &Path, home: Option<&Path>) -> Vec<ProtonBuild> {
    let mut builds = own_builds(root);
    if let Some(home) = home {
        builds.extend(find_proton_installs(home).into_iter().filter_map(|path| {
            let name = path.file_name()?.to_string_lossy().into_owned();
            Some(ProtonBuild {
                id: format!("{STEAM_PREFIX}{name}"),
                path,
            })
        }));
    }
    builds
}

// ───────────── GE-Proton releases ─────────────

#[derive(Deserialize)]
struct GithubRelease {
    tag_name: String,
    published_at: Option<String>,
    #[serde(default)]
    draft: bool,
    #[serde(default)]
    prerelease: bool,
    #[serde(default)]
    assets: Vec<GithubAsset>,
}

#[derive(Deserialize)]
struct GithubAsset {
    name: String,
    size: u64,
    browser_download_url: String,
}

/// A GE-Proton release Cove can install.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct Release {
    name: String,
    published_at: Option<String>,
    archive_url: String,
    archive_size: u64,
    checksum_url: String,
}

/// Published releases with a `.tar.gz` and its `.sha512sum`, newest first.
fn releases_from(github: Vec<GithubRelease>) -> Vec<Release> {
    let mut releases: Vec<Release> = github
        .into_iter()
        .filter(|r| !r.draft && !r.prerelease && is_ge(&r.tag_name))
        .filter_map(|r| {
            let asset = |name: String| r.assets.iter().find(|a| a.name == name);
            let archive = asset(format!("{}.tar.gz", r.tag_name))?;
            let checksum = asset(format!("{}.sha512sum", r.tag_name))?;
            Some(Release {
                archive_url: archive.browser_download_url.clone(),
                archive_size: archive.size,
                checksum_url: checksum.browser_download_url.clone(),
                name: r.tag_name.clone(),
                published_at: r.published_at.clone(),
            })
        })
        .collect();
    releases.sort_by_key(|r| std::cmp::Reverse(version_key(&r.name)));
    releases
}

/// The release list and when it was loaded.
struct ReleaseList {
    releases: Vec<Release>,
    checked_at: Option<DateTime<Utc>>,
    offline: bool,
}

fn unreachable_error() -> AppError {
    AppError::Offline(tr!(
        "Die Proton-Versionen konnten nicht geladen werden. Prüfe deine Internetverbindung.",
        "The Proton versions could not be loaded. Check your internet connection."
    ))
}

/// First word of a `sha512sum` line, when it is a SHA-512 in hex.
fn parse_checksum(text: &str) -> Option<String> {
    let hash = text.split_whitespace().next()?.to_ascii_lowercase();
    (hash.len() == 128 && hash.chars().all(|c| c.is_ascii_hexdigit())).then_some(hash)
}

/// Streams a file through SHA-512 (blocking; run in `spawn_blocking`).
fn sha512_file(path: &Path) -> AppResult<String> {
    use sha2::{Digest, Sha512};
    use std::io::Read;
    let mut file = std::fs::File::open(path)?;
    let mut hasher = Sha512::new();
    let mut buffer = vec![0u8; 1 << 20];
    loop {
        let n = file.read(&mut buffer)?;
        if n == 0 {
            break;
        }
        hasher.update(&buffer[..n]);
    }
    Ok(to_hex(&hasher.finalize()))
}

fn to_hex(bytes: &[u8]) -> String {
    bytes.iter().fold(String::with_capacity(128), |mut hex, b| {
        use std::fmt::Write;
        let _ = write!(hex, "{b:02x}");
        hex
    })
}

/// Unpacks a GE-Proton `.tar.gz` (one folder named like the release) into `dest`.
fn unpack(archive: &Path, dest: &Path) -> AppResult<()> {
    let file = std::fs::File::open(archive)?;
    let mut tar = tar::Archive::new(flate2::read::GzDecoder::new(file));
    tar.set_preserve_mtime(true);
    // `unpack` refuses entries outside `dest`; Proton's own symlinks stay inside.
    tar.unpack(dest).map_err(|e| {
        AppError::Download(tr!(
            "Proton konnte nicht entpackt werden ({e}).",
            "Proton could not be unpacked ({e})."
        ))
    })
}

// ───────────── manager ─────────────

pub type ProgressFn = Arc<dyn Fn(ProtonProgress) + Send + Sync>;

type InstallResult = Option<Result<PathBuf, String>>;

struct Install {
    done: watch::Receiver<InstallResult>,
    progress: ProtonProgress,
}

pub struct ProtonManager {
    pool: SqlitePool,
    client: reqwest::Client,
    releases_url: String,
    /// Cove's Proton builds (`<app data>/proton`).
    root: PathBuf,
    home: Option<PathBuf>,
    defaults: AppSettings,
    emit: ProgressFn,
    installs: Mutex<HashMap<String, Install>>,
}

impl ProtonManager {
    pub fn new(
        pool: SqlitePool,
        root: PathBuf,
        home: Option<PathBuf>,
        defaults: AppSettings,
        emit: ProgressFn,
    ) -> Self {
        Self::with_endpoint(pool, root, home, defaults, emit, GE_RELEASES_URL.into())
    }

    pub fn with_endpoint(
        pool: SqlitePool,
        root: PathBuf,
        home: Option<PathBuf>,
        defaults: AppSettings,
        emit: ProgressFn,
        releases_url: String,
    ) -> Self {
        let client = reqwest::Client::builder()
            .user_agent(concat!(
                "Cove/",
                env!("CARGO_PKG_VERSION"),
                " (desktop game launcher)"
            ))
            .connect_timeout(Duration::from_secs(10))
            .read_timeout(Duration::from_secs(60))
            .build()
            .unwrap_or_default();
        Self {
            pool,
            client,
            releases_url,
            root,
            home,
            defaults,
            emit,
            installs: Mutex::new(HashMap::new()),
        }
    }

    fn installs(&self) -> std::sync::MutexGuard<'_, HashMap<String, Install>> {
        self.installs.lock().unwrap_or_else(PoisonError::into_inner)
    }

    pub fn builds(&self) -> Vec<ProtonBuild> {
        installed_builds(&self.root, self.home.as_deref())
    }

    /// The host for a launch: the Proton builds as they are now, and the default.
    pub async fn host(&self, base: &HostEnv) -> AppResult<HostEnv> {
        let settings = db::settings::load(&self.pool, &self.defaults).await?;
        Ok(HostEnv {
            protons: self.builds(),
            default_proton: settings.library.default_proton,
            ..base.clone()
        })
    }

    // ── release list ──

    async fn fetch_releases(&self) -> Option<Vec<Release>> {
        let response = self
            .client
            .get(&self.releases_url)
            .header(ACCEPT, "application/vnd.github+json")
            .timeout(Duration::from_secs(20))
            .send()
            .await
            .ok()?;
        if !response.status().is_success() {
            eprintln!("[cove] GE-Proton-Liste: HTTP {}", response.status());
            return None;
        }
        let github: Vec<GithubRelease> = response.json().await.ok()?;
        Some(releases_from(github))
    }

    /// Fresh list from the cache, else from GitHub; an old list beats none.
    async fn release_list(&self, force: bool) -> AppResult<ReleaseList> {
        let cached = db::catalog_cache::get(&self.pool, CACHE_KEY)
            .await?
            .and_then(|entry| {
                let releases: Vec<Release> = serde_json::from_str(&entry.value).ok()?;
                Some((releases, entry.fetched_at))
            });
        if let Some((releases, fetched_at)) = &cached {
            let fresh = Utc::now() - *fetched_at < chrono::Duration::hours(LIST_TTL_HOURS);
            if fresh && !force {
                return Ok(ReleaseList {
                    releases: releases.clone(),
                    checked_at: Some(*fetched_at),
                    offline: false,
                });
            }
        }
        if let Some(releases) = self.fetch_releases().await {
            db::catalog_cache::put(
                &self.pool,
                CACHE_PROVIDER,
                CACHE_KEY,
                &serde_json::to_string(&releases)?,
            )
            .await?;
            return Ok(ReleaseList {
                releases,
                checked_at: Some(Utc::now()),
                offline: false,
            });
        }
        Ok(match cached {
            Some((releases, fetched_at)) => ReleaseList {
                releases,
                checked_at: Some(fetched_at),
                offline: true,
            },
            None => ReleaseList {
                releases: Vec::new(),
                checked_at: None,
                offline: true,
            },
        })
    }

    // ── which games use what ──

    /// Library games per build id they start with ("latest" resolved).
    async fn usage(&self, builds: &[ProtonBuild]) -> AppResult<(HashMap<String, i64>, bool)> {
        let settings = db::settings::load(&self.pool, &self.defaults).await?;
        let mut counts = HashMap::new();
        let mut latest_used = false;
        for wanted in db::library::proton_choices(&self.pool).await? {
            let id = wanted.unwrap_or_else(|| settings.library.default_proton.clone());
            latest_used |= id == LATEST;
            let id = resolve(builds, &id).map_or(id, |b| b.id.clone());
            *counts.entry(id).or_insert(0) += 1;
        }
        Ok((counts, latest_used))
    }

    // ── overview ──

    pub async fn overview(&self, force: bool) -> AppResult<ProtonOverview> {
        let list = self.release_list(force).await?;
        let builds = self.builds();
        let (usage, _) = self.usage(&builds).await?;
        let installing: Vec<ProtonProgress> = self
            .installs()
            .values()
            .map(|i| i.progress.clone())
            .collect();
        let busy = |id: &str| installing.iter().any(|p| p.id == id);
        let games = |id: &str| usage.get(id).copied().unwrap_or(0);

        let mut ge: Vec<ProtonVersion> = list
            .releases
            .iter()
            .map(|r| ProtonVersion {
                id: r.name.clone(),
                name: r.name.clone(),
                kind: ProtonKind::Ge,
                installed: false,
                download_bytes: i64::try_from(r.archive_size).ok(),
                released_at: r.published_at.clone(),
                games: 0,
                removable: false,
            })
            .collect();
        for build in builds.iter().filter(|b| is_ge(&b.id)) {
            if !ge.iter().any(|v| v.id == build.id) {
                ge.push(ProtonVersion {
                    id: build.id.clone(),
                    name: build.id.clone(),
                    kind: ProtonKind::Ge,
                    installed: true,
                    download_bytes: None,
                    released_at: None,
                    games: 0,
                    removable: false,
                });
            }
        }
        for version in &mut ge {
            version.installed = builds.iter().any(|b| b.id == version.id);
            version.games = games(&version.id);
            version.removable = version.installed && version.games == 0 && !busy(&version.id);
        }
        ge.sort_by_key(|v| std::cmp::Reverse(version_key(&v.id)));

        let mut steam: Vec<ProtonVersion> = builds
            .iter()
            .filter(|b| b.id.starts_with(STEAM_PREFIX))
            .map(|b| ProtonVersion {
                id: b.id.clone(),
                name: display_name(&b.id).to_owned(),
                kind: ProtonKind::Steam,
                installed: true,
                download_bytes: None,
                released_at: None,
                games: games(&b.id),
                removable: false,
            })
            .collect();
        steam.sort_by_key(|v| std::cmp::Reverse(version_key(&v.name)));

        let latest = list
            .releases
            .first()
            .map(|r| r.name.clone())
            .or_else(|| resolve(&builds, LATEST).map(|b| b.id.clone()));
        ge.extend(steam);
        Ok(ProtonOverview {
            versions: ge,
            latest,
            checked_at: list
                .checked_at
                .map(|t| t.to_rfc3339_opts(chrono::SecondsFormat::Secs, true)),
            offline: list.offline,
            installing,
        })
    }

    // ── installing ──

    /// Path of the build `id` stands for; a missing GE-Proton build is installed first.
    pub async fn ensure(self: &Arc<Self>, id: &str) -> AppResult<PathBuf> {
        if let Some(build) = resolve(&self.builds(), id) {
            return Ok(build.path.clone());
        }
        if id.starts_with(STEAM_PREFIX) {
            return Err(AppError::CompatToolNotFound(display_name(id).to_owned()));
        }
        let name = if id == LATEST {
            self.release_list(false)
                .await?
                .releases
                .first()
                .map(|r| r.name.clone())
                .ok_or_else(unreachable_error)?
        } else if is_ge(id) {
            id.to_owned()
        } else {
            return Err(AppError::CompatToolNotFound(id.to_owned()));
        };
        self.install(&name, id == LATEST).await
    }

    /// Installs a GE-Proton release (or waits for the install already running).
    pub async fn install(self: &Arc<Self>, name: &str, auto: bool) -> AppResult<PathBuf> {
        let mut done = self.start_install(name, auto)?;
        let result = done
            .wait_for(Option::is_some)
            .await
            .map_err(|_| AppError::Download(interrupted()))?
            .clone();
        match result {
            Some(Ok(path)) => Ok(path),
            Some(Err(message)) => Err(AppError::Download(message)),
            None => Err(AppError::Download(interrupted())),
        }
    }

    /// Starts installing in the background, unless it already runs.
    pub fn start_install(
        self: &Arc<Self>,
        name: &str,
        auto: bool,
    ) -> AppResult<watch::Receiver<InstallResult>> {
        if !is_ge(name) {
            return Err(AppError::InvalidInput(tr!(
                "Das ist keine GE-Proton-Version.",
                "That's not a GE-Proton version."
            )));
        }
        let mut installs = self.installs();
        if let Some(running) = installs.get(name) {
            return Ok(running.done.clone());
        }
        let (tx, rx) = watch::channel(None);
        let progress = ProtonProgress {
            id: name.to_owned(),
            phase: ProtonPhase::Downloading,
            downloaded_bytes: 0,
            total_bytes: 0,
            error: None,
        };
        (self.emit)(progress.clone());
        installs.insert(
            name.to_owned(),
            Install {
                done: rx.clone(),
                progress,
            },
        );
        drop(installs);
        let me = Arc::clone(self);
        let name = name.to_owned();
        tokio::spawn(async move {
            let result = me.download_and_unpack(&name, auto).await;
            let total = me
                .installs()
                .remove(&name)
                .map_or(0, |i| i.progress.total_bytes);
            let (phase, error) = match &result {
                Ok(_) => (ProtonPhase::Done, None),
                Err(error) => {
                    eprintln!("[cove] {name} konnte nicht installiert werden: {error}");
                    (ProtonPhase::Failed, Some(error.user_message()))
                }
            };
            (me.emit)(ProtonProgress {
                id: name.clone(),
                phase,
                downloaded_bytes: if result.is_ok() { total } else { 0 },
                total_bytes: total,
                error: error.clone(),
            });
            let _ = tx.send(Some(result.map_err(|e| e.user_message())));
        });
        Ok(rx)
    }

    fn report(&self, name: &str, phase: ProtonPhase, downloaded: u64, total: u64) {
        let progress = ProtonProgress {
            id: name.to_owned(),
            phase,
            downloaded_bytes: i64::try_from(downloaded).unwrap_or(i64::MAX),
            total_bytes: i64::try_from(total).unwrap_or(i64::MAX),
            error: None,
        };
        if let Some(install) = self.installs().get_mut(name) {
            install.progress = progress.clone();
        }
        (self.emit)(progress);
    }

    async fn release(&self, name: &str) -> AppResult<Release> {
        let find = |list: ReleaseList| list.releases.into_iter().find(|r| r.name == name);
        if let Some(release) = find(self.release_list(false).await?) {
            return Ok(release);
        }
        let list = self.release_list(true).await?;
        let offline = list.offline;
        find(list).ok_or_else(|| {
            if offline {
                unreachable_error()
            } else {
                AppError::InvalidInput(tr!(
                    "{name} gibt es nicht (mehr) zum Herunterladen.",
                    "{name} is not (or no longer) available for download."
                ))
            }
        })
    }

    async fn checksum(&self, release: &Release) -> AppResult<String> {
        let response = self
            .client
            .get(&release.checksum_url)
            .timeout(Duration::from_secs(30))
            .send()
            .await
            .map_err(transfer::http_error)?;
        if !response.status().is_success() {
            return Err(transfer::status_error(response.status()));
        }
        let text = response.text().await.map_err(transfer::http_error)?;
        parse_checksum(&text).ok_or_else(|| {
            AppError::Download(tr!(
                "Die Prüfsumme von {} ist unlesbar.",
                "The checksum of {} can't be read.",
                release.name
            ))
        })
    }

    async fn download_and_unpack(&self, name: &str, auto: bool) -> AppResult<PathBuf> {
        let target = self.root.join(name);
        if target.join("proton").is_file() {
            return Ok(target);
        }
        let release = self.release(name).await?;
        let expected = self.checksum(&release).await?;
        let work = self.root.join(WORK_DIR);
        tokio::fs::create_dir_all(&work).await?;
        let archive = work.join(format!("{name}.tar.gz"));
        let unpacked = work.join(name);
        let result = self
            .fetch_verify_unpack(&release, &expected, &archive, &unpacked)
            .await;
        let _ = tokio::fs::remove_file(&archive).await;
        let outcome = match result {
            Ok(()) => {
                let inner = unpacked.join(name);
                if inner.join("proton").is_file() {
                    if target.exists() {
                        tokio::fs::remove_dir_all(&target).await?;
                    }
                    tokio::fs::rename(&inner, &target).await?;
                    if auto {
                        tokio::fs::write(target.join(AUTO_MARKER), b"").await?;
                    }
                    Ok(target)
                } else {
                    Err(AppError::Download(tr!(
                        "Das Archiv von {name} enthält kein Proton.",
                        "The archive of {name} contains no Proton."
                    )))
                }
            }
            Err(error) => Err(error),
        };
        let _ = tokio::fs::remove_dir_all(&unpacked).await;
        outcome
    }

    async fn fetch_verify_unpack(
        &self,
        release: &Release,
        expected: &str,
        archive: &Path,
        unpacked: &Path,
    ) -> AppResult<()> {
        let name = release.name.as_str();
        let settings = db::settings::load(&self.pool, &self.defaults).await?;
        let limiter = RateLimiter::new(
            settings
                .downloads
                .bandwidth_limit_bytes_per_second
                .and_then(|b| u64::try_from(b).ok()),
        );
        let remote = transfer::probe(&self.client, &release.archive_url).await?;
        let total = remote.total.unwrap_or(release.archive_size);
        let progress = Progress::default();
        // Proton installs are never paused; the sender just has to live.
        let (_control, control) = watch::channel(Control::Run);
        let job = Job {
            client: &self.client,
            url: &release.archive_url,
            part: archive,
            remote: &remote,
            chunk_size: CHUNK_SIZE,
            limiter: &limiter,
            progress: &progress,
            control,
        };
        let reporter = async {
            let mut ticker = tokio::time::interval(PROGRESS_INTERVAL);
            loop {
                ticker.tick().await;
                let bytes = progress.bytes.load(Ordering::Relaxed);
                self.report(name, ProtonPhase::Downloading, bytes, total);
            }
        };
        let outcome = tokio::select! {
            outcome = job.run() => outcome?,
            () = reporter => unreachable!("the reporter runs until dropped"),
        };
        if outcome != Outcome::Completed {
            return Err(AppError::Download(interrupted()));
        }

        self.report(name, ProtonPhase::Verifying, total, total);
        let path = archive.to_path_buf();
        let actual = tokio::task::spawn_blocking(move || sha512_file(&path))
            .await
            .map_err(|e| AppError::Download(e.to_string()))??;
        if actual != expected {
            return Err(AppError::Download(tr!(
                "Der Download von {name} ist beschädigt (Prüfsumme stimmt nicht). Versuch es noch einmal.",
                "The download of {name} is damaged (checksum mismatch). Please try again."
            )));
        }

        self.report(name, ProtonPhase::Unpacking, total, total);
        if unpacked.exists() {
            tokio::fs::remove_dir_all(unpacked).await?;
        }
        let (archive, dest) = (archive.to_path_buf(), unpacked.to_path_buf());
        tokio::task::spawn_blocking(move || unpack(&archive, &dest))
            .await
            .map_err(|e| AppError::Download(e.to_string()))?
    }

    // ── updates and housekeeping ──

    /// Installs the newest GE-Proton when a game uses "latest", then removes
    /// older builds Cove installed for "latest" that no game needs any more
    /// (only when `may_clean`, i.e. no game is running).
    pub async fn check_updates(self: &Arc<Self>, force: bool, may_clean: bool) -> AppResult<()> {
        let list = self.release_list(force).await?;
        let (_, latest_used) = self.usage(&self.builds()).await?;
        if !latest_used {
            return Ok(());
        }
        let Some(newest) = list.releases.first() else {
            return Ok(());
        };
        self.install(&newest.name, true).await?;
        if may_clean {
            self.clean_up().await?;
        }
        Ok(())
    }

    async fn clean_up(&self) -> AppResult<()> {
        let builds = self.builds();
        let newest = resolve(&builds, LATEST).map(|b| b.id.clone());
        let (usage, _) = self.usage(&builds).await?;
        for build in builds.iter().filter(|b| is_ge(&b.id)) {
            let unused = usage.get(&build.id).copied().unwrap_or(0) == 0;
            if Some(&build.id) != newest.as_ref()
                && unused
                && build.path.join(AUTO_MARKER).is_file()
            {
                eprintln!("[cove] Alte Proton-Version {} entfernt", build.id);
                tokio::fs::remove_dir_all(&build.path).await?;
            }
        }
        Ok(())
    }

    /// Right after the user picked a build: install it in the background.
    pub fn prepare(self: &Arc<Self>, id: &str) {
        if id == LATEST {
            let me = Arc::clone(self);
            tokio::spawn(async move {
                if let Err(error) = me.check_updates(false, false).await {
                    eprintln!("[cove] Proton-Update: {error}");
                }
            });
        } else if is_ge(id) && resolve(&self.builds(), id).is_none() {
            let _ = self.start_install(id, false);
        }
    }

    /// Deletes a GE-Proton build Cove installed, when no game uses it.
    pub async fn remove(&self, id: &str) -> AppResult<()> {
        if id.starts_with(STEAM_PREFIX) {
            return Err(AppError::InvalidInput(tr!(
                "Proton-Versionen von Steam werden in Steam verwaltet.",
                "Proton versions from Steam are managed in Steam."
            )));
        }
        let builds = self.builds();
        let Some(build) = builds.iter().find(|b| b.id == id && is_ge(id)) else {
            return Err(AppError::InvalidInput(tr!(
                "{id} ist nicht installiert.",
                "{id} is not installed."
            )));
        };
        if self.installs().contains_key(id) {
            return Err(AppError::InvalidInput(tr!(
                "{id} wird gerade installiert.",
                "{id} is being installed right now."
            )));
        }
        let (usage, _) = self.usage(&builds).await?;
        let games = usage.get(id).copied().unwrap_or(0);
        if games > 0 {
            return Err(AppError::InvalidInput(if games == 1 {
                tr!(
                    "{id} wird noch von einem Spiel verwendet.",
                    "{id} is still used by a game."
                )
            } else {
                tr!(
                    "{id} wird noch von {games} Spielen verwendet.",
                    "{id} is still used by {games} games."
                )
            }));
        }
        tokio::fs::remove_dir_all(&build.path).await?;
        Ok(())
    }
}

fn interrupted() -> String {
    tr!(
        "Die Installation von Proton wurde unterbrochen.",
        "Installing Proton was interrupted."
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::{LaunchOptions, LibraryUpdate};
    use std::io::Write;
    use wiremock::matchers::{method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    fn build(id: &str) -> ProtonBuild {
        ProtonBuild {
            id: id.into(),
            path: PathBuf::from(format!("/p/{id}")),
        }
    }

    #[test]
    fn latest_is_the_newest_ge_build_on_the_machine() {
        let builds = [
            build("GE-Proton9-27"),
            build("GE-Proton10-3"),
            build("GE-Proton10-25"),
            build("steam:Proton 11.0"),
        ];
        assert_eq!(
            resolve(&builds, LATEST).map(|b| b.id.as_str()),
            Some("GE-Proton10-25")
        );
        assert_eq!(
            resolve(&builds, "steam:Proton 11.0").map(|b| b.id.as_str()),
            Some("steam:Proton 11.0")
        );
        assert_eq!(resolve(&builds, "GE-Proton8-1"), None);
        assert_eq!(resolve(&[build("steam:Proton 9.0")], LATEST), None);
    }

    #[test]
    fn games_ask_for_their_build_or_the_default() {
        let mut options = LaunchOptions {
            compatibility_tool: CompatibilityTool::Proton,
            ..LaunchOptions::default()
        };
        assert_eq!(wanted(&options, LATEST).as_deref(), Some(LATEST));
        options.proton_version = Some("GE-Proton10-25".into());
        assert_eq!(wanted(&options, LATEST).as_deref(), Some("GE-Proton10-25"));
        options.compatibility_tool = CompatibilityTool::Wine;
        assert_eq!(wanted(&options, LATEST), None);
    }

    #[test]
    fn only_plain_ge_names_are_accepted() {
        assert!(is_ge("GE-Proton10-25"));
        assert!(!is_ge("GE-Proton/../../etc"));
        assert!(!is_ge("Proton-9.0"));
        assert!(!is_ge(".GE-Proton"));
    }

    #[test]
    fn release_list_keeps_installable_releases_newest_first() {
        let json = serde_json::json!([
            { "tag_name": "GE-Proton9-27", "published_at": "2025-01-01T00:00:00Z", "assets": [
                { "name": "GE-Proton9-27.tar.gz", "size": 400, "browser_download_url": "https://x/9-27.tar.gz" },
                { "name": "GE-Proton9-27.sha512sum", "size": 1, "browser_download_url": "https://x/9-27.sha512sum" }
            ]},
            { "tag_name": "GE-Proton10-25", "published_at": "2026-10-01T00:00:00Z", "assets": [
                { "name": "GE-Proton10-25.tar.gz", "size": 500, "browser_download_url": "https://x/10-25.tar.gz" },
                { "name": "GE-Proton10-25.sha512sum", "size": 1, "browser_download_url": "https://x/10-25.sha512sum" }
            ]},
            { "tag_name": "GE-Proton10-26", "draft": true, "assets": [] },
            { "tag_name": "GE-Proton10-24", "assets": [
                { "name": "GE-Proton10-24.tar.gz", "size": 500, "browser_download_url": "https://x/no-checksum" }
            ]}
        ]);
        let releases = releases_from(serde_json::from_value(json).unwrap());
        let names: Vec<&str> = releases.iter().map(|r| r.name.as_str()).collect();
        assert_eq!(names, ["GE-Proton10-25", "GE-Proton9-27"]);
        assert_eq!(releases[0].archive_size, 500);
    }

    #[test]
    fn checksum_lines_are_parsed() {
        let hash = "AB".repeat(64);
        assert_eq!(
            parse_checksum(&format!("{hash}  GE-Proton10-25.tar.gz\n")),
            Some(hash.to_lowercase())
        );
        assert_eq!(parse_checksum("not a hash"), None);
    }

    /// A tiny GE-Proton archive: `<name>/proton`, a library and a symlink.
    fn archive(name: &str) -> Vec<u8> {
        let mut out = Vec::new();
        {
            let gz = flate2::write::GzEncoder::new(&mut out, flate2::Compression::fast());
            let mut tar = tar::Builder::new(gz);
            let mut add = |path: String, data: &[u8], mode: u32| {
                let mut header = tar::Header::new_gnu();
                header.set_size(data.len() as u64);
                header.set_mode(mode);
                header.set_cksum();
                tar.append_data(&mut header, path, data).unwrap();
            };
            add(format!("{name}/proton"), b"#!/usr/bin/env python3\n", 0o755);
            add(format!("{name}/files/lib/wine.so"), b"elf", 0o644);
            let mut link = tar::Header::new_gnu();
            link.set_entry_type(tar::EntryType::Symlink);
            link.set_size(0);
            link.set_mode(0o777);
            link.set_cksum();
            tar.append_link(&mut link, format!("{name}/files/lib64"), "lib")
                .unwrap();
            tar.into_inner().unwrap().finish().unwrap();
        }
        out
    }

    fn sha512(data: &[u8]) -> String {
        use sha2::{Digest, Sha512};
        to_hex(&Sha512::digest(data))
    }

    struct Fixture {
        server: MockServer,
        manager: Arc<ProtonManager>,
        pool: SqlitePool,
        root: tempfile::TempDir,
        events: Arc<Mutex<Vec<ProtonProgress>>>,
    }

    /// GitHub with the given releases; `damaged` serves an archive that does not match its checksum.
    async fn fixture(names: &[&str], damaged: bool) -> Fixture {
        let server = MockServer::start().await;
        let mut releases = Vec::new();
        for name in names {
            let data = archive(name);
            let checksum = if damaged {
                sha512(b"other")
            } else {
                sha512(&data)
            };
            releases.push(serde_json::json!({
                "tag_name": name, "published_at": "2026-10-01T00:00:00Z",
                "assets": [
                    { "name": format!("{name}.tar.gz"), "size": data.len(),
                      "browser_download_url": format!("{}/dl/{name}.tar.gz", server.uri()) },
                    { "name": format!("{name}.sha512sum"), "size": 140,
                      "browser_download_url": format!("{}/dl/{name}.sha512sum", server.uri()) }
                ]
            }));
            Mock::given(method("GET"))
                .and(path(format!("/dl/{name}.tar.gz")))
                .respond_with(ResponseTemplate::new(200).set_body_bytes(data))
                .mount(&server)
                .await;
            Mock::given(method("GET"))
                .and(path(format!("/dl/{name}.sha512sum")))
                .respond_with(
                    ResponseTemplate::new(200)
                        .set_body_string(format!("{checksum}  {name}.tar.gz\n")),
                )
                .mount(&server)
                .await;
        }
        Mock::given(method("GET"))
            .and(path("/releases"))
            .respond_with(ResponseTemplate::new(200).set_body_json(releases))
            .mount(&server)
            .await;
        let pool = db::open_in_memory().await.unwrap();
        let root = tempfile::tempdir().unwrap();
        let events = Arc::new(Mutex::new(Vec::new()));
        let sink = events.clone();
        let manager = Arc::new(ProtonManager::with_endpoint(
            pool.clone(),
            root.path().to_path_buf(),
            None,
            crate::state::default_settings(None),
            Arc::new(move |p| sink.lock().unwrap().push(p)),
            format!("{}/releases", server.uri()),
        ));
        Fixture {
            server,
            manager,
            pool,
            root,
            events,
        }
    }

    async fn proton_game(pool: &SqlitePool, id: &str, version: Option<&str>) {
        let item = db::library::add_manual(
            pool,
            &crate::models::ManualGameInput {
                title: id.into(),
                executable_path: format!("/games/{id}/game.exe"),
                size_bytes: None,
            },
        )
        .await
        .unwrap();
        db::library::update(
            pool,
            &item.game.id,
            &LibraryUpdate {
                launch_options: Some(LaunchOptions {
                    compatibility_tool: CompatibilityTool::Proton,
                    proton_version: version.map(str::to_owned),
                    ..LaunchOptions::default()
                }),
                ..LibraryUpdate::default()
            },
        )
        .await
        .unwrap();
    }

    #[tokio::test]
    async fn installs_a_release_after_checking_it() {
        let f = fixture(&["GE-Proton10-25", "GE-Proton10-24"], false).await;
        let path = f.manager.ensure("GE-Proton10-24").await.unwrap();
        assert_eq!(path, f.root.path().join("GE-Proton10-24"));
        assert!(path.join("proton").is_file());
        assert!(path.join("files/lib64").is_symlink());
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = std::fs::metadata(path.join("proton"))
                .unwrap()
                .permissions()
                .mode();
            assert_eq!(mode & 0o111, 0o111);
        }
        // Nothing is left over, and a pinned build is not one "latest" may remove.
        assert_eq!(
            std::fs::read_dir(f.root.path().join(WORK_DIR))
                .unwrap()
                .count(),
            0
        );
        assert!(!path.join(AUTO_MARKER).exists());
        let events = f.events.lock().unwrap().clone();
        assert_eq!(
            events.first().map(|e| e.phase),
            Some(ProtonPhase::Downloading)
        );
        assert_eq!(events.last().map(|e| e.phase), Some(ProtonPhase::Done));
        // Installed builds are not downloaded again.
        assert_eq!(f.manager.ensure("GE-Proton10-24").await.unwrap(), path);
        let downloads = f.server.received_requests().await.unwrap();
        let archives = downloads
            .iter()
            .filter(|r| r.url.path().ends_with(".tar.gz"))
            .count();
        assert!(archives <= 2, "probe + download, got {archives}");
    }

    #[tokio::test]
    async fn a_damaged_download_is_thrown_away() {
        let f = fixture(&["GE-Proton10-25"], true).await;
        let error = f.manager.ensure(LATEST).await.unwrap_err();
        assert!(error.user_message().contains("Prüfsumme"), "{error}");
        assert!(!f.root.path().join("GE-Proton10-25").exists());
        assert_eq!(
            std::fs::read_dir(f.root.path().join(WORK_DIR))
                .unwrap()
                .count(),
            0
        );
        let last = f.events.lock().unwrap().last().cloned().unwrap();
        assert_eq!(last.phase, ProtonPhase::Failed);
        assert!(last.error.is_some());
    }

    #[tokio::test]
    async fn parallel_requests_share_one_install() {
        let f = fixture(&["GE-Proton10-25"], false).await;
        let (a, b) = tokio::join!(
            f.manager.ensure("GE-Proton10-25"),
            f.manager.ensure("GE-Proton10-25")
        );
        assert_eq!(a.unwrap(), b.unwrap());
        let checks = f
            .server
            .received_requests()
            .await
            .unwrap()
            .iter()
            .filter(|r| r.url.path().ends_with(".sha512sum"))
            .count();
        assert_eq!(checks, 1);
    }

    #[tokio::test]
    async fn unknown_and_missing_builds_are_explained() {
        let f = fixture(&["GE-Proton10-25"], false).await;
        assert!(matches!(
            f.manager.ensure("steam:Proton 9.0").await,
            Err(AppError::CompatToolNotFound(name)) if name == "Proton 9.0"
        ));
        // A release GitHub doesn't list (any more).
        assert!(matches!(
            f.manager.ensure("GE-Proton1-1").await,
            Err(AppError::Download(message)) if message.contains("GE-Proton1-1 gibt es nicht")
        ));
        assert!(matches!(
            f.manager.ensure("wine-7").await,
            Err(AppError::CompatToolNotFound(_))
        ));
    }

    #[tokio::test]
    async fn latest_follows_new_releases_and_old_ones_are_cleaned_up() {
        let f = fixture(&["GE-Proton10-25"], false).await;
        // An older build installed for "latest", and one a game is pinned to.
        for (name, auto) in [
            ("GE-Proton10-20", true),
            ("GE-Proton9-1", false),
            ("GE-Proton9-2", true),
        ] {
            let dir = f.root.path().join(name);
            std::fs::create_dir_all(&dir).unwrap();
            std::fs::File::create(dir.join("proton"))
                .unwrap()
                .write_all(b"x")
                .unwrap();
            if auto {
                std::fs::write(dir.join(AUTO_MARKER), b"").unwrap();
            }
        }
        proton_game(&f.pool, "a", None).await; // default: latest
        proton_game(&f.pool, "b", Some("GE-Proton9-2")).await;

        f.manager.check_updates(false, true).await.unwrap();
        let mut left: Vec<String> = f.manager.builds().into_iter().map(|b| b.id).collect();
        left.sort();
        // 10-25 is new, 9-2 is pinned by a game, 9-1 was installed by hand; 10-20 is gone.
        assert_eq!(left, ["GE-Proton10-25", "GE-Proton9-1", "GE-Proton9-2"]);
        assert!(f
            .root
            .path()
            .join("GE-Proton10-25")
            .join(AUTO_MARKER)
            .is_file());

        let overview = f.manager.overview(false).await.unwrap();
        assert_eq!(overview.latest.as_deref(), Some("GE-Proton10-25"));
        assert!(!overview.offline);
        let version = |id: &str| {
            overview
                .versions
                .iter()
                .find(|v| v.id == id)
                .unwrap()
                .clone()
        };
        assert_eq!(version("GE-Proton10-25").games, 1);
        assert!(!version("GE-Proton10-25").removable);
        assert_eq!(version("GE-Proton9-2").games, 1);
        assert!(version("GE-Proton9-1").removable);

        assert!(matches!(
            f.manager.remove("GE-Proton9-2").await,
            Err(AppError::InvalidInput(_))
        ));
        f.manager.remove("GE-Proton9-1").await.unwrap();
        assert!(!f.root.path().join("GE-Proton9-1").exists());
    }

    #[tokio::test]
    async fn nothing_is_downloaded_while_no_game_uses_latest() {
        let f = fixture(&["GE-Proton10-25"], false).await;
        proton_game(&f.pool, "pinned", Some("steam:Proton 9.0")).await;
        f.manager.check_updates(false, true).await.unwrap();
        assert_eq!(f.manager.builds().len(), 0);
    }

    #[tokio::test]
    async fn the_list_is_cached_and_an_old_one_serves_when_github_is_down() {
        let f = fixture(&["GE-Proton10-25"], false).await;
        let first = f.manager.overview(false).await.unwrap();
        assert_eq!(first.versions.len(), 1);
        f.manager.overview(false).await.unwrap();
        let lists = |requests: &[wiremock::Request]| {
            requests
                .iter()
                .filter(|r| r.url.path() == "/releases")
                .count()
        };
        assert_eq!(lists(&f.server.received_requests().await.unwrap()), 1);

        f.server.reset().await;
        let offline = f.manager.overview(true).await.unwrap();
        assert!(offline.offline);
        assert_eq!(offline.versions.len(), 1);
        assert!(offline.checked_at.is_some());
    }
}
