//! Save sync for every game through the user's own cloud (Google Drive,
//! `OneDrive` or Dropbox, `cloud.rs`). Where a game keeps its saves comes from the save
//! backups (Ludusavi manifest); each file is stored under a name another
//! computer can resolve (`<winAppData>/…`, `paths::portable`), so a save made
//! on Windows lands in the game's Proton prefix on a Steam Deck.
//!
//! In the cloud, per game (keyed by its manifest name): `manifest.json`
//! (revision, device, files with SHA-256) and `blobs/<sha256>`. Blobs go up
//! first, the manifest last, so a manifest only names blobs that are there.
//!
//! Before a start newer cloud saves come down (the local ones are backed up
//! first), after a session changed local saves go up. When both sides changed
//! since this computer's last sync and differ, nothing is overwritten: the
//! game shows a conflict until the user picks a side, and the other side is
//! kept as a save backup. Local files are never deleted.

pub mod cloud;

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use sqlx::SqlitePool;

use self::cloud::{Endpoints, Grant, Remote, Session, SESSION_KEY};
use crate::auth::loopback::LoopbackServer;
use crate::backups::{paths, BackupService, SaveSet};
use crate::catalog::secrets::SecretStore;
use crate::db;
use crate::error::{AppError, AppResult};
use crate::launcher::HostEnv;
use crate::models::{
    AppSettings, CloudProvider, GameSaveSync, LibraryItem, SaveSyncConflict, SaveSyncKeep,
    SaveSyncState, SaveSyncStatus, StoreSource,
};
use crate::stores::login::OpenUrl;

const ROOT: &str = "cove-saves/v1";
const FORMAT: &str = "cove-saves";
const LOGIN_TIMEOUT: Duration = Duration::from_secs(300);
/// A start waits at most this long for the cloud, then the game starts anyway.
const BEFORE_LAUNCH_LIMIT: Duration = Duration::from_secs(30);

/// What the cloud holds of one game.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct Manifest {
    format: String,
    version: u32,
    revision: String,
    device: String,
    saved_at: String,
    title: String,
    files: Vec<ManifestFile>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ManifestFile {
    /// `paths::portable` form.
    path: String,
    size: u64,
    sha256: String,
    /// Seconds since 1970.
    modified: i64,
}

