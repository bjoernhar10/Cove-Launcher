//! Download manager: queue, parallel transfers with resume, SHA-256 check,
//! unpacking and adding the game to the library. Store games are handed to
//! their own client. All state is in SQLite, so downloads survive restarts.

pub mod amazon;
pub mod amazon_install;
pub mod epic;
mod epic_install;
pub mod extract;
pub mod gog;
mod gog_cloud;
pub mod gog_saves;
pub mod handoff;
pub mod itch;
pub mod limiter;
pub mod transfer;

use std::collections::{BTreeSet, HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::atomic::Ordering;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use sqlx::SqlitePool;
use tokio::sync::{watch, Notify};

use crate::db;
use crate::error::{AppError, AppResult};
use crate::models::{
    AppSettings, CompatibilityTool, DirectDownloadInput, DlcOption, DownloadOrigin,
    DownloadProgress, DownloadStatus, GameId, GameUpdateInfo, InstallPreview, InstallTarget,
    LibraryItem, StoreSource,
};
use crate::stores::StoreService;
use itch::ItchApi;
use limiter::RateLimiter;
use transfer::{Control, Job, Outcome, Progress, Remote};

/// Progress events per download and second (about 4).
pub const PROGRESS_INTERVAL: Duration = Duration::from_millis(250);
/// How often installs handed to a store client are checked.
const HANDOFF_POLL: Duration = Duration::from_secs(30);
const WORK_DIR: &str = ".cove-downloads";

/// Callbacks into the app (events, library follow-ups).
pub struct Hooks {
    /// The list changed (status, queue): the UI reloads it.
    pub changed: Arc<dyn Fn() + Send + Sync>,
    pub progress: Arc<dyn Fn(DownloadProgress<'_>) + Send + Sync>,
    /// A game was installed (catalogue matching, achievements).
    pub installed: Arc<dyn Fn(GameId) + Send + Sync>,
    /// Opens a URL (store client handoff).
    pub open_url: crate::launcher::OpenUrlFn,
}

pub struct DownloadManager {
    pool: SqlitePool,
    defaults: AppSettings,
    client: reqwest::Client,
    limiter: RateLimiter,
    running: Mutex<HashMap<String, watch::Sender<Control>>>,
    wake: Notify,
    pub itch: ItchApi,
    stores: Option<Arc<StoreService>>,
    hooks: Hooks,
    /// Steam uninstalls waiting for the app manifest to disappear: (game id, app id).
    uninstalls: Mutex<Vec<(GameId, String)>>,
}

fn validate_url(raw: &str) -> AppResult<String> {
    let url: reqwest::Url = raw.trim().parse().map_err(|_| {
        AppError::InvalidInput(tr!(
            "Das ist kein gültiger Link.",
            "That's not a valid link."
        ))
    })?;
    if !matches!(url.scheme(), "https" | "http") {
        return Err(AppError::InvalidInput(tr!(
            "Nur http- und https-Links werden unterstützt.",
            "Only http and https links are supported."
        )));
    }
    Ok(url.to_string())
}

fn validate_sha256(raw: Option<&str>) -> AppResult<Option<String>> {
    let Some(value) = raw.map(str::trim).filter(|s| !s.is_empty()) else {
        return Ok(None);
    };
    if value.len() != 64 || !value.chars().all(|c| c.is_ascii_hexdigit()) {
        return Err(AppError::InvalidInput(tr!(
            "Eine SHA-256-Prüfsumme hat 64 Hex-Zeichen.",
            "A SHA-256 checksum has 64 hex characters."
        )));
    }
    Ok(Some(value.to_lowercase()))
}

/// Title from a file name: "lantern-1.2_linux.tar.gz" → "Lantern 1.2 Linux".
pub fn title_from_file_name(name: &str) -> String {
    let lower = name.to_lowercase();
    let stem_len = [
        ".tar.gz",
        ".tgz",
        ".tar",
        ".zip",
        ".7z",
        ".appimage",
        ".exe",
        ".sh",
    ]
    .iter()
    .find(|ext| lower.ends_with(*ext))
    .map_or(name.len(), |ext| name.len() - ext.len());
    crate::scanner::prettify_title(&name[..stem_len])
}

impl DownloadManager {
    pub fn new(
        pool: SqlitePool,
        defaults: AppSettings,
        itch: ItchApi,
        stores: Option<Arc<StoreService>>,
        hooks: Hooks,
    ) -> Self {
        Self {
            pool,
            defaults,
            client: reqwest::Client::builder()
                .user_agent(concat!("Cove/", env!("CARGO_PKG_VERSION")))
                .gzip(false)
                .connect_timeout(Duration::from_secs(15))
                .read_timeout(Duration::from_secs(60))
                .build()
                .unwrap_or_default(),
            limiter: RateLimiter::new(None),
            running: Mutex::new(HashMap::new()),
            wake: Notify::new(),
            itch,
            stores,
            hooks,
            uninstalls: Mutex::new(Vec::new()),
        }
    }

    /// Checks the folder from the install dialog and, when asked, makes it the default.
    async fn install_folder(&self, target: &InstallTarget) -> AppResult<String> {
        let folder = crate::folders::require_install_folder(&target.folder)?
            .to_string_lossy()
            .into_owned();
        let mut settings = self.settings().await?;
        let known = settings.library.game_folders.contains(&folder);
        if target.make_default || !known {
            if target.make_default {
                settings.downloads.directory.clone_from(&folder);
            }
            settings.library.game_folders.push(folder.clone());
            db::settings::save(&self.pool, &settings).await?;
        }
        Ok(folder)
    }

    /// Sizes for the install dialog of a direct link.
    pub async fn preview_url(&self, input: &DirectDownloadInput) -> AppResult<InstallPreview> {
        let url = validate_url(&input.url)?;
        validate_sha256(input.sha256.as_deref())?;
        let remote = transfer::probe(&self.client, &url).await?;
        // Same title as `add_url` gives the download.
        let url_name = transfer::file_name_from_url(&url);
        let title = input
            .title
            .as_deref()
            .map(str::trim)
            .filter(|t| !t.is_empty())
            .map(str::to_owned)
            .or_else(|| url_name.as_deref().map(title_from_file_name))
            .unwrap_or_else(|| "Download".into());
        let file_name = remote.file_name.clone().or(url_name);
        let download_bytes = remote.total.and_then(|t| i64::try_from(t).ok());
        // A single file is installed as it is; archives are only known after unpacking.
        let archive = file_name
            .as_deref()
            .is_none_or(|name| extract::archive_kind(name) != extract::ArchiveKind::Plain);
        Ok(InstallPreview {
            title,
            download_bytes,
            install_bytes: if archive { None } else { download_bytes },
            launcher_only: None,
            dlcs: Vec::new(),
        })
    }

    /// Steam asked the user to uninstall: watch the app manifests until the game is gone.
    pub fn watch_uninstall(&self, game_id: GameId, app_id: String) {
        let mut pending = self
            .uninstalls
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if !pending.iter().any(|(id, _)| *id == game_id) {
            pending.push((game_id, app_id));
        }
    }

    async fn poll_uninstalls(&self) -> AppResult<()> {
        let pending = self
            .uninstalls
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .clone();
        let Some(stores) = &self.stores else {
            return Ok(());
        };
        if pending.is_empty() {
            return Ok(());
        }
        let local = stores.detect_local(Some(StoreSource::Steam)).await?;
        for (game_id, app_id) in pending {
            let still_there = local
                .iter()
                .any(|g| g.store == StoreSource::Steam && g.store_game_id == app_id);
            if still_there {
                continue;
            }
            db::library::mark_uninstalled(&self.pool, &game_id).await?;
            self.uninstalls
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .retain(|(id, _)| *id != game_id);
            (self.hooks.changed)();
        }
        Ok(())
    }

    async fn settings(&self) -> AppResult<AppSettings> {
        db::settings::load(&self.pool, &self.defaults).await
    }

    /// Starts the scheduler. Interrupted transfers continue where they stopped.
    pub fn start(self: &Arc<Self>) {
        let this = self.clone();
        tauri::async_runtime::spawn(async move {
            match db::downloads::requeue_interrupted(&this.pool).await {
                Ok(n) if n > 0 => {
                    eprintln!("[cove] {n} unterbrochene Download(s) werden fortgesetzt");
                }
                Ok(_) => {}
                Err(e) => eprintln!("[cove] Downloads: {e}"),
            }
            loop {
                if let Err(error) = this.schedule().await {
                    eprintln!("[cove] Downloads: {error}");
                }
                tokio::select! {
                    () = this.wake.notified() => {}
                    () = tokio::time::sleep(HANDOFF_POLL) => {
                        if let Err(error) = this.poll_handoffs().await {
                            eprintln!("[cove] Store-Installationen prüfen: {error}");
                        }
                        if let Err(error) = this.poll_uninstalls().await {
                            eprintln!("[cove] Steam-Deinstallationen prüfen: {error}");
                        }
                    }
                }
            }
        });
    }

    /// Re-reads settings (limit, parallel downloads) and starts queued transfers.
    pub fn wake(&self) {
        self.wake.notify_one();
    }

    fn running_ids(&self) -> Vec<String> {
        self.running
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .keys()
            .cloned()
            .collect()
    }

    async fn schedule(self: &Arc<Self>) -> AppResult<()> {
        let settings = self.settings().await?;
        self.limiter.set_rate(
            settings
                .downloads
                .bandwidth_limit_bytes_per_second
                .and_then(|b| u64::try_from(b).ok()),
        );
        let max = usize::try_from(settings.downloads.max_concurrent.max(1)).unwrap_or(1);
        let running = self.running_ids();
        let free = max.saturating_sub(running.len());
        for id in db::downloads::queued(&self.pool)
            .await?
            .into_iter()
            .filter(|id| !running.contains(id))
            .take(free)
        {
            let (tx, rx) = watch::channel(Control::Run);
            self.running
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .insert(id.clone(), tx);
            let this = self.clone();
            tauri::async_runtime::spawn(async move { this.run(id, rx).await });
        }
        Ok(())
    }

    async fn run(self: Arc<Self>, id: String, control: watch::Receiver<Control>) {
        let result = self.transfer(&id, control).await;
        self.running
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .remove(&id);
        match result {
            Ok(Outcome::Completed | Outcome::Paused) => {}
            Ok(Outcome::Cancelled) => {
                if let Ok(record) = db::downloads::record(&self.pool, &id).await {
                    Self::remove_files(&record);
                }
            }
            Err(error) => {
                eprintln!("[cove] Download {id}: {error}");
                let message = match &error {
                    AppError::Download(m) | AppError::Itch(m) | AppError::Offline(m) => m.clone(),
                    other => other.user_message(),
                };
                if let Err(e) = db::downloads::fail(&self.pool, &id, &message).await {
                    eprintln!("[cove] Download {id}: {e}");
                }
            }
        }
        (self.hooks.changed)();
        self.wake();
    }

    fn work_dir(record: &db::downloads::Record) -> Option<PathBuf> {
        record
            .target_dir
            .as_ref()
            .map(|d| Path::new(d).join(WORK_DIR))
    }

    fn part_path(record: &db::downloads::Record) -> Option<PathBuf> {
        Self::work_dir(record).map(|d| d.join(format!("{}.part", record.id)))
    }

    fn remove_files(record: &db::downloads::Record) {
        if let Some(part) = Self::part_path(record) {
            let _ = std::fs::remove_file(part);
        }
        if let Some(work) = Self::work_dir(record) {
            let _ = std::fs::remove_dir_all(work.join(&record.id));
            let _ = std::fs::remove_dir(work); // only if empty
        }
    }

    async fn set_status(&self, id: &str, status: DownloadStatus) -> AppResult<()> {
        db::downloads::set_status(&self.pool, id, status).await?;
        (self.hooks.changed)();
        Ok(())
    }

    /// Checks the SHA-256 sum when the user gave one; a mismatch deletes the file.
    async fn verify(&self, id: &str, expected: Option<&str>, part: &Path) -> AppResult<()> {
        let Some(expected) = expected else {
            return Ok(());
        };
        let path = part.to_path_buf();
        let actual = tokio::task::spawn_blocking(move || transfer::sha256_file(&path))
            .await
            .map_err(background_failed(tr!(
                "Die Datei konnte nicht geprüft werden.",
                "The file could not be verified."
            )))??;
        if actual != expected {
            let _ = tokio::fs::remove_file(part).await;
            db::downloads::save_progress(&self.pool, id, 0, &[]).await?;
            return Err(AppError::Download(tr!(
                "Die Prüfsumme stimmt nicht. Die Datei ist beschädigt oder wurde verändert und wurde gelöscht.",
                "The checksum doesn't match. The file is damaged or was changed, and has been deleted."
            )));
        }
        db::downloads::set_checksum_verified(&self.pool, id).await
    }

    async fn transfer(
        &self,
        id: &str,
        mut control: watch::Receiver<Control>,
    ) -> AppResult<Outcome> {
        let record = db::downloads::record(&self.pool, id).await?;
        let work_dir = Self::work_dir(&record).ok_or_else(|| {
            AppError::Download(tr!("Kein Zielordner festgelegt.", "No target folder set."))
        })?;
        tokio::fs::create_dir_all(&work_dir).await?;
        let part = work_dir.join(format!("{id}.part"));
        self.set_status(id, DownloadStatus::Downloading).await?;

        // GOG: a fresh plan on every start (its CDN links expire). Native Linux
        // games are one installer file; everything else goes chunk by chunk.
        let mut gog_installer = None;
        let url = match DownloadOrigin::parse(&record.origin) {
            DownloadOrigin::Epic => return self.run_epic(&record, control).await,
            DownloadOrigin::Amazon => return self.run_amazon(&record, control).await,
            DownloadOrigin::Gog => {
                let excluded =
                    db::store_installs::excluded_dlcs(&self.pool, &record.game_id).await?;
                let plan = self.gog_plan(&record.game_id).await?.without(&excluded);
                match plan.installer.clone() {
                    Some(installer) => {
                        gog_installer = Some(plan);
                        installer.url
                    }
                    None => return self.run_gog(&record, &plan, control).await,
                }
            }
            _ => self.file_url(&record).await?,
        };
        let remote = tokio::select! {
            remote = transfer::probe(&self.client, &url) => remote?,
            Ok(signal) = control.wait_for(|c| *c != Control::Run) => {
                let cancelled = *signal == Control::Cancel;
                return Ok(if cancelled { Outcome::Cancelled } else { Outcome::Paused });
            }
        };
        let file_name = record
            .file_name
            .clone()
            .or_else(|| remote.file_name.clone())
            .unwrap_or_else(|| "download.bin".into());

        // A different file version (or size) on the server: start over.
        let total = remote
            .total
            .and_then(|t| i64::try_from(t).ok())
            .unwrap_or(0);
        let mut done: BTreeSet<u64> = serde_json::from_str(&record.chunks_done).unwrap_or_default();
        let changed = record.etag.is_some() && record.etag != remote.version
            || record.total_bytes != 0 && record.total_bytes != total;
        if changed || !remote.ranges || !part.exists() {
            done.clear();
        }
        db::downloads::set_remote(&self.pool, id, total, remote.version.as_deref(), &file_name)
            .await?;

        let chunk_size = u64::try_from(record.chunk_size.max(1 << 20)).unwrap_or(8 << 20);
        let progress = Arc::new(Progress::default());
        *progress
            .chunks_done
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner) = done;
        let outcome = {
            let reporter = self.report_progress(id, total, progress.clone());
            let job = Job {
                client: &self.client,
                url: &url,
                part: &part,
                remote: &Remote {
                    file_name: Some(file_name.clone()),
                    ..remote.clone()
                },
                chunk_size,
                limiter: &self.limiter,
                progress: &progress,
                control: control.clone(),
            };
            let outcome = tokio::select! {
                outcome = job.run() => outcome,
                () = reporter => unreachable!("the reporter runs until dropped"),
            };
            self.save_progress(id, &progress).await?;
            outcome?
        };
        if outcome != Outcome::Completed {
            return Ok(outcome);
        }

        // Verify.
        self.set_status(id, DownloadStatus::Verifying).await?;
        self.verify(id, record.checksum_sha256.as_deref(), &part)
            .await?;
        if *control.borrow_and_update() == Control::Cancel {
            return Ok(Outcome::Cancelled);
        }

        if let Some(plan) = gog_installer {
            self.install_gog_installer(&record, &part, &plan).await?;
        } else {
            self.install(&record, &part, &file_name).await?;
        }
        let _ = std::fs::remove_dir(&work_dir); // only if empty
        Ok(Outcome::Completed)
    }

    /// Direct links are stored; itch.io links are resolved right before each start.
    async fn file_url(&self, record: &db::downloads::Record) -> AppResult<String> {
        if DownloadOrigin::parse(&record.origin) == DownloadOrigin::Itch {
            let upload = record
                .itch_upload_id
                .ok_or_else(|| AppError::Itch(tr!("Unbekannte Datei.", "Unknown file.")))?;
            return self
                .itch
                .resolve_download(upload, record.itch_download_key_id)
                .await;
        }
        record
            .url
            .clone()
            .ok_or_else(|| AppError::Download(tr!("Kein Link vorhanden.", "No link given.")))
    }

    /// Unpacks into the game folder, finds the executable and adds the game to the library.
    async fn install(
        &self,
        record: &db::downloads::Record,
        part: &Path,
        file_name: &str,
    ) -> AppResult<()> {
        // Install.
        self.set_status(&record.id, DownloadStatus::Extracting)
            .await?;
        if record.extra.is_some() {
            return self.install_extra(record, part, file_name).await;
        }
        let target = PathBuf::from(record.target_dir.clone().unwrap_or_default());
        let fixed = record.install_path.clone().map(PathBuf::from);
        let title = record.title.clone();
        let source = part.to_path_buf();
        let name = file_name.to_owned();
        let (install_dir, executable, size) =
            tokio::task::spawn_blocking(move || -> AppResult<_> {
                let dir = fixed.unwrap_or_else(|| extract::unique_dir(&target, &title));
                extract::unpack(&source, &name, &dir)?;
                let _ = std::fs::remove_file(&source);
                let executable = extract::find_executable(&dir);
                Ok((dir.clone(), executable, extract::dir_size(&dir)))
            })
            .await
            .map_err(background_failed(tr!(
                "Das Spiel konnte nicht entpackt werden.",
                "The game could not be unpacked."
            )))??;
        let (exe, compat) = executable.map_or(
            (None, crate::models::CompatibilityTool::Native),
            |(e, c)| (Some(e), c),
        );
        db::downloads::complete_install(
            &self.pool,
            &record.id,
            &record.game_id,
            &install_dir.to_string_lossy(),
            exe.as_deref(),
            i64::try_from(size).unwrap_or(i64::MAX),
            compat,
        )
        .await?;
        (self.hooks.installed)(record.game_id.clone());
        Ok(())
    }

    /// An itch.io extra: unpacked (archives) or copied into `<game>/Extras`.
    /// It never becomes a library entry of its own.
    async fn install_extra(
        &self,
        record: &db::downloads::Record,
        part: &Path,
        file_name: &str,
    ) -> AppResult<()> {
        let game_dir = Self::game_dir(record)?;
        let source = part.to_path_buf();
        let name = file_name.to_owned();
        let placed = tokio::task::spawn_blocking(move || -> AppResult<PathBuf> {
            let extras = game_dir.join("Extras");
            std::fs::create_dir_all(&extras)?;
            let placed = if extract::archive_kind(&name) == extract::ArchiveKind::Plain {
                let to = extras.join(crate::folders::folder_name(&name));
                std::fs::copy(&source, &to)?;
                to
            } else {
                let stem = name
                    .trim_end_matches(".tar.gz")
                    .rsplit_once('.')
                    .map_or(name.as_str(), |(stem, _)| stem)
                    .to_owned();
                let dir = extract::unique_dir(&extras, &stem);
                extract::unpack(&source, &name, &dir)?;
                dir
            };
            let _ = std::fs::remove_file(&source);
            Ok(placed)
        })
        .await
        .map_err(background_failed(tr!(
            "Das Extra konnte nicht entpackt werden.",
            "The extra could not be unpacked."
        )))??;
        db::downloads::complete_extra(&self.pool, &record.id, &placed.to_string_lossy()).await
    }

    async fn save_progress(&self, id: &str, progress: &Progress) -> AppResult<()> {
        let chunks: Vec<u64> = progress
            .chunks_done
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .iter()
            .copied()
            .collect();
        let bytes = i64::try_from(progress.bytes.load(Ordering::Relaxed)).unwrap_or(i64::MAX);
        db::downloads::save_progress(&self.pool, id, bytes, &chunks).await
    }

    /// Emits progress every 250 ms and saves finished chunks for resuming. Never returns.
    #[allow(clippy::cast_precision_loss)] // byte counts far below 2^52
    async fn report_progress(&self, id: &str, total: i64, progress: Arc<Progress>) {
        let mut last_bytes = progress.bytes.load(Ordering::Relaxed);
        let mut saved_chunks = 0usize;
        let mut speed = 0.0f64;
        let mut ticker = tokio::time::interval(PROGRESS_INTERVAL);
        ticker.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
        loop {
            ticker.tick().await;
            let bytes = progress.bytes.load(Ordering::Relaxed);
            let delta = bytes.saturating_sub(last_bytes) as f64 / PROGRESS_INTERVAL.as_secs_f64();
            last_bytes = bytes;
            // Smoothed so the number does not jump around.
            speed = if speed == 0.0 {
                delta
            } else {
                speed * 0.7 + delta * 0.3
            };
            (self.hooks.progress)(DownloadProgress {
                id,
                status: DownloadStatus::Downloading,
                downloaded_bytes: i64::try_from(bytes).unwrap_or(i64::MAX),
                total_bytes: total,
                #[allow(clippy::cast_possible_truncation)]
                bytes_per_second: speed.round() as i64,
            });
            let chunk_count = progress
                .chunks_done
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .len();
            if chunk_count != saved_chunks {
                saved_chunks = chunk_count;
                if let Err(error) = self.save_progress(id, &progress).await {
                    eprintln!("[cove] Download {id}: {error}");
                }
            }
        }
    }

    // ───────────── GOG ─────────────

    fn stores(&self) -> AppResult<&Arc<StoreService>> {
        self.stores.as_ref().ok_or_else(|| {
            AppError::ConnectorsUnavailable(tr!(
                "Die Store-Verbindungen sind nicht verfügbar.",
                "The store connections aren't available."
            ))
        })
    }

    /// Store and store id of a game Cove can install itself (GOG, Epic).
    async fn store_game(&self, game_id: &str) -> AppResult<(LibraryItem, StoreSource, String)> {
        let item = db::library::require(&self.pool, game_id).await?;
        match (item.entry.source, item.entry.store_game_id.clone()) {
            (store @ (StoreSource::Gog | StoreSource::Epic | StoreSource::Amazon), Some(id)) => {
                Ok((item, store, id))
            }
            _ => Err(AppError::InvalidInput(tr!(
                "Cove kann nur GOG-, Epic- und Amazon-Spiele selbst installieren.",
                "Cove can only install GOG, Epic and Amazon games itself."
            ))),
        }
    }

    async fn language(&self) -> AppResult<String> {
        Ok(self.settings().await?.general.language)
    }

    async fn gog_plan(&self, game_id: &str) -> AppResult<gog::GogPlan> {
        let (_, _, product) = self.store_game(game_id).await?;
        let language = self.language().await?;
        self.stores()?
            .gog_plan(&product, gog::host_platform(), &language)
            .await
    }

    /// Sizes for the install dialog (and why the store's launcher must do it, if so).
    /// The sizes are the game alone; each DLC carries its own.
    pub async fn preview_store(&self, game_id: &str) -> AppResult<InstallPreview> {
        let (item, store, app) = self.store_game(game_id).await?;
        let bytes = |b: u64| i64::try_from(b).ok();
        let excluded = db::store_installs::excluded_dlcs(&self.pool, game_id).await?;
        let option = |id: &str, title: &str, (download, install): (u64, u64)| DlcOption {
            selected: !excluded.contains(id),
            id: id.to_owned(),
            title: title.to_owned(),
            download_bytes: bytes(download),
            install_bytes: bytes(install),
        };
        if store == StoreSource::Amazon {
            let (download, install) = self.preview_amazon(&app).await?;
            return Ok(InstallPreview {
                title: item.game.title,
                download_bytes: bytes(download),
                install_bytes: bytes(install),
                launcher_only: None,
                dlcs: Vec::new(),
            });
        }
        if store == StoreSource::Epic {
            let info = self
                .stores()?
                .epic_install_info(&app, gog::host_platform())
                .await?;
            let (manifest, _) = epic::fetch_manifest(&self.client, &info.manifest_urls).await?;
            let launcher_only = epic::launcher_only(&info, &manifest);
            let mut dlcs = Vec::new();
            if launcher_only.is_none() {
                for dlc in &info.dlcs {
                    let (manifest, _) =
                        epic::fetch_manifest(&self.client, &dlc.manifest_urls).await?;
                    let size = (
                        manifest.download_size(manifest.chunks.keys()),
                        manifest.install_size(),
                    );
                    dlcs.push(option(&dlc.app_name, &dlc.title, size));
                }
            }
            return Ok(InstallPreview {
                title: item.game.title,
                download_bytes: bytes(manifest.download_size(manifest.chunks.keys())),
                install_bytes: bytes(manifest.install_size()),
                launcher_only,
                dlcs,
            });
        }
        let plan = self.gog_plan(game_id).await?;
        let dlcs: Vec<DlcOption> = plan
            .dlcs
            .iter()
            .map(|d| option(&d.product_id, &d.title, plan.product_size(&d.product_id)))
            .collect();
        let all: HashSet<String> = plan.dlcs.iter().map(|d| d.product_id.clone()).collect();
        let game = plan.without(&all);
        Ok(InstallPreview {
            title: item.game.title,
            download_bytes: bytes(game.download_bytes),
            install_bytes: bytes(game.install_bytes),
            launcher_only: None,
            dlcs,
        })
    }

    /// Queues a GOG or Epic game into `<folder>/<game name>`.
    pub async fn install_store(&self, game_id: &str, target: &InstallTarget) -> AppResult<String> {
        let (item, store, _) = self.store_game(game_id).await?;
        let folder = self.install_folder(target).await?;
        let dir = crate::folders::unique_dir(Path::new(&folder), &item.game.title);
        db::store_installs::set_excluded_dlcs(&self.pool, game_id, &target.excluded_dlcs).await?;
        let id = db::downloads::enqueue_store_install(
            &self.pool,
            game_id,
            store,
            &folder,
            &dir.to_string_lossy(),
        )
        .await?;
        (self.hooks.changed)();
        self.wake();
        Ok(id)
    }

    /// Installed and newest version of a store game Cove installed.
    pub async fn check_update(&self, game_id: &str) -> AppResult<GameUpdateInfo> {
        let (_, store, product) = self.store_game(game_id).await?;
        let installed = db::store_installs::get(&self.pool, game_id)
            .await?
            .ok_or_else(|| {
                AppError::InvalidInput(tr!(
                    "Cove hat dieses Spiel nicht installiert.",
                    "Cove didn't install this game."
                ))
            })?;
        if store == StoreSource::Amazon {
            let latest = self
                .amazon_versions(&product)
                .await?
                .unwrap_or_else(|| installed.build_id.clone());
            return Ok(GameUpdateInfo {
                available: latest != installed.build_id,
                installed_version: installed.version,
                latest_version: latest,
            });
        }
        if store == StoreSource::Epic {
            let info = self
                .stores()?
                .epic_install_info(&product, &installed.platform)
                .await?;
            // A DLC update, a new DLC or one no longer owned also counts.
            let have: std::collections::BTreeMap<String, String> =
                db::store_installs::dlcs(&self.pool, game_id)
                    .await?
                    .into_iter()
                    .map(|d| (d.app_name, d.build_id))
                    .collect();
            let excluded = db::store_installs::excluded_dlcs(&self.pool, game_id).await?;
            let want: std::collections::BTreeMap<String, String> = info
                .dlcs
                .iter()
                .filter(|d| !excluded.contains(&d.app_name))
                .map(|d| (d.app_name.clone(), d.build_version.clone()))
                .collect();
            let game_changed = info.build_version != installed.build_id;
            return Ok(GameUpdateInfo {
                available: game_changed || have != want,
                installed_version: installed.version,
                latest_version: if game_changed || have == want {
                    info.build_version
                } else {
                    tr!(
                        "{} mit geänderten DLCs",
                        "{} with changed DLCs",
                        info.build_version
                    )
                },
            });
        }
        let language = self.language().await?;
        let latest = self
            .stores()?
            .gog_latest(&product, &installed.platform, &language)
            .await?;
        Ok(GameUpdateInfo {
            available: latest.build_id != installed.build_id,
            installed_version: installed.version,
            latest_version: latest.version,
        })
    }

    /// Queues the update of a store game into its current folder (only changed parts load).
    pub async fn update_store(&self, game_id: &str) -> AppResult<String> {
        let (item, store, _) = self.store_game(game_id).await?;
        let dir = item
            .entry
            .install_path
            .filter(|_| item.entry.installed_by_cove)
            .map(PathBuf::from)
            .ok_or_else(|| {
                AppError::InvalidInput(tr!(
                    "Cove hat dieses Spiel nicht installiert.",
                    "Cove didn't install this game."
                ))
            })?;
        let folder = dir.parent().unwrap_or(&dir).to_string_lossy().into_owned();
        let id = db::downloads::enqueue_store_install(
            &self.pool,
            game_id,
            store,
            &folder,
            &dir.to_string_lossy(),
        )
        .await?;
        (self.hooks.changed)();
        self.wake();
        Ok(id)
    }

    /// "DLCs ändern": remembers the new choice and queues an update that
    /// downloads newly ticked DLCs and removes unticked ones.
    pub async fn change_dlcs(&self, game_id: &str, excluded: &[String]) -> AppResult<String> {
        self.store_game(game_id).await?;
        let previous: Vec<String> = db::store_installs::excluded_dlcs(&self.pool, game_id)
            .await?
            .into_iter()
            .collect();
        db::store_installs::set_excluded_dlcs(&self.pool, game_id, excluded).await?;
        match self.update_store(game_id).await {
            Ok(id) => Ok(id),
            Err(error) => {
                // Nothing was queued: keep the choice that matches the disk.
                db::store_installs::set_excluded_dlcs(&self.pool, game_id, &previous).await?;
                Err(error)
            }
        }
    }

    /// A cancelled first store install leaves nothing behind; an update keeps the game.
    async fn remove_unfinished_game(&self, record: &db::downloads::Record) {
        let Some(dir) = record
            .install_path
            .as_deref()
            .filter(|_| matches!(record.origin.as_str(), "gog" | "epic" | "amazon"))
        else {
            return;
        };
        if matches!(
            db::store_installs::get(&self.pool, &record.game_id).await,
            Ok(None)
        ) {
            let _ = tokio::fs::remove_dir_all(dir).await;
        }
    }

    fn game_dir(record: &db::downloads::Record) -> AppResult<PathBuf> {
        record
            .install_path
            .as_ref()
            .map(PathBuf::from)
            .ok_or_else(|| {
                AppError::Download(tr!("Kein Spielordner festgelegt.", "No game folder set."))
            })
    }

    /// Downloads a GOG plan chunk by chunk into the game folder.
    async fn run_gog(
        &self,
        record: &db::downloads::Record,
        plan: &gog::GogPlan,
        control: watch::Receiver<Control>,
    ) -> AppResult<Outcome> {
        let dir = Self::game_dir(record)?;
        let work = Self::work_dir(record)
            .ok_or_else(|| {
                AppError::Download(tr!("Kein Zielordner festgelegt.", "No target folder set."))
            })?
            .join(&record.id);
        tokio::fs::create_dir_all(&work).await?;
        tokio::fs::create_dir_all(&dir).await?;
        let total = i64::try_from(plan.download_bytes).unwrap_or(i64::MAX);
        db::downloads::set_remote(
            &self.pool,
            &record.id,
            total,
            Some(&plan.build_id),
            &record.title,
        )
        .await?;
        let previous = db::store_installs::get(&self.pool, &record.game_id).await?;
        let old_files = previous
            .as_ref()
            .map(|p| p.files.clone())
            .unwrap_or_default();
        let progress = Arc::new(Progress::default());
        let outcome = {
            let reporter = self.report_progress(&record.id, total, progress.clone());
            let job = gog::Job {
                client: &self.client,
                limiter: &self.limiter,
                progress: &progress,
                control,
                plan,
                dir: &dir,
                work: &work,
            };
            let outcome = tokio::select! {
                outcome = job.run(&old_files) => outcome,
                () = reporter => unreachable!("the reporter runs until dropped"),
            };
            self.save_progress(&record.id, &progress).await?;
            outcome?
        };
        match outcome {
            Outcome::Completed => {}
            Outcome::Cancelled => {
                self.remove_unfinished_game(record).await;
                return Ok(outcome);
            }
            Outcome::Paused => return Ok(outcome),
        }
        let _ = tokio::fs::remove_dir_all(&work).await;
        if let Some(parent) = work.parent() {
            let _ = tokio::fs::remove_dir(parent).await; // only if empty
        }
        self.complete_gog(record, plan, &dir, gog::installed_files(plan))
            .await?;
        Ok(Outcome::Completed)
    }

    /// Native Linux game: GOG's installer was downloaded; unpack it into the game folder.
    async fn install_gog_installer(
        &self,
        record: &db::downloads::Record,
        part: &Path,
        plan: &gog::GogPlan,
    ) -> AppResult<()> {
        self.set_status(&record.id, DownloadStatus::Extracting)
            .await?;
        let dir = Self::game_dir(record)?;
        let (source, target) = (part.to_path_buf(), dir.clone());
        tokio::task::spawn_blocking(move || -> AppResult<()> {
            // An update replaces the whole folder content.
            let _ = std::fs::remove_dir_all(&target);
            gog::extract_installer(&source, &target)?;
            let _ = std::fs::remove_file(&source);
            Ok(())
        })
        .await
        .map_err(background_failed(tr!(
            "Das Spiel konnte nicht entpackt werden.",
            "The game could not be unpacked."
        )))??;
        self.complete_gog(record, plan, &dir, Vec::new()).await
    }

    /// The game is on disk: library entry, start file, installed build.
    async fn complete_gog(
        &self,
        record: &db::downloads::Record,
        plan: &gog::GogPlan,
        dir: &Path,
        files: Vec<String>,
    ) -> AppResult<()> {
        let executable = gog::launch_file(dir, &plan.product_id);
        let windows_game = executable
            .as_deref()
            .and_then(Path::extension)
            .is_some_and(|e| e.eq_ignore_ascii_case("exe"));
        let compat = if windows_game && !cfg!(windows) {
            CompatibilityTool::Proton
        } else {
            CompatibilityTool::Native
        };
        let size = {
            let path = dir.to_path_buf();
            tokio::task::spawn_blocking(move || extract::dir_size(&path))
                .await
                .unwrap_or(plan.install_bytes)
        };
        // Last: `complete_install` marks the download as completed.
        db::library::set_compat_if_native(&self.pool, &record.game_id, compat).await?;
        db::store_installs::save(
            &self.pool,
            &db::store_installs::StoreInstall {
                game_id: record.game_id.clone(),
                store: StoreSource::Gog,
                product_id: plan.product_id.clone(),
                build_id: plan.build_id.clone(),
                version: plan.version.clone(),
                platform: plan.platform.clone(),
                files,
                meta: "{}".into(),
            },
        )
        .await?;
        db::downloads::complete_install(
            &self.pool,
            &record.id,
            &record.game_id,
            &dir.to_string_lossy(),
            executable
                .as_deref()
                .map(|p| p.to_string_lossy())
                .as_deref(),
            i64::try_from(size).unwrap_or(i64::MAX),
            compat,
        )
        .await?;
        (self.hooks.installed)(record.game_id.clone());
        Ok(())
    }

    // ───────────── commands ─────────────

    pub async fn add_url(
        &self,
        input: &DirectDownloadInput,
        target: &InstallTarget,
    ) -> AppResult<String> {
        let url = validate_url(&input.url)?;
        let sha256 = validate_sha256(input.sha256.as_deref())?;
        let file_name = transfer::file_name_from_url(&url);
        let title = input
            .title
            .as_deref()
            .map(str::trim)
            .filter(|t| !t.is_empty())
            .map(str::to_owned)
            .or_else(|| file_name.as_deref().map(title_from_file_name))
            .unwrap_or_else(|| "Download".into());
        let target_dir = self.install_folder(target).await?;
        let id = db::downloads::enqueue_transfer(
            &self.pool,
            &db::downloads::NewTransfer {
                title,
                origin: Some(DownloadOrigin::Direct),
                url: Some(url),
                sha256,
                target_dir,
                ..db::downloads::NewTransfer::default()
            },
        )
        .await?;
        (self.hooks.changed)();
        self.wake();
        Ok(id)
    }

    pub async fn add_itch(
        &self,
        choice: &db::downloads::ItchChoice,
        title: &str,
        target: &InstallTarget,
    ) -> AppResult<String> {
        let uploads = self
            .itch
            .uploads(choice.game_id, choice.download_key_id)
            .await?;
        let find = |id: i64| {
            uploads.iter().find(|u| u.upload_id == id).ok_or_else(|| {
                AppError::Itch(tr!(
                    "Diese Datei gehört nicht zu dem Spiel.",
                    "This file doesn't belong to the game."
                ))
            })
        };
        let upload = find(choice.upload_id)?;
        let extras = choice
            .extras
            .iter()
            .filter(|id| **id != choice.upload_id)
            .map(|id| find(*id))
            .collect::<AppResult<Vec<_>>>()?;
        let target_dir = self.install_folder(target).await?;
        // With extras the game folder is fixed now, so they know where to go.
        let install_path = (!extras.is_empty()).then(|| {
            extract::unique_dir(Path::new(&target_dir), title)
                .to_string_lossy()
                .into_owned()
        });
        let transfer = |upload: &crate::models::ItchUpload| db::downloads::NewTransfer {
            title: title.to_owned(),
            origin: Some(DownloadOrigin::Itch),
            file_name: Some(upload.file_name.clone()),
            target_dir: target_dir.clone(),
            itch_game_id: Some(choice.game_id),
            itch_upload_id: Some(upload.upload_id),
            itch_download_key_id: choice.download_key_id,
            install_path: install_path.clone(),
            ..db::downloads::NewTransfer::default()
        };
        let id = db::downloads::enqueue_transfer(&self.pool, &transfer(upload)).await?;
        if !extras.is_empty() {
            let game_id = db::downloads::record(&self.pool, &id).await?.game_id;
            for extra in extras {
                db::downloads::enqueue_transfer(
                    &self.pool,
                    &db::downloads::NewTransfer {
                        extra_of: Some((game_id.clone(), extra.file_name.clone())),
                        ..transfer(extra)
                    },
                )
                .await?;
            }
        }
        (self.hooks.changed)();
        self.wake();
        Ok(id)
    }

    /// Store game: asks the store client to install it.
    pub async fn hand_off(&self, game_id: &str) -> AppResult<()> {
        let (_, store, store_game_id) = db::downloads::enqueue_handoff(&self.pool, game_id).await?;
        let url = handoff::install_url(store, &store_game_id, cfg!(windows)).ok_or_else(|| {
            AppError::InvalidInput(tr!(
                "Dieses Spiel stammt aus keinem Store.",
                "This game doesn't come from a store."
            ))
        })?;
        (self.hooks.open_url)(&url).map_err(|_| {
            AppError::Launch(tr!(
                "{} konnte nicht geöffnet werden. Ist der Client installiert?",
                "{} could not be opened. Is the client installed?",
                match store {
                    StoreSource::Steam => "Steam",
                    _ if cfg!(windows) => store.display_name(),
                    _ => "Heroic",
                }
            ))
        })?;
        (self.hooks.changed)();
        Ok(())
    }

    /// Pause, resume/retry (`queued`) or cancel.
    pub async fn control(&self, id: &str, status: DownloadStatus) -> AppResult<()> {
        let signal = |control| {
            if let Some(tx) = self
                .running
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .get(id)
            {
                let _ = tx.send(control);
                true
            } else {
                false
            }
        };
        match status {
            DownloadStatus::Paused => {
                signal(Control::Pause);
                self.set_status(id, DownloadStatus::Paused).await?;
            }
            DownloadStatus::Queued => {
                let record = db::downloads::record(&self.pool, id).await?;
                if record.target_dir.is_none() {
                    return Err(AppError::InvalidInput(tr!(
                        "Store-Installationen laufen im Store-Client.",
                        "Store installs run in the store client."
                    )));
                }
                self.set_status(id, DownloadStatus::Queued).await?;
                self.wake();
            }
            DownloadStatus::Cancelled => {
                // Cancelling a game also cancels its extras.
                if let Ok(record) = db::downloads::record(&self.pool, id).await {
                    if record.extra.is_none() {
                        for extra in
                            db::downloads::active_extras(&self.pool, &record.game_id).await?
                        {
                            Box::pin(self.control(&extra, DownloadStatus::Cancelled)).await?;
                        }
                    }
                }
                let was_running = signal(Control::Cancel);
                if !was_running {
                    if let Ok(record) = db::downloads::record(&self.pool, id).await {
                        Self::remove_files(&record);
                        self.remove_unfinished_game(&record).await;
                    }
                }
                self.set_status(id, DownloadStatus::Cancelled).await?;
            }
            _ => {
                return Err(AppError::InvalidInput(tr!(
                    "Dieser Status kann nicht gesetzt werden.",
                    "This status can't be set."
                )))
            }
        }
        Ok(())
    }

    /// Store installs: done once the store's manifest shows the game as installed.
    pub async fn poll_handoffs(&self) -> AppResult<()> {
        let Some(stores) = &self.stores else {
            return Ok(());
        };
        let pending = db::downloads::handed_off(&self.pool).await?;
        if pending.is_empty() {
            return Ok(());
        }
        let local = stores.detect_local(None).await?;
        for (id, origin, store_game_id, game_id) in pending {
            let Some(found) = local
                .iter()
                .find(|g| g.store.as_str() == origin && g.store_game_id == store_game_id)
            else {
                continue;
            };
            let Some(store) = StoreSource::parse(&origin) else {
                continue;
            };
            db::store_games::upsert(
                &self.pool,
                store,
                &db::store_games::StoreGameInput {
                    store_game_id: found.store_game_id.clone(),
                    title: found.title.clone(),
                    install: Some(db::store_games::LocalInstall {
                        install_path: found.install_path.clone(),
                        executable_path: found.executable_path.clone(),
                        size_bytes: found.size_bytes,
                    }),
                    ..db::store_games::StoreGameInput::default()
                },
            )
            .await?;
            db::downloads::complete_handoff(&self.pool, &id).await?;
            (self.hooks.installed)(game_id);
            (self.hooks.changed)();
        }
        Ok(())
    }
}

/// A blocking task that died (panic): log it, show `message`.
fn background_failed(message: String) -> impl FnOnce(tokio::task::JoinError) -> AppError {
    move |error| {
        eprintln!("[cove] Hintergrundaufgabe: {error}");
        AppError::Download(message)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::catalog::secrets::MemoryStore;
    use crate::db::open_in_memory;
    use crate::models::CompatibilityTool;
    use std::io::Write;
    use std::sync::atomic::AtomicUsize;
    use wiremock::matchers::{method, path};
    use wiremock::{Mock, MockServer};

    impl DownloadManager {
        /// Into the default games folder, like the install dialog without changes.
        async fn add_url_to(&self, input: &DirectDownloadInput) -> AppResult<String> {
            let target = InstallTarget {
                folder: self.defaults.downloads.directory.clone(),
                make_default: false,
                excluded_dlcs: Vec::new(),
            };
            self.add_url(input, &target).await
        }
    }

    struct Recorder {
        changed: AtomicUsize,
        progress: AtomicUsize,
        installed: Mutex<Vec<String>>,
        opened: Mutex<Vec<String>>,
    }

    async fn manager(dir: &Path) -> (Arc<DownloadManager>, Arc<Recorder>) {
        let itch = ItchApi::new(Arc::new(MemoryStore::default()), "http://127.0.0.1:1");
        manager_with(dir, itch).await
    }

    async fn manager_with(dir: &Path, itch: ItchApi) -> (Arc<DownloadManager>, Arc<Recorder>) {
        let pool = open_in_memory().await.unwrap();
        let mut defaults = crate::state::default_settings(None);
        defaults.downloads.directory = dir.to_string_lossy().into_owned();
        defaults.library.game_folders = vec![defaults.downloads.directory.clone()];
        defaults.downloads.max_concurrent = 2;
        let rec = Arc::new(Recorder {
            changed: AtomicUsize::new(0),
            progress: AtomicUsize::new(0),
            installed: Mutex::new(Vec::new()),
            opened: Mutex::new(Vec::new()),
        });
        let (a, b, c, d) = (rec.clone(), rec.clone(), rec.clone(), rec.clone());
        let hooks = Hooks {
            changed: Arc::new(move || {
                a.changed.fetch_add(1, Ordering::Relaxed);
            }),
            progress: Arc::new(move |_| {
                b.progress.fetch_add(1, Ordering::Relaxed);
            }),
            installed: Arc::new(move |id| c.installed.lock().unwrap().push(id)),
            open_url: Arc::new(move |url| {
                d.opened.lock().unwrap().push(url.to_owned());
                Ok(())
            }),
        };
        let manager = Arc::new(DownloadManager::new(pool, defaults, itch, None, hooks));
        (manager, rec)
    }

    #[tokio::test(flavor = "multi_thread")]
    #[allow(clippy::too_many_lines)] // cancel and install in one walk
    async fn itch_extras_go_into_the_game_folder() {
        use crate::catalog::secrets::SecretStore;
        let server = MockServer::start().await;
        let mut ost = std::io::Cursor::new(Vec::new());
        {
            let mut zip = zip::ZipWriter::new(&mut ost);
            zip.start_file("track.ogg", zip::write::SimpleFileOptions::default())
                .unwrap();
            zip.write_all(b"music").unwrap();
            zip.finish().unwrap();
        }
        let files = [
            (100, "zeta-linux.zip", zip_with_game()),
            (101, "ost.zip", ost.into_inner()),
            (102, "artbook.pdf", b"%PDF-1.4 art".to_vec()),
        ];
        for (upload, name, data) in &files {
            Mock::given(method("GET"))
                .and(path(format!("/uploads/{upload}/download")))
                .respond_with(
                    wiremock::ResponseTemplate::new(302)
                        .insert_header("Location", format!("{}/files/{name}", server.uri())),
                )
                .mount(&server)
                .await;
            Mock::given(method("GET"))
                .and(path(format!("/files/{name}")))
                .respond_with(transfer::tests::RangeServer {
                    data: data.clone(),
                    etag: "\"1\"".into(),
                })
                .mount(&server)
                .await;
        }
        Mock::given(method("GET"))
            .and(path("/games/1/uploads"))
            .respond_with(wiremock::ResponseTemplate::new(200).set_body_json(
                serde_json::json!({ "uploads": [
                { "id": 100, "filename": "zeta-linux.zip", "p_linux": true, "type": "default" },
                { "id": 101, "filename": "ost.zip", "type": "soundtrack" },
                { "id": 102, "filename": "artbook.pdf", "type": "book" } ] }),
            ))
            .mount(&server)
            .await;
        let secrets = Arc::new(MemoryStore::default());
        secrets.set("itch-api-key", "key").unwrap();
        let dir = tempfile::tempdir().unwrap();
        let (manager, rec) = manager_with(dir.path(), ItchApi::new(secrets, &server.uri())).await;
        let target = InstallTarget {
            folder: dir.path().to_string_lossy().into_owned(),
            make_default: false,
            excluded_dlcs: Vec::new(),
        };
        let choice = db::downloads::ItchChoice {
            game_id: 1,
            upload_id: 100,
            download_key_id: None,
            extras: vec![101, 102],
        };

        // Cancelling the game cancels its extras too.
        let id = manager.add_itch(&choice, "Zeta", &target).await.unwrap();
        manager
            .control(&id, DownloadStatus::Cancelled)
            .await
            .unwrap();
        let all = db::downloads::list(&manager.pool).await.unwrap();
        assert_eq!(all.len(), 3);
        assert!(all
            .iter()
            .all(|d| d.download.status == DownloadStatus::Cancelled));

        manager.start();
        let id = manager.add_itch(&choice, "Zeta", &target).await.unwrap();
        wait_for(&manager, &id, DownloadStatus::Completed).await;
        // Wait until every extra of the second try is finished (or failed).
        for _ in 0..200 {
            let all = db::downloads::list(&manager.pool).await.unwrap();
            if all
                .iter()
                .filter(|d| d.download.extra.is_some())
                .filter(|d| d.download.status != DownloadStatus::Cancelled)
                .all(|d| {
                    matches!(
                        d.download.status,
                        DownloadStatus::Completed | DownloadStatus::Failed
                    )
                })
            {
                break;
            }
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
        let game = dir.path().join("Zeta");
        assert!(game.join("Lantern/Lantern.x86_64").exists());
        assert_eq!(
            std::fs::read(game.join("Extras/ost/track.ogg")).unwrap(),
            b"music"
        );
        assert_eq!(
            std::fs::read(game.join("Extras/artbook.pdf")).unwrap(),
            b"%PDF-1.4 art"
        );
        let extras: Vec<(String, DownloadStatus)> = db::downloads::list(&manager.pool)
            .await
            .unwrap()
            .into_iter()
            .filter(|d| d.download.status != DownloadStatus::Cancelled)
            .filter_map(|d| Some((d.download.extra?, d.download.status)))
            .collect();
        assert_eq!(extras.len(), 2, "{extras:?}");
        assert!(extras.iter().all(|(_, s)| *s == DownloadStatus::Completed));
        // Only the game itself is in the library.
        assert_eq!(rec.installed.lock().unwrap().as_slice(), ["itch-1"]);
    }

    fn zip_with_game() -> Vec<u8> {
        let mut buffer = std::io::Cursor::new(Vec::new());
        {
            let mut zip = zip::ZipWriter::new(&mut buffer);
            let options = zip::write::SimpleFileOptions::default();
            zip.start_file("Lantern/Lantern.x86_64", options).unwrap();
            let mut elf = b"\x7fELF".to_vec();
            elf.resize(20_000, 7);
            zip.write_all(&elf).unwrap();
            zip.finish().unwrap();
        }
        buffer.into_inner()
    }

    async fn wait_for(
        manager: &DownloadManager,
        id: &str,
        status: DownloadStatus,
    ) -> crate::models::DownloadItem {
        for _ in 0..200 {
            let item = db::downloads::get(&manager.pool, id).await.unwrap();
            if item.download.status == status {
                return item;
            }
            tokio::time::sleep(Duration::from_millis(25)).await;
        }
        panic!(
            "{id} never reached {status:?}: {:?}",
            db::downloads::get(&manager.pool, id)
                .await
                .unwrap()
                .download
        );
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn downloads_verifies_unpacks_and_adds_to_the_library() {
        let server = MockServer::start().await;
        let body = zip_with_game();
        let sha = transfer::sha256_file(&{
            let f = tempfile::NamedTempFile::new().unwrap();
            std::fs::write(f.path(), &body).unwrap();
            f.into_temp_path().keep().unwrap()
        })
        .unwrap();
        Mock::given(method("GET"))
            .and(path("/files/lantern-1.0.zip"))
            .respond_with(transfer::tests::RangeServer {
                data: body,
                etag: "\"1\"".into(),
            })
            .mount(&server)
            .await;
        let dir = tempfile::tempdir().unwrap();
        let (manager, rec) = manager(dir.path()).await;
        manager.start();
        let id = manager
            .add_url_to(&DirectDownloadInput {
                url: format!("{}/files/lantern-1.0.zip", server.uri()),
                title: None,
                sha256: Some(sha.to_uppercase()),
            })
            .await
            .unwrap();
        let item = wait_for(&manager, &id, DownloadStatus::Completed).await;
        assert_eq!(item.game.title, title_from_file_name("lantern-1.0.zip"));
        assert!(item.download.checksum_verified);
        let library = db::library::require(&manager.pool, &item.game.id)
            .await
            .unwrap();
        assert!(library.entry.installed);
        assert!(library.entry.installed_by_cove);
        db::library::mark_uninstalled(&manager.pool, &item.game.id)
            .await
            .unwrap();
        let gone = db::library::require(&manager.pool, &item.game.id)
            .await
            .unwrap();
        assert!(!gone.entry.installed && !gone.entry.installed_by_cove);
        assert_eq!(gone.entry.launch_via, None);
        if cfg!(target_os = "linux") {
            assert_eq!(
                library.entry.launch_via,
                Some(crate::models::LaunchVia::Direct)
            );
            assert!(library
                .entry
                .executable_path
                .unwrap()
                .ends_with("Lantern/Lantern.x86_64"));
        }
        assert!(
            !dir.path().join(WORK_DIR).exists(),
            "temporary files removed"
        );
        assert_eq!(
            *rec.installed.lock().unwrap(),
            std::slice::from_ref(&item.game.id)
        );
        assert!(rec.changed.load(Ordering::Relaxed) > 0);
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn wrong_checksums_fail_and_bad_input_is_rejected() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/x.zip"))
            .respond_with(transfer::tests::RangeServer {
                data: zip_with_game(),
                etag: "e".into(),
            })
            .mount(&server)
            .await;
        let dir = tempfile::tempdir().unwrap();
        let (manager, _) = manager(dir.path()).await;
        manager.start();
        let id = manager
            .add_url_to(&DirectDownloadInput {
                url: format!("{}/x.zip", server.uri()),
                title: Some("X".into()),
                sha256: Some("0".repeat(64)),
            })
            .await
            .unwrap();
        let item = wait_for(&manager, &id, DownloadStatus::Failed).await;
        assert!(item
            .download
            .error
            .unwrap()
            .contains("Prüfsumme stimmt nicht"));
        assert!(db::library::get(&manager.pool, &item.game.id)
            .await
            .unwrap()
            .is_none());

        let bad = |url: &str, sha: Option<&str>| DirectDownloadInput {
            url: url.into(),
            title: None,
            sha256: sha.map(str::to_owned),
        };
        assert!(matches!(
            manager.add_url_to(&bad("ftp://x/y", None)).await,
            Err(AppError::InvalidInput(_))
        ));
        assert!(matches!(
            manager.add_url_to(&bad("nonsense", None)).await,
            Err(AppError::InvalidInput(_))
        ));
        assert!(matches!(
            manager
                .add_url_to(&bad("https://x.test/a.zip", Some("abc")))
                .await,
            Err(AppError::InvalidInput(_))
        ));
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn pause_resume_and_cancel() {
        let server = MockServer::start().await;
        let data = transfer::tests::data(3_000_000);
        Mock::given(method("GET"))
            .and(path("/big.bin"))
            .respond_with(
                transfer::tests::RangeServer {
                    data,
                    etag: "e".into(),
                }
                .delay(),
            )
            .mount(&server)
            .await;
        let dir = tempfile::tempdir().unwrap();
        let (manager, _) = manager(dir.path()).await;
        let id = manager
            .add_url_to(&DirectDownloadInput {
                url: format!("{}/big.bin", server.uri()),
                title: Some("Big".into()),
                sha256: None,
            })
            .await
            .unwrap();
        manager.start();
        wait_for(&manager, &id, DownloadStatus::Downloading).await;
        manager.control(&id, DownloadStatus::Paused).await.unwrap();
        wait_for(&manager, &id, DownloadStatus::Paused).await;
        for _ in 0..80 {
            if manager.running_ids().is_empty() {
                break;
            }
            tokio::time::sleep(Duration::from_millis(25)).await;
        }
        assert!(manager.running_ids().is_empty(), "task stopped within 2 s");

        manager.control(&id, DownloadStatus::Queued).await.unwrap();
        wait_for(&manager, &id, DownloadStatus::Downloading).await;
        manager
            .control(&id, DownloadStatus::Cancelled)
            .await
            .unwrap();
        wait_for(&manager, &id, DownloadStatus::Cancelled).await;
        tokio::time::sleep(Duration::from_millis(300)).await;
        assert!(!dir
            .path()
            .join(WORK_DIR)
            .join(format!("{id}.part"))
            .exists());
        assert!(matches!(
            manager.control(&id, DownloadStatus::Completed).await,
            Err(AppError::InvalidInput(_))
        ));
    }

    #[tokio::test]
    async fn store_games_are_handed_to_their_client() {
        let dir = tempfile::tempdir().unwrap();
        let (manager, rec) = manager(dir.path()).await;
        db::store_games::upsert(
            &manager.pool,
            StoreSource::Steam,
            &db::store_games::StoreGameInput {
                store_game_id: "620".into(),
                title: "Portal 2".into(),
                ..Default::default()
            },
        )
        .await
        .unwrap();
        manager.hand_off("steam-620").await.unwrap();
        assert_eq!(*rec.opened.lock().unwrap(), ["steam://install/620"]);
        let items = db::downloads::list(&manager.pool).await.unwrap();
        assert_eq!(items[0].download.status, DownloadStatus::HandedOff);
        assert!(matches!(
            manager
                .control(&items[0].download.id, DownloadStatus::Queued)
                .await,
            Err(AppError::InvalidInput(_))
        ));
    }

    #[tokio::test]
    async fn install_dialog_sizes_and_target_folders() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/lantern-1.0.zip"))
            .respond_with(transfer::tests::RangeServer {
                data: zip_with_game(),
                etag: "e".into(),
            })
            .mount(&server)
            .await;
        // A single file without Content-Disposition: installed as it is.
        Mock::given(method("GET"))
            .and(path("/Lantern.AppImage"))
            .respond_with(wiremock::ResponseTemplate::new(200).set_body_bytes(vec![1u8; 4096]))
            .mount(&server)
            .await;
        let dir = tempfile::tempdir().unwrap();
        let (manager, _) = manager(dir.path()).await;
        let input = |file: &str| DirectDownloadInput {
            url: format!("{}{file}", server.uri()),
            title: None,
            sha256: None,
        };
        let zip = manager
            .preview_url(&input("/lantern-1.0.zip"))
            .await
            .unwrap();
        assert_eq!(zip.title, title_from_file_name("lantern-1.0.zip"));
        assert_eq!(
            zip.download_bytes,
            i64::try_from(zip_with_game().len()).ok()
        );
        assert_eq!(zip.install_bytes, None, "archives: known after unpacking");
        let single = manager
            .preview_url(&input("/Lantern.AppImage"))
            .await
            .unwrap();
        assert_eq!(single.install_bytes, Some(4096));

        // A new folder chosen as default.
        let other = dir.path().join("Other Drive/Games");
        let target = InstallTarget {
            folder: other.to_string_lossy().into_owned(),
            make_default: true,
            excluded_dlcs: Vec::new(),
        };
        let id = manager
            .add_url(&input("/lantern-1.0.zip"), &target)
            .await
            .unwrap();
        let record = db::downloads::record(&manager.pool, &id).await.unwrap();
        assert_eq!(
            record.target_dir.as_deref(),
            Some(&*other.to_string_lossy())
        );
        let settings = manager.settings().await.unwrap();
        assert_eq!(settings.downloads.directory, other.to_string_lossy());
        assert_eq!(
            settings.library.game_folders,
            [
                other.to_string_lossy().into_owned(),
                dir.path().to_string_lossy().into_owned()
            ]
        );
        let relative = InstallTarget {
            folder: "Games".into(),
            make_default: false,
            excluded_dlcs: Vec::new(),
        };
        assert!(matches!(
            manager.add_url(&input("/lantern-1.0.zip"), &relative).await,
            Err(AppError::InvalidInput(_))
        ));
    }

    /// A GOG game served by a fake CDN; the connector answers with `plan`.
    struct GogWorld {
        manager: Arc<DownloadManager>,
        plan: Arc<Mutex<serde_json::Value>>,
        server: MockServer,
        games: tempfile::TempDir,
    }

    async fn serve_chunk(server: &MockServer, data: &[u8]) -> serde_json::Value {
        let (chunk, compressed) = gog::tests::chunk_of(data);
        let galaxy = format!(
            "/cdn/{}/{}/{}",
            &chunk.compressed_md5[..2],
            &chunk.compressed_md5[2..4],
            chunk.compressed_md5
        );
        Mock::given(method("GET"))
            .and(path(galaxy))
            .respond_with(wiremock::ResponseTemplate::new(200).set_body_bytes(compressed))
            .mount(server)
            .await;
        serde_json::to_value(chunk).unwrap()
    }

    async fn gog_world() -> GogWorld {
        use crate::catalog::secrets::SecretStore;
        let server = MockServer::start().await;
        let pool = open_in_memory().await.unwrap();
        let games = tempfile::tempdir().unwrap();
        let mut defaults = crate::state::default_settings(None);
        defaults.downloads.directory = games.path().to_string_lossy().into_owned();
        defaults.library.game_folders = vec![defaults.downloads.directory.clone()];
        let plan = Arc::new(Mutex::new(serde_json::Value::Null));
        let answer = plan.clone();
        let rpc = crate::stores::tests::FakeRpc::new(move |method, _| {
            let plan = answer.lock().unwrap().clone();
            Ok(match method {
                "gog.plan" => plan,
                "gog.latest" => {
                    serde_json::json!({ "buildId": plan["buildId"], "version": plan["version"], "platform": plan["platform"] })
                }
                _ => serde_json::Value::Null,
            })
        });
        let secrets = Arc::new(MemoryStore::default());
        secrets
            .set(
                "store-session:gog",
                r#"{ "store": "gog", "accountId": "1", "displayName": "Kiefer", "accessToken": "a1",
                     "refreshToken": "r1", "expiresAt": "2099-01-01T00:00:00Z" }"#,
            )
            .unwrap();
        let stores = Arc::new(StoreService::new(
            rpc,
            pool.clone(),
            secrets,
            None,
            Duration::ZERO,
        ));
        db::store_games::upsert(
            &pool,
            StoreSource::Gog,
            &db::store_games::StoreGameInput {
                store_game_id: "100".into(),
                title: "Saltmarsh: Tides".into(),
                ..Default::default()
            },
        )
        .await
        .unwrap();
        let hooks = Hooks {
            changed: Arc::new(|| {}),
            progress: Arc::new(|_| {}),
            installed: Arc::new(|_| {}),
            open_url: Arc::new(|_| Ok(())),
        };
        let itch = ItchApi::new(Arc::new(MemoryStore::default()), "http://127.0.0.1:1");
        let manager = Arc::new(DownloadManager::new(
            pool,
            defaults,
            itch,
            Some(stores),
            hooks,
        ));
        manager.start();
        GogWorld {
            manager,
            plan,
            server,
            games,
        }
    }

    fn gog_plan(
        world: &GogWorld,
        build: &str,
        files: serde_json::Value,
        containers: serde_json::Value,
    ) -> serde_json::Value {
        serde_json::json!({
            "productId": "100", "platform": "windows", "buildId": build, "version": format!("1.{build}"),
            "downloadBytes": 0, "installBytes": 0, "directories": ["saves"],
            "files": files, "links": [{ "path": "start", "target": "bin/game.exe" }],
            "containers": containers,
            // A broken CDN first (404): Cove falls back to the next one. (Not a
            // closed port: Windows takes about 2 s to refuse each connection.)
            "chunkUrls": { "100": [format!("{}/broken/{{chunk}}", world.server.uri()), format!("{}/cdn/{{chunk}}", world.server.uri())] },
            "installer": null
        })
    }

    #[tokio::test(flavor = "multi_thread")]
    #[allow(clippy::too_many_lines)] // one walk through install and update
    async fn installs_and_updates_gog_games_chunk_by_chunk() {
        let world = gog_world().await;
        let part_a = transfer::tests::data(300_000);
        let a = serve_chunk(&world.server, &part_a).await;
        let b = serve_chunk(&world.server, b"tail of the old build").await;
        let info = br#"{ "playTasks": [ { "isPrimary": true, "type": "FileTask", "path": "bin\\game.exe", "arguments": "-lang de" } ] }"#;
        let i = serve_chunk(&world.server, info).await;
        let container = serve_chunk(&world.server, b"..tiny!..").await;
        *world.plan.lock().unwrap() = gog_plan(
            &world,
            "8",
            serde_json::json!([
                { "path": "bin/game.exe", "productId": "100", "chunks": [a, b], "executable": true, "sfc": null },
                { "path": "goggame-100.info", "productId": "100", "chunks": [i], "executable": false, "sfc": null },
                { "path": "tiny.txt", "productId": "100", "chunks": [], "executable": false,
                  "sfc": { "container": 0, "offset": 2, "size": 5 } }
            ]),
            serde_json::json!([{ "productId": "100", "chunks": [container] }]),
        );

        let preview = world.manager.preview_store("gog-100").await.unwrap();
        assert_eq!(preview.title, "Saltmarsh: Tides");
        let target = InstallTarget {
            folder: world.games.path().to_string_lossy().into_owned(),
            make_default: false,
            excluded_dlcs: Vec::new(),
        };
        let id = world
            .manager
            .install_store("gog-100", &target)
            .await
            .unwrap();
        wait_for(&world.manager, &id, DownloadStatus::Completed).await;

        let dir = world.games.path().join("Saltmarsh Tides");
        let mut expected = part_a.clone();
        expected.extend_from_slice(b"tail of the old build");
        assert_eq!(std::fs::read(dir.join("bin/game.exe")).unwrap(), expected);
        assert_eq!(std::fs::read(dir.join("tiny.txt")).unwrap(), b"tiny!");
        assert!(dir.join("saves").is_dir());
        #[cfg(unix)]
        assert_eq!(
            std::fs::read_link(dir.join("start")).unwrap(),
            Path::new("bin/game.exe")
        );
        let item = db::library::require(&world.manager.pool, "gog-100")
            .await
            .unwrap();
        assert!(item.entry.installed && item.entry.installed_by_cove);
        assert_eq!(
            item.entry.install_path.as_deref(),
            Some(&*dir.to_string_lossy())
        );
        assert_eq!(
            item.entry.executable_path.as_deref(),
            Some(&*dir.join("bin").join("game.exe").to_string_lossy())
        );
        assert_eq!(
            item.entry.launch_via,
            Some(crate::models::LaunchVia::Direct)
        );
        if !cfg!(windows) {
            assert_eq!(
                item.entry.launch_options.compatibility_tool,
                CompatibilityTool::Proton
            );
        }
        assert!(
            !world.games.path().join(WORK_DIR).exists(),
            "temporary files removed"
        );
        let installed = db::store_installs::get(&world.manager.pool, "gog-100")
            .await
            .unwrap()
            .unwrap();
        assert_eq!(installed.build_id, "8");
        assert!(
            !world
                .manager
                .check_update("gog-100")
                .await
                .unwrap()
                .available
        );

        // Build 9 changes the end of game.exe, drops tiny.txt and adds new.txt.
        let b2 = serve_chunk(&world.server, b"tail of the new build!").await;
        let n = serve_chunk(&world.server, b"new").await;
        *world.plan.lock().unwrap() = gog_plan(
            &world,
            "9",
            serde_json::json!([
                { "path": "bin/game.exe", "productId": "100", "chunks": [a, b2], "executable": true, "sfc": null },
                { "path": "goggame-100.info", "productId": "100", "chunks": [i], "executable": false, "sfc": null },
                { "path": "new.txt", "productId": "100", "chunks": [n], "executable": false, "sfc": null }
            ]),
            serde_json::json!([]),
        );
        let update = world.manager.check_update("gog-100").await.unwrap();
        assert!(update.available);
        assert_eq!(
            (
                update.installed_version.as_str(),
                update.latest_version.as_str()
            ),
            ("1.8", "1.9")
        );
        let id = world.manager.update_store("gog-100").await.unwrap();
        wait_for(&world.manager, &id, DownloadStatus::Completed).await;
        let mut expected = part_a;
        expected.extend_from_slice(b"tail of the new build!");
        assert_eq!(std::fs::read(dir.join("bin/game.exe")).unwrap(), expected);
        assert_eq!(std::fs::read(dir.join("new.txt")).unwrap(), b"new");
        assert!(
            !dir.join("tiny.txt").exists(),
            "files the new build dropped are removed"
        );
        // The unchanged first chunk was not downloaded again.
        let requests = world.server.received_requests().await.unwrap();
        let a_path = a["compressedMd5"].as_str().unwrap().to_owned();
        assert_eq!(
            requests
                .iter()
                .filter(|r| r.url.path().starts_with("/cdn/") && r.url.path().ends_with(&a_path))
                .count(),
            1
        );
        assert_eq!(
            db::store_installs::get(&world.manager.pool, "gog-100")
                .await
                .unwrap()
                .unwrap()
                .build_id,
            "9"
        );
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn damaged_gog_chunks_fail_and_cancelled_installs_leave_nothing() {
        let world = gog_world().await;
        let good = transfer::tests::data(1000);
        let mut chunk = serve_chunk(&world.server, &good).await;
        chunk["md5"] = serde_json::json!("0".repeat(32));
        *world.plan.lock().unwrap() = gog_plan(
            &world,
            "8",
            serde_json::json!([{ "path": "game.exe", "productId": "100", "chunks": [chunk], "executable": false, "sfc": null }]),
            serde_json::json!([]),
        );
        let target = InstallTarget {
            folder: world.games.path().to_string_lossy().into_owned(),
            make_default: false,
            excluded_dlcs: Vec::new(),
        };
        let id = world
            .manager
            .install_store("gog-100", &target)
            .await
            .unwrap();
        let failed = wait_for(&world.manager, &id, DownloadStatus::Failed).await;
        assert!(failed.download.error.unwrap().contains("beschädigt"));
        let failed_dir = world.games.path().join("Saltmarsh Tides");
        assert!(failed_dir.is_dir(), "kept for a retry");
        // A slow chunk: cancelling the first install removes the game folder again.
        let slow = transfer::tests::data(2000);
        let (slow_chunk, compressed) = gog::tests::chunk_of(&slow);
        let galaxy = format!(
            "/cdn/{}/{}/{}",
            &slow_chunk.compressed_md5[..2],
            &slow_chunk.compressed_md5[2..4],
            slow_chunk.compressed_md5
        );
        Mock::given(method("GET"))
            .and(path(galaxy))
            .respond_with(
                wiremock::ResponseTemplate::new(200)
                    .set_body_bytes(compressed)
                    .set_delay(Duration::from_secs(10)),
            )
            .mount(&world.server)
            .await;
        *world.plan.lock().unwrap() = gog_plan(
            &world,
            "8",
            serde_json::json!([{ "path": "game.exe", "productId": "100",
                "chunks": [serde_json::to_value(&slow_chunk).unwrap()], "executable": false, "sfc": null }]),
            serde_json::json!([]),
        );
        let id = world
            .manager
            .install_store("gog-100", &target)
            .await
            .unwrap();
        wait_for(&world.manager, &id, DownloadStatus::Downloading).await;
        let dir = PathBuf::from(
            db::downloads::record(&world.manager.pool, &id)
                .await
                .unwrap()
                .install_path
                .unwrap(),
        );
        assert_eq!(dir, world.games.path().join("Saltmarsh Tides 2"));
        for _ in 0..200 {
            if dir.join("game.exe").exists() {
                break;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
        world
            .manager
            .control(&id, DownloadStatus::Cancelled)
            .await
            .unwrap();
        for _ in 0..200 {
            if !dir.exists() && world.manager.running_ids().is_empty() {
                break;
            }
            tokio::time::sleep(Duration::from_millis(25)).await;
        }
        assert!(!dir.exists(), "cancelled first install leaves no folder");
        world
            .manager
            .control(&failed.download.id, DownloadStatus::Cancelled)
            .await
            .unwrap();
        assert!(
            !failed_dir.exists(),
            "cancelling the failed install cleans up too"
        );
        assert!(db::store_installs::get(&world.manager.pool, "gog-100")
            .await
            .unwrap()
            .is_none());
        // Manual games can't be installed from GOG.
        assert!(matches!(
            world.manager.preview_store("nope").await,
            Err(AppError::NotInLibrary(_))
        ));
    }

    #[tokio::test(flavor = "multi_thread")]
    #[allow(clippy::too_many_lines)] // install, DLCs, start, saves, anti-cheat in one walk
    async fn installs_epic_games_starts_them_with_a_login_and_uploads_saves() {
        use crate::catalog::secrets::SecretStore;
        use epic::format;
        let server = MockServer::start().await;
        let (mut manifest, chunks, files) = format::tests::sample();
        manifest.version = 18;
        manifest.files.push(format::FileEntry {
            name: "Saves/slot1.sav".into(),
            ..manifest.files[1].clone()
        });
        let readme = files[1].1.clone();
        // A DLC with a build of its own: one file in the game's folder.
        let mut island = manifest.clone();
        island.files = vec![format::FileEntry {
            name: "Island/island.pak".into(),
            ..manifest.files[1].clone()
        }];
        for (build, manifest) in [("Sugar", &manifest), ("Island", &island)] {
            Mock::given(method("GET"))
                .and(path(format!("/Builds/{build}/abc.manifest")))
                .respond_with(
                    wiremock::ResponseTemplate::new(200).set_body_bytes(manifest.to_bytes()),
                )
                .mount(&server)
                .await;
            for (guid, bytes) in &chunks {
                let chunk_path = format!(
                    "/Builds/{build}/{}",
                    manifest.chunk_path(&manifest.chunks[guid])
                );
                Mock::given(method("GET"))
                    .and(path(chunk_path))
                    .respond_with(
                        wiremock::ResponseTemplate::new(200).set_body_bytes(bytes.clone()),
                    )
                    .mount(&server)
                    .await;
            }
        }
        Mock::given(method("PUT"))
            .respond_with(wiremock::ResponseTemplate::new(200))
            .mount(&server)
            .await;

        let pool = open_in_memory().await.unwrap();
        let games = tempfile::tempdir().unwrap();
        let mut defaults = crate::state::default_settings(None);
        defaults.downloads.directory = games.path().to_string_lossy().into_owned();
        defaults.library.game_folders = vec![defaults.downloads.directory.clone()];
        let uri = server.uri();
        let anti_cheat = Arc::new(Mutex::new(false));
        let cheat = anti_cheat.clone();
        let dlc_owned = Arc::new(Mutex::new(true));
        let owned = dlc_owned.clone();
        let rpc = crate::stores::tests::FakeRpc::new(move |method, params| {
            Ok(match method {
                "epic.installInfo" => serde_json::json!({
                    "appName": "Sugar", "namespace": "ns1", "catalogItemId": "cat1", "buildVersion": "1.0-Windows",
                    "manifestUrls": [format!("{uri}/Builds/Sugar/abc.manifest?f_token=x")],
                    "launcherOnly": if *cheat.lock().unwrap() { serde_json::json!("needs the EA app") } else { serde_json::Value::Null },
                    "cloudSaveFolder": "{InstallDir}/Saves", "ownershipToken": false,
                    "dlcs": if *owned.lock().unwrap() { serde_json::json!([{
                        "appName": "SugarIsland", "catalogItemId": "dlc1", "title": "Sugar Island",
                        "buildVersion": "7-Windows", "manifestUrls": [format!("{uri}/Builds/Island/abc.manifest")]
                    }]) } else { serde_json::json!([]) }
                }),
                "epic.launch" => {
                    serde_json::json!({ "exchangeCode": "ex-42", "accountId": "acc", "displayName": "Tiny", "ownershipToken": null })
                }
                "epic.saves" => serde_json::json!({ "files": [] }),
                "epic.saveWriteLinks" => {
                    let links: serde_json::Map<String, serde_json::Value> = params["paths"]
                        .as_array()
                        .unwrap()
                        .iter()
                        .map(|p| {
                            (
                                p.as_str().unwrap().to_owned(),
                                serde_json::json!(format!("{uri}/put/{}", p.as_str().unwrap())),
                            )
                        })
                        .collect();
                    serde_json::json!({ "links": links })
                }
                _ => serde_json::Value::Null,
            })
        });
        let secrets = Arc::new(MemoryStore::default());
        secrets
            .set(
                "store-session:epic",
                r#"{ "store": "epic", "accountId": "acc", "displayName": "Tiny", "accessToken": "a1",
                     "refreshToken": "r1", "expiresAt": "2099-01-01T00:00:00Z" }"#,
            )
            .unwrap();
        let stores = Arc::new(StoreService::new(
            rpc.clone(),
            pool.clone(),
            secrets,
            None,
            Duration::ZERO,
        ));
        db::store_games::upsert(
            &pool,
            StoreSource::Epic,
            &db::store_games::StoreGameInput {
                store_game_id: "Sugar".into(),
                title: "Sugar".into(),
                ..Default::default()
            },
        )
        .await
        .unwrap();
        let hooks = Hooks {
            changed: Arc::new(|| {}),
            progress: Arc::new(|_| {}),
            installed: Arc::new(|_| {}),
            open_url: Arc::new(|_| Ok(())),
        };
        let itch = ItchApi::new(Arc::new(MemoryStore::default()), "http://127.0.0.1:1");
        let manager = Arc::new(DownloadManager::new(
            pool.clone(),
            defaults,
            itch,
            Some(stores),
            hooks,
        ));
        manager.start();

        // Install.
        let preview = manager.preview_store("epic-sugar").await.unwrap();
        assert_eq!(preview.launcher_only, None);
        // The game's size alone; the DLC brings its own (ticked by default).
        assert_eq!(
            preview.install_bytes,
            i64::try_from(manifest.install_size()).ok()
        );
        assert_eq!(
            preview.dlcs,
            [crate::models::DlcOption {
                id: "SugarIsland".into(),
                title: "Sugar Island".into(),
                download_bytes: i64::try_from(island.download_size(island.chunks.keys())).ok(),
                install_bytes: i64::try_from(island.install_size()).ok(),
                selected: true,
            }]
        );
        let target = InstallTarget {
            folder: games.path().to_string_lossy().into_owned(),
            make_default: false,
            excluded_dlcs: Vec::new(),
        };
        let id = manager.install_store("epic-sugar", &target).await.unwrap();
        wait_for(&manager, &id, DownloadStatus::Completed).await;
        let dir = games.path().join("Sugar");
        assert_eq!(
            std::fs::read(dir.join("Binaries/Win64/Sugar.exe")).unwrap(),
            files[0].1
        );
        assert_eq!(std::fs::read(dir.join("readme.txt")).unwrap(), readme);
        let item = db::library::require(&pool, "epic-sugar").await.unwrap();
        assert!(item.entry.installed_by_cove);
        assert_eq!(
            item.entry.launch_via,
            Some(crate::models::LaunchVia::Direct)
        );
        assert!(item.entry.executable_path.unwrap().ends_with("Sugar.exe"));
        assert_eq!(
            db::store_installs::get(&pool, "epic-sugar")
                .await
                .unwrap()
                .unwrap()
                .build_id,
            "1.0-Windows"
        );
        // The DLC's file is in the game folder, but remembered on its own.
        assert_eq!(
            std::fs::read(dir.join("Island/island.pak")).unwrap(),
            readme
        );
        let dlcs = db::store_installs::dlcs(&pool, "epic-sugar").await.unwrap();
        assert_eq!(
            dlcs.iter()
                .map(|d| (d.app_name.as_str(), d.build_id.as_str(), d.files.clone()))
                .collect::<Vec<_>>(),
            [(
                "SugarIsland",
                "7-Windows",
                vec!["Island/island.pak".to_owned()]
            )]
        );
        assert!(!manager.check_update("epic-sugar").await.unwrap().available);

        // A DLC no longer owned: the update removes its files, the game stays.
        *dlc_owned.lock().unwrap() = false;
        let update = manager.check_update("epic-sugar").await.unwrap();
        assert!(update.available);
        assert!(update.latest_version.contains("DLC"));
        let id = manager.update_store("epic-sugar").await.unwrap();
        wait_for(&manager, &id, DownloadStatus::Completed).await;
        assert!(!dir.join("Island/island.pak").exists());
        assert_eq!(std::fs::read(dir.join("readme.txt")).unwrap(), readme);
        assert_eq!(
            db::store_installs::dlcs(&pool, "epic-sugar").await.unwrap(),
            Vec::new()
        );
        assert!(!manager.check_update("epic-sugar").await.unwrap().available);

        // Owned again: "DLCs ändern" ticks it, the update brings its file back.
        *dlc_owned.lock().unwrap() = true;
        assert!(manager.check_update("epic-sugar").await.unwrap().available);
        let id = manager.change_dlcs("epic-sugar", &[]).await.unwrap();
        wait_for(&manager, &id, DownloadStatus::Completed).await;
        assert_eq!(
            std::fs::read(dir.join("Island/island.pak")).unwrap(),
            readme
        );
        // Unticked: removed, no update offered, unticked next time.
        let id = manager
            .change_dlcs("epic-sugar", &["SugarIsland".into()])
            .await
            .unwrap();
        wait_for(&manager, &id, DownloadStatus::Completed).await;
        assert!(!dir.join("Island/island.pak").exists());
        assert!(!manager.check_update("epic-sugar").await.unwrap().available);
        let preview = manager.preview_store("epic-sugar").await.unwrap();
        assert!(!preview.dlcs[0].selected);
        // Unknown games are refused before anything is stored.
        assert!(matches!(
            manager.change_dlcs("nope", &["x".into()]).await,
            Err(AppError::NotInLibrary(_))
        ));

        // Start: a fresh exchange code and the manifest's launch command.
        let host = crate::launcher::HostEnv {
            platform: crate::models::Platform::current(),
            home: None,
            compat_data_root: games.path().join("compat"),
            protons: Vec::new(),
            default_proton: crate::proton::LATEST.to_owned(),
            umu: None,
            tools: crate::launcher::ToolPaths::default(),
            rom_folders: Vec::new(),
            emulators: crate::emulators::Env::detect(None),
        };
        let item = db::library::require(&pool, "epic-sugar").await.unwrap();
        let args = manager.epic_launch_args(&item, &host).await.unwrap();
        assert_eq!(args[0], "-nosplash");
        assert!(args.contains(&"-AUTH_PASSWORD=ex-42".to_owned()));
        assert!(args.contains(&"-epicsandboxid=ns1".to_owned()));

        // After playing: the newer local saves go up, manifest last.
        manager.after_session("epic-sugar", &host).await.unwrap();
        let puts: Vec<String> = server
            .received_requests()
            .await
            .unwrap()
            .into_iter()
            .filter(|r| r.method.as_str() == "PUT")
            .map(|r| r.url.path().to_owned())
            .collect();
        assert_eq!(puts.len(), 2, "{puts:?}");
        assert!(puts[0].starts_with("/put/acc/Sugar/ChunksV4/"));
        assert!(puts[1].starts_with("/put/acc/Sugar/manifests/"));

        // Games of other launchers (or with anti-cheat) stay with the Epic Launcher.
        *anti_cheat.lock().unwrap() = true;
        let blocked = manager.preview_store("epic-sugar").await.unwrap();
        assert_eq!(blocked.launcher_only.as_deref(), Some("needs the EA app"));
        let id = manager.update_store("epic-sugar").await.unwrap();
        let failed = wait_for(&manager, &id, DownloadStatus::Failed).await;
        assert_eq!(failed.download.error.as_deref(), Some("needs the EA app"));
    }

    #[test]
    fn titles_from_file_names() {
        assert_eq!(
            title_from_file_name("lantern-1.2_linux.tar.gz"),
            crate::scanner::prettify_title("lantern-1.2_linux")
        );
        assert_eq!(
            title_from_file_name("Game.AppImage"),
            crate::scanner::prettify_title("Game")
        );
    }
}
