//! Download manager: queue, parallel transfers with resume, SHA-256 check,
//! unpacking and adding the game to the library. Store games are handed to
//! their own client. All state is in SQLite, so downloads survive restarts.

pub mod extract;
pub mod handoff;
pub mod itch;
pub mod limiter;
pub mod transfer;

use std::collections::{BTreeSet, HashMap};
use std::path::{Path, PathBuf};
use std::sync::atomic::Ordering;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use sqlx::SqlitePool;
use tokio::sync::{watch, Notify};

use crate::db;
use crate::error::{AppError, AppResult};
use crate::models::{
    AppSettings, DirectDownloadInput, DownloadOrigin, DownloadProgress, DownloadStatus, GameId,
    StoreSource,
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
        }
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

        let url = match DownloadOrigin::parse(&record.origin) {
            DownloadOrigin::Itch => {
                let upload = record
                    .itch_upload_id
                    .ok_or_else(|| AppError::Itch(tr!("Unbekannte Datei.", "Unknown file.")))?;
                self.itch
                    .resolve_download(upload, record.itch_download_key_id)
                    .await?
            }
            _ => record
                .url
                .clone()
                .ok_or_else(|| AppError::Download(tr!("Kein Link vorhanden.", "No link given.")))?,
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

        self.install(&record, &part, &file_name).await?;
        let _ = std::fs::remove_dir(&work_dir); // only if empty
        Ok(Outcome::Completed)
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
        let target = PathBuf::from(record.target_dir.clone().unwrap_or_default());
        let title = record.title.clone();
        let source = part.to_path_buf();
        let name = file_name.to_owned();
        let (install_dir, executable, size) =
            tokio::task::spawn_blocking(move || -> AppResult<_> {
                let dir = extract::unique_dir(&target, &title);
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

    // ───────────── commands ─────────────

    pub async fn add_url(&self, input: &DirectDownloadInput) -> AppResult<String> {
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
        let settings = self.settings().await?;
        let id = db::downloads::enqueue_transfer(
            &self.pool,
            &db::downloads::NewTransfer {
                title,
                origin: Some(DownloadOrigin::Direct),
                url: Some(url),
                sha256,
                target_dir: settings.downloads.directory,
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
        game_id: i64,
        upload_id: i64,
        download_key_id: Option<i64>,
        title: &str,
    ) -> AppResult<String> {
        let upload = self
            .itch
            .uploads(game_id, download_key_id)
            .await?
            .into_iter()
            .find(|u| u.upload_id == upload_id)
            .ok_or_else(|| {
                AppError::Itch(tr!(
                    "Diese Datei gehört nicht zu dem Spiel.",
                    "This file doesn't belong to the game."
                ))
            })?;
        let settings = self.settings().await?;
        let id = db::downloads::enqueue_transfer(
            &self.pool,
            &db::downloads::NewTransfer {
                title: title.to_owned(),
                origin: Some(DownloadOrigin::Itch),
                file_name: Some(upload.file_name),
                target_dir: settings.downloads.directory,
                itch_game_id: Some(game_id),
                itch_upload_id: Some(upload_id),
                itch_download_key_id: download_key_id,
                ..db::downloads::NewTransfer::default()
            },
        )
        .await?;
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
                if record.origin != "direct" && record.origin != "itch" {
                    return Err(AppError::InvalidInput(tr!(
                        "Store-Installationen laufen im Store-Client.",
                        "Store installs run in the store client."
                    )));
                }
                self.set_status(id, DownloadStatus::Queued).await?;
                self.wake();
            }
            DownloadStatus::Cancelled => {
                let was_running = signal(Control::Cancel);
                if !was_running {
                    if let Ok(record) = db::downloads::record(&self.pool, id).await {
                        Self::remove_files(&record);
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
    use std::io::Write;
    use std::sync::atomic::AtomicUsize;
    use wiremock::matchers::{method, path};
    use wiremock::{Mock, MockServer};

    struct Recorder {
        changed: AtomicUsize,
        progress: AtomicUsize,
        installed: Mutex<Vec<String>>,
        opened: Mutex<Vec<String>>,
    }

    async fn manager(dir: &Path) -> (Arc<DownloadManager>, Arc<Recorder>) {
        let pool = open_in_memory().await.unwrap();
        let mut defaults = crate::state::default_settings(None);
        defaults.downloads.directory = dir.to_string_lossy().into_owned();
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
        let itch = ItchApi::new(Arc::new(MemoryStore::default()), "http://127.0.0.1:1");
        let manager = Arc::new(DownloadManager::new(pool, defaults, itch, None, hooks));
        (manager, rec)
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
            .add_url(&DirectDownloadInput {
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
        if cfg!(target_os = "linux") {
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
            .add_url(&DirectDownloadInput {
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
            manager.add_url(&bad("ftp://x/y", None)).await,
            Err(AppError::InvalidInput(_))
        ));
        assert!(matches!(
            manager.add_url(&bad("nonsense", None)).await,
            Err(AppError::InvalidInput(_))
        ));
        assert!(matches!(
            manager
                .add_url(&bad("https://x.test/a.zip", Some("abc")))
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
            .add_url(&DirectDownloadInput {
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