#[derive(Debug, Clone)]
struct LocalFile {
    portable: String,
    path: PathBuf,
    size: u64,
    modified: i64,
    /// The modification time to the nanosecond: a save rewritten within the
    /// second of the last sync (same size) still counts as changed.
    stamp: u128,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
struct Row {
    revision: Option<String>,
    fingerprint: Option<String>,
    synced_at: Option<String>,
    conflict: Option<SaveSyncConflict>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Step {
    InSync,
    Pull,
    Push,
    /// Both sides may have changed: compare the contents.
    Compare,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Mode {
    /// Only fetch; local changes go up after the session.
    BeforeLaunch,
    AfterSession,
    Now,
}

/// What the UI hears about a game's sync.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Notice {
    Changed,
    Conflict,
}

pub type Notify = Arc<dyn Fn(&LibraryItem, Notice) + Send + Sync>;

/// The files of a save set that have a name other computers understand.
fn local_files(set: &SaveSet) -> Vec<LocalFile> {
    set.files
        .iter()
        .filter_map(|(path, size, modified)| {
            Some(LocalFile {
                portable: paths::portable(path, &set.contexts)?,
                path: path.clone(),
                size: *size,
                modified: *modified,
                stamp: std::fs::metadata(path)
                    .and_then(|m| m.modified())
                    .ok()
                    .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
                    .map_or(0, |d| d.as_nanos()),
            })
        })
        .collect()
}

fn fingerprint(files: &[LocalFile]) -> String {
    let mut hash = Sha256::new();
    for file in files {
        hash.update(file.portable.as_bytes());
        hash.update([0]);
        hash.update(file.size.to_le_bytes());
        hash.update(file.stamp.to_le_bytes());
    }
    cloud::hex(&hash.finalize())
}

fn hash_file(path: &Path) -> AppResult<String> {
    use std::io::Read as _;
    let mut file = std::fs::File::open(path)?;
    let mut hash = Sha256::new();
    let mut buf = vec![0u8; 64 * 1024];
    loop {
        let n = file.read(&mut buf)?;
        if n == 0 {
            return Ok(cloud::hex(&hash.finalize()));
        }
        hash.update(&buf[..n]);
    }
}

/// `Hollow Knight` → `hollow-knight-1a2b3c4d5e6f`: the game's folder in the cloud.
fn remote_key(name: &str) -> String {
    let slug: String = db::slugify(name).chars().take(60).collect();
    let digest = cloud::hex(&Sha256::digest(name.as_bytes()));
    format!("{slug}-{}", &digest[..12])
}

fn manifest_name(key: &str) -> String {
    format!("{ROOT}/{key}/manifest.json")
}

fn blob_name(key: &str, sha: &str) -> String {
    format!("{ROOT}/{key}/blobs/{sha}")
}

fn decide(row: Option<&Row>, print: &str, local_empty: bool, remote: Option<&Manifest>) -> Step {
    let Some(remote) = remote else {
        return if local_empty {
            Step::InSync
        } else {
            Step::Push
        };
    };
    let Some(row) = row.filter(|r| r.revision.is_some()) else {
        // First sync on this computer.
        return if local_empty {
            Step::Pull
        } else {
            Step::Compare
        };
    };
    let local_changed = row.fingerprint.as_deref() != Some(print);
    let remote_changed = row.revision.as_deref() != Some(remote.revision.as_str());
    match (local_changed, remote_changed) {
        (false, false) => Step::InSync,
        (false, true) => Step::Pull,
        (true, false) => Step::Push,
        (true, true) => Step::Compare,
    }
}

/// The store's own cloud keeps this game's saves: Cove leaves them alone.
fn store_cloud(item: &LibraryItem, cloud: &[String], settings: &AppSettings) -> bool {
    let has = |store: &str| cloud.iter().any(|c| c == store);
    let entry = &item.entry;
    match entry.source {
        StoreSource::Steam => has("steam"),
        StoreSource::Gog => {
            entry.installed_by_cove && settings.library.gog_cloud_saves && has("gog")
        }
        StoreSource::Epic => {
            entry.installed_by_cove && settings.library.epic_cloud_saves && has("epic")
        }
        StoreSource::Amazon | StoreSource::Manual => false,
    }
}

fn now() -> String {
    chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Secs, true)
}

fn set_modified(path: &Path, seconds: i64) {
    let Ok(seconds) = u64::try_from(seconds) else {
        return;
    };
    if let Ok(file) = std::fs::File::options().write(true).open(path) {
        let _ = file.set_modified(std::time::UNIX_EPOCH + Duration::from_secs(seconds));
    }
}

fn status_of(row: Option<&Row>) -> GameSaveSync {
    let state = match row {
        Some(r) if r.conflict.is_some() => SaveSyncState::Conflict,
        Some(r) if r.synced_at.is_some() => SaveSyncState::Synced,
        _ => SaveSyncState::Pending,
    };
    GameSaveSync {
        state,
        synced_at: row.and_then(|r| r.synced_at.clone()),
        conflict: row.and_then(|r| r.conflict.clone()),
    }
}

fn plain(state: SaveSyncState) -> GameSaveSync {
    GameSaveSync {
        state,
        synced_at: None,
        conflict: None,
    }
}

async fn load_row(pool: &SqlitePool, game_id: &str) -> AppResult<Option<Row>> {
    type Columns = (
        Option<String>,
        Option<String>,
        Option<String>,
        Option<String>,
    );
    let row: Option<Columns> = sqlx::query_as(
        "SELECT revision, fingerprint, synced_at, conflict FROM save_sync WHERE game_id = ?",
    )
    .bind(game_id)
    .fetch_optional(pool)
    .await?;
    Ok(row.map(|(revision, fingerprint, synced_at, conflict)| Row {
        revision,
        fingerprint,
        synced_at,
        conflict: conflict.and_then(|c| serde_json::from_str(&c).ok()),
    }))
}

async fn save_row(pool: &SqlitePool, game_id: &str, row: &Row) -> AppResult<()> {
    let conflict = row
        .conflict
        .as_ref()
        .map(serde_json::to_string)
        .transpose()?;
    sqlx::query(
        "INSERT INTO save_sync (game_id, revision, fingerprint, synced_at, conflict)
         VALUES (?, ?, ?, ?, ?)
         ON CONFLICT (game_id) DO UPDATE SET revision = excluded.revision,
           fingerprint = excluded.fingerprint, synced_at = excluded.synced_at,
           conflict = excluded.conflict",
    )
    .bind(game_id)
    .bind(&row.revision)
    .bind(&row.fingerprint)
    .bind(&row.synced_at)
    .bind(conflict)
    .execute(pool)
    .await?;
    Ok(())
}

async fn forget_all(pool: &SqlitePool) -> AppResult<()> {
    sqlx::query("DELETE FROM save_sync").execute(pool).await?;
    Ok(())
}

pub struct SaveSync {
    pool: SqlitePool,
    defaults: AppSettings,
    backups: Arc<BackupService>,
    secrets: Arc<dyn SecretStore>,
    endpoints: Endpoints,
    client: reqwest::Client,
    upload: reqwest::Client,
    /// The access token and until when it is good.
    token: tokio::sync::Mutex<Option<(String, Instant)>>,
    /// One sync at a time.
    busy: tokio::sync::Mutex<()>,
    device: String,
    notify: Notify,
}

/// One game's sync, with everything it needs.
struct Job<'a> {
    item: &'a LibraryItem,
    host: &'a HostEnv,
    remote: Remote,
    set: SaveSet,
    key: String,
    manifest: Option<Manifest>,
}

impl SaveSync {
    pub fn new(
        pool: SqlitePool,
        defaults: AppSettings,
        backups: Arc<BackupService>,
        secrets: Arc<dyn SecretStore>,
        endpoints: Endpoints,
        notify: Notify,
    ) -> Self {
        let (client, upload) = cloud::clients();
        let device = sysinfo::System::host_name()
            .filter(|n| !n.trim().is_empty())
            .unwrap_or_else(|| tr!("Computer", "Computer"));
        Self {
            pool,
            defaults,
            backups,
            secrets,
            endpoints,
            client,
            upload,
            token: tokio::sync::Mutex::new(None),
            busy: tokio::sync::Mutex::new(()),
            device,
            notify,
        }
    }

    async fn session(&self) -> AppResult<Option<Session>> {
        let secrets = self.secrets.clone();
        let raw = tokio::task::spawn_blocking(move || secrets.get(SESSION_KEY))
            .await
            .map_err(|e| AppError::Keyring(e.to_string()))??;
        Ok(raw.and_then(|r| serde_json::from_str(&r).ok()))
    }

    async fn store_session(&self, session: Option<&Session>) -> AppResult<()> {
        let secrets = self.secrets.clone();
        let value = session.map(serde_json::to_string).transpose()?;
        tokio::task::spawn_blocking(move || match value {
            Some(value) => secrets.set(SESSION_KEY, &value),
            None => secrets.delete(SESSION_KEY),
        })
        .await
        .map_err(|e| AppError::Keyring(e.to_string()))?
    }

    pub async fn status(&self) -> AppResult<SaveSyncStatus> {
        let session = self.session().await?;
        Ok(SaveSyncStatus {
            provider: session.as_ref().map(|s| s.provider),
            account: session.as_ref().and_then(|s| s.account.clone()),
            needs_reauth: session.as_ref().is_some_and(|s| s.needs_reauth),
            available: self.endpoints.available(),
            device: self.device.clone(),
        })
    }

    /// Sign-in in the system browser; the cloud's refresh token goes to the keyring.
    pub async fn connect(
        &self,
        provider: CloudProvider,
        open_url: &OpenUrl,
    ) -> AppResult<SaveSyncStatus> {
        let name = cloud::provider_name(provider);
        if !self.endpoints.available().contains(&provider) {
            return Err(AppError::InvalidInput(tr!(
                "Diese Version von Cove kann sich nicht mit {name} verbinden.",
                "This build of Cove can't connect to {name}."
            )));
        }
        let (verifier, challenge) = cloud::pkce()?;
        let state = crate::auth::state::new_state()?;
        let server = match provider {
            CloudProvider::Dropbox => LoopbackServer::bind_port(cloud::DROPBOX_PORT).await?,
            CloudProvider::GoogleDrive | CloudProvider::OneDrive => LoopbackServer::bind().await?,
        };
        let redirect_uri = match provider {
            CloudProvider::GoogleDrive | CloudProvider::Dropbox => server.redirect_uri(),
            CloudProvider::OneDrive => server.localhost_redirect_uri(),
        };
        open_url(&cloud::authorize_url(
            &self.endpoints,
            provider,
            &redirect_uri,
            &state,
            &challenge,
        ))?;
        let callback = server.wait(&state, LOGIN_TIMEOUT).await?;
        let cancelled = || {
            AppError::LoginFailed(tr!(
                "Die Anmeldung bei {name} wurde abgebrochen.",
                "The sign-in to {name} was cancelled."
            ))
        };
        if callback.params.contains_key("error") {
            return Err(cancelled());
        }
        let code = callback.params.get("code").ok_or_else(cancelled)?;
        let token = cloud::exchange(
            &self.client,
            &self.endpoints,
            provider,
            Grant::Code {
                code,
                verifier: &verifier,
                redirect_uri: &redirect_uri,
            },
        )
        .await
        .map_err(|error| match error {
            AppError::ReauthRequired(_) => AppError::LoginFailed(tr!(
                "{name} hat die Anmeldung nicht angenommen.",
                "{name} didn't accept the sign-in."
            )),
            other => other,
        })?;
        let refresh_token = token.refresh.clone().ok_or_else(|| {
            AppError::LoginFailed(tr!(
                "{name} hat keinen dauerhaften Zugang erteilt.",
                "{name} didn't grant lasting access."
            ))
        })?;
        let account =
            cloud::account_name(&self.client, &self.endpoints, provider, &token.access).await;
        let busy = self.busy.lock().await;
        self.store_session(Some(&Session {
            provider,
            refresh_token,
            account,
            needs_reauth: false,
        }))
        .await?;
        *self.token.lock().await = Some((token.access, expiry(token.expires_in)));
        // What this computer matched in another cloud means nothing here.
        forget_all(&self.pool).await?;
        drop(busy);
        self.status().await
    }

    /// Forgets the cloud on this computer (the files in it stay).
    pub async fn disconnect(&self) -> AppResult<SaveSyncStatus> {
        let busy = self.busy.lock().await;
        self.store_session(None).await?;
        *self.token.lock().await = None;
        forget_all(&self.pool).await?;
        drop(busy);
        self.status().await
    }

    /// The connected cloud with a fresh access token; None = not connected.
    async fn remote(&self) -> AppResult<Option<Remote>> {
        let Some(mut session) = self.session().await? else {
            return Ok(None);
        };
        let name = cloud::provider_name(session.provider);
        if session.needs_reauth {
            return Err(AppError::ReauthRequired(name.into()));
        }
        let mut cached = self.token.lock().await;
        let fresh = cached
            .as_ref()
            .filter(|(_, until)| Instant::now() < *until)
            .map(|(token, _)| token.clone());
        let access = if let Some(token) = fresh {
            token
        } else {
            let token = match cloud::exchange(
                &self.client,
                &self.endpoints,
                session.provider,
                Grant::Refresh(&session.refresh_token),
            )
            .await
            {
                Ok(token) => token,
                Err(AppError::ReauthRequired(store)) => {
                    session.needs_reauth = true;
                    self.store_session(Some(&session)).await?;
                    return Err(AppError::ReauthRequired(store));
                }
                Err(error) => return Err(error),
            };
            // OneDrive hands out a new refresh token each time.
            if let Some(new) = token.refresh.filter(|t| *t != session.refresh_token) {
                session.refresh_token = new;
                self.store_session(Some(&session)).await?;
            }
            *cached = Some((token.access.clone(), expiry(token.expires_in)));
            token.access
        };
        Ok(Some(Remote::new(
            self.client.clone(),
            self.upload.clone(),
            self.endpoints.clone(),
            session.provider,
            access,
        )))
    }

    /// The game page: what is known without asking the cloud.
    pub async fn game(&self, game_id: &str) -> AppResult<GameSaveSync> {
        if self.session().await?.is_none() {
            return Ok(plain(SaveSyncState::Off));
        }
        let item = db::library::require(&self.pool, game_id).await?;
        let Some(game) = self.backups.manifest_game(&item).await? else {
            return Ok(plain(SaveSyncState::Unsupported));
        };
        let settings = db::settings::load(&self.pool, &self.defaults).await?;
        if store_cloud(&item, &game.cloud, &settings) {
            return Ok(plain(SaveSyncState::StoreCloud));
        }
        Ok(status_of(load_row(&self.pool, game_id).await?.as_ref()))
    }

    /// Before a start: newer saves from the cloud, at most 30 seconds; the
    /// game starts either way.
    pub async fn before_launch(&self, item: &LibraryItem, host: &HostEnv) {
        if !matches!(self.session().await, Ok(Some(_))) {
            return;
        }
        match tokio::time::timeout(
            BEFORE_LAUNCH_LIMIT,
            self.run(item, host, Mode::BeforeLaunch),
        )
        .await
        {
            Ok(Ok(_)) => {}
            Ok(Err(error)) => eprintln!("[cove] Spielstand-Sync von {}: {error}", item.game.id),
            Err(_) => eprintln!(
                "[cove] Spielstand-Sync von {}: Zeit überschritten",
                item.game.id
            ),
        }
    }

    /// After a session (in the background): changed saves go up.
    pub async fn after_session(&self, game_id: &str, host: &HostEnv) {
        if !matches!(self.session().await, Ok(Some(_))) {
            return;
        }
        let result = match db::library::require(&self.pool, game_id).await {
            Ok(item) => self.run(&item, host, Mode::AfterSession).await,
            Err(error) => Err(error),
        };
        if let Err(error) = result {
            eprintln!("[cove] Spielstand-Sync von {game_id}: {error}");
        }
    }

    /// "Jetzt abgleichen".
    pub async fn sync_now(&self, game_id: &str, host: &HostEnv) -> AppResult<GameSaveSync> {
        let item = db::library::require(&self.pool, game_id).await?;
        self.run(&item, host, Mode::Now).await
    }

    /// Prepares a game's sync; Err(status) when there is nothing to sync.
    async fn job<'a>(
        &self,
        item: &'a LibraryItem,
        host: &'a HostEnv,
    ) -> AppResult<Result<Job<'a>, GameSaveSync>> {
        let Some(remote) = self.remote().await? else {
            return Ok(Err(plain(SaveSyncState::Off)));
        };
        let Some(set) = self.backups.save_set(item, host).await? else {
            return Ok(Err(plain(SaveSyncState::Unsupported)));
        };
        let settings = db::settings::load(&self.pool, &self.defaults).await?;
        if store_cloud(item, &set.game.cloud, &settings) {
            return Ok(Err(plain(SaveSyncState::StoreCloud)));
        }
        let key = remote_key(&set.game.name);
        let manifest = remote
            .get(&manifest_name(&key))
            .await?
            .and_then(|bytes| serde_json::from_slice::<Manifest>(&bytes).ok())
            .filter(|m| m.format == FORMAT && m.version == 1);
        Ok(Ok(Job {
            item,
            host,
            remote,
            set,
            key,
            manifest,
        }))
    }

    async fn run(&self, item: &LibraryItem, host: &HostEnv, mode: Mode) -> AppResult<GameSaveSync> {
        let _busy = self.busy.lock().await;
        let game_id = item.game.id.as_str();
        let row = load_row(&self.pool, game_id).await?;
        // A conflict waits for the user.
        if row.as_ref().is_some_and(|r| r.conflict.is_some()) {
            return Ok(status_of(row.as_ref()));
        }
        let job = match self.job(item, host).await? {
            Ok(job) => job,
            Err(status) => return Ok(status),
        };
        let local = local_files(&job.set);
        let print = fingerprint(&local);
        let mut step = decide(
            row.as_ref(),
            &print,
            local.is_empty(),
            job.manifest.as_ref(),
        );
        if step == Step::Compare {
            let remote_files = job
                .manifest
                .as_ref()
                .map(|m| m.files.clone())
                .unwrap_or_default();
            let contexts = job.set.contexts.clone();
            let same =
                tokio::task::spawn_blocking(move || same_content(&local, &remote_files, &contexts))
                    .await
                    .map_err(|e| AppError::Download(e.to_string()))??;
            if same {
                step = Step::InSync;
            } else {
                let manifest = job.manifest.as_ref();
                let conflict = SaveSyncConflict {
                    device: manifest.map(|m| m.device.clone()).unwrap_or_default(),
                    saved_at: manifest.map(|m| m.saved_at.clone()).unwrap_or_default(),
                };
                let row = Row {
                    conflict: Some(conflict),
                    ..row.clone().unwrap_or_default()
                };
                save_row(&self.pool, game_id, &row).await?;
                (self.notify)(item, Notice::Conflict);
                return Ok(status_of(Some(&row)));
            }
        }
        let row = match step {
            Step::InSync | Step::Compare => {
                let row = Row {
                    revision: job.manifest.as_ref().map(|m| m.revision.clone()),
                    fingerprint: Some(print),
                    synced_at: Some(now()),
                    conflict: None,
                };
                save_row(&self.pool, game_id, &row).await?;
                row
            }
            Step::Pull => {
                let row = self.pull(&job).await?;
                (self.notify)(item, Notice::Changed);
                row
            }
            Step::Push if mode == Mode::BeforeLaunch => return Ok(status_of(row.as_ref())),
            Step::Push => {
                let row = self.push(&job).await?;
                (self.notify)(item, Notice::Changed);
                row
            }
        };
        Ok(status_of(Some(&row)))
    }

    /// The cloud's saves replace the local ones (backed up first).
    async fn pull(&self, job: &Job<'_>) -> AppResult<Row> {
        let Some(manifest) = &job.manifest else {
            return Err(AppError::InvalidInput(tr!(
                "In der Cloud liegen keine Spielstände dieses Spiels.",
                "The cloud holds no saves of this game."
            )));
        };
        if !job.set.files.is_empty() {
            self.backups.back_up(&job.item.game.id, job.host).await?;
        }
        for file in &manifest.files {
            let Some(target) = paths::resolve(&file.path, &job.set.contexts) else {
                continue;
            };
            let check = target.clone();
            let wanted = file.clone();
            let current = tokio::task::spawn_blocking(move || {
                std::fs::metadata(&check)
                    .ok()
                    .filter(|m| m.len() == wanted.size)
                    .and_then(|_| hash_file(&check).ok())
            })
            .await
            .map_err(|e| AppError::Download(e.to_string()))?;
            if current.as_deref() == Some(file.sha256.as_str()) {
                continue;
            }
            if let Some(parent) = target.parent() {
                tokio::fs::create_dir_all(parent).await?;
            }
            let temp = target.with_file_name(format!(
                ".{}.cove-sync",
                target
                    .file_name()
                    .map(|n| n.to_string_lossy().into_owned())
                    .unwrap_or_default()
            ));
            let got = job
                .remote
                .download(&blob_name(&job.key, &file.sha256), &temp)
                .await;
            if !matches!(&got, Ok(Some(sha)) if *sha == file.sha256) {
                let _ = tokio::fs::remove_file(&temp).await;
                got?;
                return Err(AppError::StoreUnavailable(tr!(
                    "Ein Spielstand in der Cloud ist unvollständig. Spiele auf dem anderen Gerät einmal, dann gleicht Cove neu ab.",
                    "A save in the cloud is incomplete. Play once on the other device, then Cove syncs again."
                )));
            }
            tokio::fs::rename(&temp, &target).await?;
            set_modified(&target, file.modified);
        }
        // How the saves look now is what matches this revision.
        let after = self.backups.save_set(job.item, job.host).await?;
        let print = after.map_or_else(String::new, |set| fingerprint(&local_files(&set)));
        let row = Row {
            revision: Some(manifest.revision.clone()),
            fingerprint: Some(print),
            synced_at: Some(now()),
            conflict: None,
        };
        save_row(&self.pool, &job.item.game.id, &row).await?;
        Ok(row)
    }

    /// The local saves go up and become the cloud's.
    async fn push(&self, job: &Job<'_>) -> AppResult<Row> {
        let local = local_files(&job.set);
        let print = fingerprint(&local);
        let to_hash = local.clone();
        let files = tokio::task::spawn_blocking(move || {
            to_hash
                .iter()
                .map(|f| {
                    Ok(ManifestFile {
                        path: f.portable.clone(),
                        size: f.size,
                        sha256: hash_file(&f.path)?,
                        modified: f.modified,
                    })
                })
                .collect::<AppResult<Vec<_>>>()
        })
        .await
        .map_err(|e| AppError::Download(e.to_string()))??;
        let old: HashSet<String> = job
            .manifest
            .as_ref()
            .map(|m| m.files.iter().map(|f| f.sha256.clone()).collect())
            .unwrap_or_default();
        let mut sent = HashSet::new();
        for (file, source) in files.iter().zip(&local) {
            if old.contains(&file.sha256) || !sent.insert(file.sha256.clone()) {
                continue;
            }
            job.remote
                .put_file(&blob_name(&job.key, &file.sha256), &source.path)
                .await?;
        }
        let manifest = Manifest {
            format: FORMAT.into(),
            version: 1,
            revision: crate::auth::state::new_state()?,
            device: self.device.clone(),
            saved_at: now(),
            title: job.item.game.title.clone(),
            files,
        };
        job.remote
            .put_bytes(
                &manifest_name(&job.key),
                serde_json::to_vec_pretty(&manifest)?,
                true,
            )
            .await?;
        // Blobs only the old manifest named.
        let new: HashSet<&str> = manifest.files.iter().map(|f| f.sha256.as_str()).collect();
        for sha in old.iter().filter(|s| !new.contains(s.as_str())) {
            if let Err(error) = job.remote.delete(&blob_name(&job.key, sha)).await {
                eprintln!("[cove] Alter Spielstand in der Cloud bleibt liegen: {error}");
            }
        }
        let row = Row {
            revision: Some(manifest.revision),
            fingerprint: Some(print),
            synced_at: Some(now()),
            conflict: None,
        };
        save_row(&self.pool, &job.item.game.id, &row).await?;
        Ok(row)
    }

    /// A conflict: one side wins, the other is kept as a save backup.
    pub async fn resolve(
        &self,
        game_id: &str,
        keep: SaveSyncKeep,
        host: &HostEnv,
    ) -> AppResult<GameSaveSync> {
        let item = db::library::require(&self.pool, game_id).await?;
        let _busy = self.busy.lock().await;
        let job = match self.job(&item, host).await? {
            Ok(job) => job,
            Err(status) => return Ok(status),
        };
        let row = match keep {
            SaveSyncKeep::Cloud => self.pull(&job).await?,
            SaveSyncKeep::Local => {
                if let Some(manifest) = &job.manifest {
                    self.keep_cloud_copy(&job, manifest).await?;
                }
                self.push(&job).await?
            }
        };
        (self.notify)(&item, Notice::Changed);
        Ok(status_of(Some(&row)))
    }

    /// The cloud's saves as a save backup before the local ones replace them.
    async fn keep_cloud_copy(&self, job: &Job<'_>, manifest: &Manifest) -> AppResult<()> {
        let mut name = [0u8; 8];
        getrandom::fill(&mut name).map_err(|e| AppError::Download(e.to_string()))?;
        let temp = std::env::temp_dir().join(format!("cove-sync-{}", cloud::hex(&name)));
        tokio::fs::create_dir_all(&temp).await?;
        let result = async {
            let mut files = Vec::new();
            for (n, file) in manifest.files.iter().enumerate() {
                let Some(target) = paths::resolve(&file.path, &job.set.contexts) else {
                    continue;
                };
                let to = temp.join(n.to_string());
                if let Some(sha) = job
                    .remote
                    .download(&blob_name(&job.key, &file.sha256), &to)
                    .await?
                {
                    if sha == file.sha256 {
                        files.push((target, to));
                    }
                }
            }
            if !files.is_empty() {
                self.backups.import(job.item, files).await?;
            }
            Ok::<(), AppError>(())
        }
        .await;
        let _ = tokio::fs::remove_dir_all(&temp).await;
        result
    }
}

fn expiry(seconds: u64) -> Instant {
    Instant::now() + Duration::from_secs(seconds.saturating_sub(120).max(60))
}

/// Do the local saves hold what the cloud holds? (Blocking.) Files this
/// computer has no place for are left out.
fn same_content(
    local: &[LocalFile],
    remote: &[ManifestFile],
    contexts: &[paths::Context],
) -> AppResult<bool> {
    let remote: HashMap<&str, &ManifestFile> = remote
        .iter()
        .filter(|f| paths::resolve(&f.path, contexts).is_some())
        .map(|f| (f.path.as_str(), f))
        .collect();
    if remote.len() != local.len() {
        return Ok(false);
    }
    for file in local {
        let Some(theirs) = remote.get(file.portable.as_str()) else {
            return Ok(false);
        };
        if theirs.size != file.size || hash_file(&file.path)? != theirs.sha256 {
            return Ok(false);
        }
    }
    Ok(true)
}

#[cfg(test)]
mod tests;
