//! Store accounts: login sessions (OS keyring only), token refresh, library
//! import, local detection and achievement sync. All store traffic goes
//! through the C# connectors sidecar; the core passes the session per call.

pub mod login;

use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use std::time::Duration;

use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use sqlx::SqlitePool;

use crate::catalog::secrets::SecretStore;
use crate::db;
use crate::db::achievements::SyncedAchievement;
use crate::db::store_games::{LocalInstall, StoreGameInput, Upsert};
use crate::error::{AppError, AppResult};
use crate::models::{
    AchievementsSyncSummary, GameId, ImportSummary, LocalStoreGame, StoreAccount, StoreProblem,
    StoreProblemKind, StoreSource, SyncPhase, SyncProgress,
};
use crate::sidecar::{RpcError, Sidecar, SidecarErrors, SidecarStatus};

/// Refresh tokens this long before they expire.
const REFRESH_MARGIN: chrono::TimeDelta = chrono::TimeDelta::minutes(5);

/// A DLC of a GOG or Epic game the account owns.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OwnedDlc {
    /// GOG product id or Epic app name.
    pub id: String,
    pub title: String,
    pub unlock_only: bool,
}

/// What the connectors return after a login. Tokens never leave the core
/// except as parameters to the connectors; `Debug` redacts them.
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StoreSession {
    pub store: String,
    pub account_id: String,
    pub display_name: String,
    pub access_token: Option<String>,
    pub refresh_token: Option<String>,
    pub expires_at: Option<String>,
}

impl std::fmt::Debug for StoreSession {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("StoreSession")
            .field("store", &self.store)
            .field("account_id", &self.account_id)
            .field("display_name", &self.display_name)
            .field("access_token", &self.access_token.as_ref().map(|_| "***"))
            .field("refresh_token", &self.refresh_token.as_ref().map(|_| "***"))
            .field("expires_at", &self.expires_at)
            .finish()
    }
}

impl StoreSession {
    fn expires_soon(&self, now: chrono::DateTime<chrono::Utc>) -> bool {
        self.expires_at
            .as_deref()
            .and_then(|e| chrono::DateTime::parse_from_rfc3339(e).ok())
            .is_some_and(|e| e.to_utc() - now < REFRESH_MARGIN)
    }
}

/// `store.loginStart` answer.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LoginStart {
    pub url: String,
    pub mode: crate::models::ConnectorLoginMode,
    pub callback_prefix: Option<String>,
    pub fallback_url: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
struct OwnedGame {
    store_game_id: String,
    title: String,
    playtime_minutes: Option<i64>,
    last_played_at: Option<String>,
    image_url: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
struct LocalGame {
    store: String,
    store_game_id: String,
    title: String,
    install_path: String,
    executable_path: Option<String>,
    size_bytes: Option<i64>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
struct StoreAchievement {
    id: String,
    title: String,
    description: String,
    icon_url: Option<String>,
    icon_locked_url: Option<String>,
    hidden: bool,
    unlocked_at: Option<String>,
    rarity_percent: Option<f64>,
}

/// JSON-RPC access to the connectors (the sidecar in the app, a fake in tests).
#[async_trait]
pub trait ConnectorRpc: Send + Sync {
    async fn call(&self, method: &str, params: Value) -> AppResult<Value>;

    /// Health for Settings → Über Cove.
    fn status(&self) -> SidecarStatus {
        SidecarStatus::Idle
    }
}

#[async_trait]
impl ConnectorRpc for Sidecar {
    async fn call(&self, method: &str, params: Value) -> AppResult<Value> {
        Sidecar::call(self, method, params).await
    }

    fn status(&self) -> SidecarStatus {
        Sidecar::status(self)
    }
}

/// Maps `error.data.kind` from the connectors to app errors.
fn connector_rpc_error(error: RpcError) -> AppError {
    let data = error.data.as_ref();
    let kind = data
        .and_then(|d| d.get("kind"))
        .and_then(Value::as_str)
        .unwrap_or("");
    match kind {
        "authFailed" => AppError::LoginFailed(error.message),
        "privateProfile" => AppError::PrivateProfile(error.message),
        "network" => AppError::Offline(error.message),
        "invalidRequest" => AppError::InvalidInput(error.message),
        _ => AppError::StoreUnavailable(error.message),
    }
}

pub const CONNECTOR_ERRORS: SidecarErrors = SidecarErrors {
    unavailable: AppError::ConnectorsUnavailable,
    rpc: connector_rpc_error,
};

/// Help link for a private Steam profile (also sent by the connectors).
fn help_url(error: &AppError) -> Option<String> {
    matches!(error, AppError::PrivateProfile(_))
        .then(|| "https://steamcommunity.com/my/edit/settings".to_owned())
}

pub type ProgressFn = Arc<dyn Fn(SyncProgress) + Send + Sync>;

pub struct StoreService {
    rpc: Arc<dyn ConnectorRpc>,
    pool: SqlitePool,
    secrets: Arc<dyn SecretStore>,
    proxy_url: Option<String>,
    /// Pause between per-game store calls (rate limits).
    pacing: Duration,
    sync_lock: tokio::sync::Mutex<()>,
}

fn secret_key(store: StoreSource) -> String {
    format!("store-session:{}", store.as_str())
}

fn connectable(store: StoreSource) -> AppResult<StoreSource> {
    if store == StoreSource::Manual {
        return Err(AppError::InvalidInput(tr!(
            "Manuelle Spiele haben kein Konto.",
            "Manual games have no account."
        )));
    }
    Ok(store)
}

impl StoreService {
    pub fn new(
        rpc: Arc<dyn ConnectorRpc>,
        pool: SqlitePool,
        secrets: Arc<dyn SecretStore>,
        proxy_url: Option<String>,
        pacing: Duration,
    ) -> Self {
        Self {
            rpc,
            pool,
            secrets,
            proxy_url,
            pacing,
            sync_lock: tokio::sync::Mutex::new(()),
        }
    }

    #[cfg(test)]
    pub fn pool(&self) -> &SqlitePool {
        &self.pool
    }

    pub fn connector_status(&self) -> SidecarStatus {
        self.rpc.status()
    }

    pub async fn list(&self) -> AppResult<Vec<StoreAccount>> {
        db::sources::list(&self.pool).await
    }

    // ───────────── login ─────────────

    pub async fn login_start(
        &self,
        store: StoreSource,
        redirect_uri: &str,
        state: &str,
    ) -> AppResult<LoginStart> {
        let store = connectable(store)?;
        let value = self
            .rpc
            .call(
                "store.loginStart",
                json!({ "store": store.as_str(), "redirectUri": redirect_uri, "state": state }),
            )
            .await?;
        Ok(serde_json::from_value(value)?)
    }

    /// Exchanges the callback (Steam) or code (GOG/Epic) for a session and stores it.
    pub async fn login_complete(
        &self,
        store: StoreSource,
        callback_url: Option<&str>,
        code: Option<&str>,
        expected_return_to: Option<&str>,
    ) -> AppResult<StoreAccount> {
        let store = connectable(store)?;
        let value = self
            .rpc
            .call(
                "store.loginComplete",
                json!({
                    "store": store.as_str(),
                    "callbackUrl": callback_url,
                    "code": code.map(str::trim),
                    "expectedReturnTo": expected_return_to,
                    "proxyUrl": self.proxy_url,
                }),
            )
            .await?;
        let session: StoreSession = serde_json::from_value(value)?;
        self.save_session(store, &session)?;
        db::sources::set_connected(&self.pool, store, &session.display_name).await
    }

    fn save_session(&self, store: StoreSource, session: &StoreSession) -> AppResult<()> {
        self.secrets
            .set(&secret_key(store), &serde_json::to_string(session)?)
    }

    fn load_session(&self, store: StoreSource) -> AppResult<Option<StoreSession>> {
        self.secrets
            .get(&secret_key(store))?
            .map(|raw| serde_json::from_str(&raw).map_err(AppError::from))
            .transpose()
    }

    /// A valid session; refreshed when it expires soon. A failed refresh marks
    /// the account "Erneut anmelden".
    pub async fn session(&self, store: StoreSource) -> AppResult<StoreSession> {
        let Some(session) = self.load_session(store)? else {
            let account = db::sources::get(&self.pool, store).await?;
            if account.connected {
                // The keyring lost the login (e.g. a new keyring): ask again.
                db::sources::set_needs_reauth(&self.pool, store).await?;
                return Err(AppError::ReauthRequired(store.display_name().into()));
            }
            return Err(AppError::InvalidInput(tr!(
                "Du bist nicht bei {} angemeldet.",
                "You're not signed in to {}.",
                store.display_name()
            )));
        };
        if !session.expires_soon(chrono::Utc::now()) {
            return Ok(session);
        }
        self.refresh_session(store, session).await
    }

    async fn refresh_session(
        &self,
        store: StoreSource,
        session: StoreSession,
    ) -> AppResult<StoreSession> {
        match self
            .rpc
            .call(
                "store.refresh",
                json!({ "session": session, "proxyUrl": self.proxy_url }),
            )
            .await
        {
            Ok(value) => {
                let fresh: StoreSession = serde_json::from_value(value)?;
                self.save_session(store, &fresh)?;
                db::sources::set_connected(&self.pool, store, &fresh.display_name).await?;
                Ok(fresh)
            }
            Err(AppError::LoginFailed(_)) => {
                db::sources::set_needs_reauth(&self.pool, store).await?;
                Err(AppError::ReauthRequired(store.display_name().into()))
            }
            Err(other) => Err(other),
        }
    }

    /// Manual "Aktualisieren": forces a token refresh (no-op for Steam).
    pub async fn refresh(&self, store: StoreSource) -> AppResult<StoreAccount> {
        let session = self.session(store).await?;
        self.refresh_session(store, session).await?;
        db::sources::get(&self.pool, store).await
    }

    /// Deletes the tokens; imported games stay unless `keep_games` is false.
    pub async fn logout(&self, store: StoreSource, keep_games: bool) -> AppResult<StoreAccount> {
        let store = connectable(store)?;
        self.secrets.delete(&secret_key(store))?;
        if !keep_games {
            db::store_games::remove_store(&self.pool, store).await?;
        }
        db::sources::set_disconnected(&self.pool, store).await
    }

    /// Runs a store call with the session; an auth failure marks the account.
    async fn with_session(
        &self,
        store: StoreSource,
        method: &str,
        extra: Value,
    ) -> AppResult<Value> {
        let session = self.session(store).await?;
        let mut params = json!({ "session": session, "proxyUrl": self.proxy_url });
        if let (Value::Object(target), Value::Object(more)) = (&mut params, extra) {
            target.extend(more);
        }
        match self.rpc.call(method, params).await {
            Err(AppError::LoginFailed(_)) => {
                db::sources::set_needs_reauth(&self.pool, store).await?;
                Err(AppError::ReauthRequired(store.display_name().into()))
            }
            other => other,
        }
    }

    /// Install plan for a GOG game (connector `gog.plan`): files, chunks, CDN links.
    pub async fn gog_plan(
        &self,
        product_id: &str,
        platform: &str,
        language: &str,
    ) -> AppResult<crate::downloads::gog::GogPlan> {
        let value = self
            .with_session(
                StoreSource::Gog,
                "gog.plan",
                json!({ "productId": product_id, "platform": platform, "language": language }),
            )
            .await?;
        Ok(serde_json::from_value(value)?)
    }

    /// DLCs of a Steam game (store list, installed ones from Steam's app manifest).
    pub async fn steam_dlcs(&self, app_id: &str) -> AppResult<crate::models::StoreDlcList> {
        #[derive(Deserialize)]
        #[serde(rename_all = "camelCase")]
        struct Dlc {
            app_id: String,
            title: String,
            installed: bool,
        }
        #[derive(Deserialize)]
        struct List {
            dlcs: Vec<Dlc>,
            total: i64,
        }
        let home = dirs_home().ok_or_else(|| {
            AppError::Launch(tr!(
                "Kein Benutzerordner gefunden.",
                "No home folder found."
            ))
        })?;
        let value = self
            .rpc
            .call(
                "steam.dlcs",
                json!({
                    "appId": app_id,
                    "home": home,
                    "programData": std::env::var("ProgramData").ok(),
                }),
            )
            .await?;
        let list: List = serde_json::from_value(value)?;
        Ok(crate::models::StoreDlcList {
            dlcs: list
                .dlcs
                .into_iter()
                .map(|d| crate::models::StoreDlc {
                    id: d.app_id,
                    title: d.title,
                    installed: d.installed,
                    unlock_only: false,
                })
                .collect(),
            total: list.total,
        })
    }

    /// DLCs of a GOG or Epic game the account owns (connector `gog.dlcs` / `epic.dlcs`).
    pub async fn owned_dlcs(
        &self,
        store: StoreSource,
        store_game_id: &str,
        platform: &str,
    ) -> AppResult<Vec<OwnedDlc>> {
        #[derive(Deserialize)]
        struct List {
            dlcs: Vec<OwnedDlc>,
        }
        let (method, params) = match store {
            StoreSource::Gog => (
                "gog.dlcs",
                json!({ "productId": store_game_id, "platform": platform }),
            ),
            StoreSource::Epic => (
                "epic.dlcs",
                json!({ "appName": store_game_id, "platform": platform }),
            ),
            _ => {
                return Err(AppError::InvalidInput(tr!(
                    "Gekaufte DLCs gibt es nur für GOG- und Epic-Spiele.",
                    "Owned DLCs are only available for GOG and Epic games."
                )))
            }
        };
        let list: List = serde_json::from_value(self.with_session(store, method, params).await?)?;
        Ok(list.dlcs)
    }

    /// Manifest links and catalogue attributes of an Epic game.
    pub async fn epic_install_info(
        &self,
        app_name: &str,
        platform: &str,
    ) -> AppResult<crate::downloads::epic::InstallInfo> {
        let value = self
            .with_session(
                StoreSource::Epic,
                "epic.installInfo",
                json!({ "appName": app_name, "platform": platform }),
            )
            .await?;
        Ok(serde_json::from_value(value)?)
    }

    /// A one-time exchange code (and ownership token) to start an Epic game.
    pub async fn epic_launch(
        &self,
        app_name: &str,
        ownership_token: bool,
    ) -> AppResult<crate::downloads::epic::LaunchAuth> {
        let value = self
            .with_session(
                StoreSource::Epic,
                "epic.launch",
                json!({ "appName": app_name, "ownershipToken": ownership_token }),
            )
            .await?;
        Ok(serde_json::from_value(value)?)
    }

    /// The cloud-save files of an Epic game.
    pub async fn epic_saves(
        &self,
        app_name: &str,
    ) -> AppResult<crate::downloads::epic::saves::Saves> {
        let value = self
            .with_session(
                StoreSource::Epic,
                "epic.saves",
                json!({ "appName": app_name }),
            )
            .await?;
        Ok(serde_json::from_value(value)?)
    }

    /// Signed upload links for new cloud-save files.
    pub async fn epic_write_links(
        &self,
        app_name: &str,
        paths: &[String],
    ) -> AppResult<HashMap<String, String>> {
        let value = self
            .with_session(
                StoreSource::Epic,
                "epic.saveWriteLinks",
                json!({ "appName": app_name, "paths": paths }),
            )
            .await?;
        Ok(serde_json::from_value(
            value.get("links").cloned().unwrap_or(Value::Null),
        )?)
    }

    /// GOG cloud saves of a game: its Galaxy client, the synced folders and a
    /// token for that client (connector `gog.cloudSaves`).
    pub async fn gog_cloud_saves(
        &self,
        product_id: &str,
    ) -> AppResult<crate::downloads::gog_saves::CloudSaves> {
        let value = self
            .with_session(
                StoreSource::Gog,
                "gog.cloudSaves",
                json!({ "productId": product_id, "platform": "windows" }),
            )
            .await?;
        Ok(serde_json::from_value(value)?)
    }

    /// Newest public GOG build (update check).
    pub async fn gog_latest(
        &self,
        product_id: &str,
        platform: &str,
        language: &str,
    ) -> AppResult<crate::downloads::gog::GogLatest> {
        let value = self
            .with_session(
                StoreSource::Gog,
                "gog.latest",
                json!({ "productId": product_id, "platform": platform, "language": language }),
            )
            .await?;
        Ok(serde_json::from_value(value)?)
    }

    async fn note_problem(&self, store: StoreSource, error: &AppError) -> AppResult<()> {
        let kind = match error {
            AppError::PrivateProfile(_) => StoreProblemKind::PrivateProfile,
            AppError::StoreUnavailable(_) => StoreProblemKind::Unavailable,
            _ => return Ok(()),
        };
        let problem = StoreProblem {
            kind,
            message: error.user_message(),
            help_url: help_url(error),
        };
        db::sources::set_problem(&self.pool, store, Some(&problem)).await
    }

    // ───────────── local + import ─────────────

    pub async fn detect_local(&self, store: Option<StoreSource>) -> AppResult<Vec<LocalStoreGame>> {
        let home = dirs_home().ok_or_else(|| {
            AppError::Launch(tr!(
                "Kein Benutzerordner gefunden.",
                "No home folder found."
            ))
        })?;
        let value = self
            .rpc
            .call(
                "store.detectLocal",
                json!({
                    "home": home,
                    "programData": std::env::var("ProgramData").ok(),
                    "store": store.map(StoreSource::as_str),
                }),
            )
            .await?;
        let games: Vec<LocalGame> =
            serde_json::from_value(value.get("games").cloned().unwrap_or(Value::Null))?;
        let known = db::store_games::known(&self.pool).await?;
        Ok(games
            .into_iter()
            .filter_map(|g| {
                let store = StoreSource::parse(&g.store).filter(|s| *s != StoreSource::Manual)?;
                Some(LocalStoreGame {
                    in_library: known.contains(&(g.store.clone(), g.store_game_id.clone())),
                    store,
                    store_game_id: g.store_game_id,
                    title: g.title,
                    install_path: g.install_path,
                    executable_path: g.executable_path,
                    size_bytes: g.size_bytes,
                })
            })
            .collect())
    }

    /// Owned games (when signed in) merged with local installs of the store.
    /// Returns the summary and the ids of new games (to match against the catalogue).
    pub async fn import_library(
        &self,
        store: StoreSource,
        progress: &ProgressFn,
    ) -> AppResult<(ImportSummary, Vec<GameId>)> {
        let store = connectable(store)?;
        let _guard = self.sync_lock.lock().await;
        let signed_in = db::sources::get(&self.pool, store).await?.connected;

        let mut games: Vec<StoreGameInput> = Vec::new();
        if signed_in {
            match self.with_session(store, "store.library", json!({})).await {
                Ok(value) => {
                    let owned: Vec<OwnedGame> =
                        serde_json::from_value(value.get("games").cloned().unwrap_or(Value::Null))?;
                    db::sources::set_problem(&self.pool, store, None).await?;
                    games.extend(owned.into_iter().map(|g| StoreGameInput {
                        store_game_id: g.store_game_id,
                        title: g.title,
                        playtime_minutes: g.playtime_minutes,
                        last_played_at: g.last_played_at,
                        image_url: g.image_url,
                        install: None,
                    }));
                }
                Err(error) => {
                    self.note_problem(store, &error).await?;
                    return Err(error);
                }
            }
        }
        for local in self.detect_local(Some(store)).await? {
            if local.store != store {
                continue;
            }
            let install = LocalInstall {
                install_path: local.install_path,
                executable_path: local.executable_path,
                size_bytes: local.size_bytes,
            };
            match games
                .iter_mut()
                .find(|g| g.store_game_id == local.store_game_id)
            {
                Some(owned) => owned.install = Some(install),
                None => games.push(StoreGameInput {
                    store_game_id: local.store_game_id,
                    title: local.title,
                    install: Some(install),
                    ..StoreGameInput::default()
                }),
            }
        }

        let total = i64::try_from(games.len()).unwrap_or(i64::MAX);
        let mut summary = ImportSummary {
            store,
            added: 0,
            updated: 0,
            total,
        };
        let mut added = Vec::new();
        for (i, game) in games.iter().enumerate() {
            match db::store_games::upsert(&self.pool, store, game).await? {
                Upsert::Added(id) => {
                    summary.added += 1;
                    added.push(id);
                }
                Upsert::Updated(_) => summary.updated += 1,
                // Real title now known: match it against the catalogue like a new game.
                Upsert::Renamed(id) => {
                    summary.updated += 1;
                    added.push(id);
                }
            }
            if i % 25 == 0 || i + 1 == games.len() {
                progress(SyncProgress {
                    store,
                    phase: SyncPhase::Library,
                    done: i64::try_from(i + 1).unwrap_or(total),
                    total,
                    finished: i + 1 == games.len(),
                });
            }
        }
        if signed_in {
            let owned: HashSet<String> = games.iter().map(|g| g.store_game_id.clone()).collect();
            let removed = db::store_games::remove_missing(&self.pool, store, &owned).await?;
            if removed > 0 {
                eprintln!("[stores] {removed} {store:?} games are no longer in the library");
            }
        }
        db::sources::mark_synced(&self.pool, store).await?;
        Ok((summary, added))
    }

    // ───────────── achievements ─────────────

    /// Reads achievements of played games, paced to respect rate limits.
    pub async fn sync_achievements(
        &self,
        store: StoreSource,
        progress: &ProgressFn,
    ) -> AppResult<AchievementsSyncSummary> {
        let store = connectable(store)?;
        let _guard = self.sync_lock.lock().await;
        let games = db::store_games::of_store(&self.pool, store, true).await?;
        let total = i64::try_from(games.len()).unwrap_or(i64::MAX);
        let mut summary = AchievementsSyncSummary::default();
        for (i, (game_id, store_game_id)) in games.iter().enumerate() {
            if i > 0 {
                tokio::time::sleep(self.pacing).await;
            }
            let result = self
                .with_session(
                    store,
                    "store.achievements",
                    json!({ "storeGameId": store_game_id }),
                )
                .await;
            match result {
                Ok(value) => {
                    let list: Vec<StoreAchievement> = serde_json::from_value(
                        value.get("achievements").cloned().unwrap_or(Value::Null),
                    )?;
                    if !list.is_empty() {
                        let synced: Vec<SyncedAchievement> = list
                            .into_iter()
                            .map(|a| SyncedAchievement {
                                id: a.id,
                                title: a.title,
                                description: a.description,
                                icon_url: a.icon_url,
                                icon_locked_url: a.icon_locked_url,
                                hidden: a.hidden,
                                unlocked_at: a.unlocked_at,
                                rarity_percent: a.rarity_percent,
                            })
                            .collect();
                        summary.achievements += i64::try_from(synced.len()).unwrap_or(0);
                        summary.newly_unlocked +=
                            db::achievements::store_synced(&self.pool, game_id, &synced).await?;
                        summary.games += 1;
                    }
                }
                // One broken game must not stop the sync; account-wide problems do.
                Err(
                    error @ (AppError::ReauthRequired(_)
                    | AppError::PrivateProfile(_)
                    | AppError::Offline(_)),
                ) => {
                    self.note_problem(store, &error).await?;
                    progress(SyncProgress {
                        store,
                        phase: SyncPhase::Achievements,
                        done: total,
                        total,
                        finished: true,
                    });
                    return Err(error);
                }
                Err(error) => eprintln!("[cove] Achievements für {game_id}: {error}"),
            }
            progress(SyncProgress {
                store,
                phase: SyncPhase::Achievements,
                done: i64::try_from(i + 1).unwrap_or(total),
                total,
                finished: i + 1 == games.len(),
            });
        }
        Ok(summary)
    }
}

fn dirs_home() -> Option<String> {
    std::env::var_os(if cfg!(windows) { "USERPROFILE" } else { "HOME" })
        .map(|h| h.to_string_lossy().into_owned())
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use crate::catalog::secrets::MemoryStore;
    use crate::db::open_in_memory;
    use crate::models::StoreAccountStatus;
    use std::sync::Mutex;

    type Handler = Box<dyn Fn(&str, &Value) -> AppResult<Value> + Send + Sync>;

    /// Records calls and answers like the C# connectors.
    pub(crate) struct FakeRpc {
        pub calls: Mutex<Vec<(String, Value)>>,
        handler: Handler,
    }

    impl FakeRpc {
        pub(crate) fn new(
            handler: impl Fn(&str, &Value) -> AppResult<Value> + Send + Sync + 'static,
        ) -> Arc<Self> {
            Arc::new(Self {
                calls: Mutex::new(Vec::new()),
                handler: Box::new(handler),
            })
        }

        fn methods(&self) -> Vec<String> {
            self.calls
                .lock()
                .unwrap()
                .iter()
                .map(|(m, _)| m.clone())
                .collect()
        }
    }

    #[async_trait]
    impl ConnectorRpc for FakeRpc {
        async fn call(&self, method: &str, params: Value) -> AppResult<Value> {
            let result = (self.handler)(method, &params);
            self.calls.lock().unwrap().push((method.to_owned(), params));
            result
        }
    }

    fn session(store: &str, expires_at: Option<&str>, access: &str) -> Value {
        json!({ "store": store, "accountId": "acc", "displayName": "Kiefer", "accessToken": access,
                "refreshToken": "r1", "expiresAt": expires_at })
    }

    async fn service(rpc: Arc<FakeRpc>) -> (StoreService, Arc<MemoryStore>) {
        let secrets = Arc::new(MemoryStore::default());
        let pool = open_in_memory().await.unwrap();
        (
            StoreService::new(
                rpc,
                pool,
                secrets.clone(),
                Some("https://proxy.test".into()),
                Duration::ZERO,
            ),
            secrets,
        )
    }

    fn noop() -> ProgressFn {
        Arc::new(|_| {})
    }

    #[tokio::test]
    async fn login_stores_tokens_in_the_keyring_only() {
        let rpc = FakeRpc::new(|method, _| match method {
            "store.loginComplete" => Ok(session(
                "gog",
                Some("2099-01-01T00:00:00Z"),
                "secret-access",
            )),
            _ => Ok(Value::Null),
        });
        let (stores, secrets) = service(rpc.clone()).await;
        let account = stores
            .login_complete(StoreSource::Gog, None, Some(" c0de "), None)
            .await
            .unwrap();
        assert_eq!(account.status, StoreAccountStatus::Connected);
        assert_eq!(account.account_name.as_deref(), Some("Kiefer"));
        assert_eq!(rpc.calls.lock().unwrap()[0].1["code"], "c0de");

        let saved = secrets.get("store-session:gog").unwrap().unwrap();
        assert!(saved.contains("secret-access"));
        // Nothing token-like in the database.
        let dump: Vec<(String,)> = sqlx::query_as(
            "SELECT COALESCE(account_name, '') || COALESCE(problem, '') FROM sources",
        )
        .fetch_all(stores.pool())
        .await
        .unwrap();
        assert!(dump
            .iter()
            .all(|(row,)| !row.contains("secret") && !row.contains("r1")));
        // Debug output is redacted.
        let session: StoreSession = serde_json::from_str(&saved).unwrap();
        assert!(!format!("{session:?}").contains("secret-access"));
    }

    #[tokio::test]
    async fn refreshes_expiring_tokens_and_flags_failed_refreshes() {
        let rpc = FakeRpc::new(|method, params| match method {
            "store.loginComplete" => Ok(session("epic", Some("2000-01-01T00:00:00Z"), "old")),
            "store.refresh" if params["session"]["accessToken"] == "old" => {
                Ok(session("epic", Some("2099-01-01T00:00:00Z"), "new"))
            }
            "store.refresh" => Err(AppError::LoginFailed(
                "Bitte melde dich erneut bei Epic an.".into(),
            )),
            "store.library" | "store.detectLocal" => Ok(json!({ "games": [] })),
            _ => Ok(Value::Null),
        });
        let (stores, secrets) = service(rpc.clone()).await;
        stores
            .login_complete(StoreSource::Epic, None, Some("abc"), None)
            .await
            .unwrap();

        // Expired → refreshed transparently before the library call.
        stores
            .import_library(StoreSource::Epic, &noop())
            .await
            .unwrap();
        let library_call = rpc
            .calls
            .lock()
            .unwrap()
            .iter()
            .find(|(m, _)| m == "store.library")
            .cloned()
            .unwrap();
        assert_eq!(library_call.1["session"]["accessToken"], "new");
        assert_eq!(library_call.1["proxyUrl"], "https://proxy.test");

        // Now the refresh token is dead: "Erneut anmelden".
        stores
            .save_session(
                StoreSource::Epic,
                &serde_json::from_value(session("epic", Some("2000-01-01T00:00:00Z"), "dead"))
                    .unwrap(),
            )
            .unwrap();
        let error = stores
            .import_library(StoreSource::Epic, &noop())
            .await
            .unwrap_err();
        assert!(matches!(error, AppError::ReauthRequired(_)));
        assert_eq!(
            db::sources::get(stores.pool(), StoreSource::Epic)
                .await
                .unwrap()
                .status,
            StoreAccountStatus::NeedsReauth
        );

        // Logout deletes the tokens and (optionally) the games.
        stores.logout(StoreSource::Epic, false).await.unwrap();
        assert!(secrets.get("store-session:epic").unwrap().is_none());
        assert_eq!(
            db::sources::get(stores.pool(), StoreSource::Epic)
                .await
                .unwrap()
                .status,
            StoreAccountStatus::Disconnected
        );
    }

    #[tokio::test]
    async fn import_merges_owned_and_installed_games() {
        let rpc = FakeRpc::new(|method, _| match method {
            "store.loginComplete" => Ok(
                json!({ "store": "steam", "accountId": "7656", "displayName": "Gabe",
                                                "accessToken": null, "refreshToken": null, "expiresAt": null }),
            ),
            "store.library" => Ok(json!({ "games": [
                { "storeGameId": "620", "title": "Portal 2", "playtimeMinutes": 754, "lastPlayedAt": null, "imageUrl": null },
                { "storeGameId": "400", "title": "Portal", "playtimeMinutes": 0, "lastPlayedAt": null, "imageUrl": null } ] })),
            "store.detectLocal" => Ok(json!({ "games": [
                { "store": "steam", "storeGameId": "620", "title": "Portal 2", "installPath": "/s/Portal 2",
                  "executablePath": null, "sizeBytes": 10, "source": "steam-manifest" },
                { "store": "steam", "storeGameId": "105600", "title": "Terraria", "installPath": "/s/Terraria",
                  "executablePath": null, "sizeBytes": 20, "source": "steam-manifest" } ] })),
            _ => Ok(Value::Null),
        });
        let (stores, _) = service(rpc.clone()).await;
        stores
            .login_complete(
                StoreSource::Steam,
                Some("http://127.0.0.1/cb?x"),
                None,
                Some("http://127.0.0.1/cb"),
            )
            .await
            .unwrap();

        let events = Arc::new(Mutex::new(Vec::new()));
        let sink = events.clone();
        let progress: ProgressFn = Arc::new(move |p| sink.lock().unwrap().push(p));
        let (summary, added) = stores
            .import_library(StoreSource::Steam, &progress)
            .await
            .unwrap();
        assert_eq!((summary.added, summary.updated, summary.total), (3, 0, 3));
        assert_eq!(added.len(), 3);
        assert!(events.lock().unwrap().last().unwrap().finished);

        let portal = db::library::require(stores.pool(), "steam-620")
            .await
            .unwrap();
        assert!(portal.entry.installed);
        assert_eq!(portal.entry.playtime_minutes, 754);
        let (again, added) = stores
            .import_library(StoreSource::Steam, &noop())
            .await
            .unwrap();
        assert_eq!((again.added, again.updated, added.len()), (0, 3, 0));

        let local = stores.detect_local(None).await.unwrap();
        assert!(local.iter().all(|g| g.in_library));
    }

    #[tokio::test]
    async fn private_profiles_are_shown_with_a_help_link() {
        let rpc = FakeRpc::new(|method, _| match method {
            "store.loginComplete" => Ok(
                json!({ "store": "steam", "accountId": "1", "displayName": "x",
                                                "accessToken": null, "refreshToken": null, "expiresAt": null }),
            ),
            "store.library" => Err(AppError::PrivateProfile(
                "Dein Steam-Profil ist privat.".into(),
            )),
            _ => Ok(json!({ "games": [] })),
        });
        let (stores, _) = service(rpc).await;
        stores
            .login_complete(StoreSource::Steam, Some("cb"), None, None)
            .await
            .unwrap();
        assert!(matches!(
            stores.import_library(StoreSource::Steam, &noop()).await,
            Err(AppError::PrivateProfile(_))
        ));
        let problem = db::sources::get(stores.pool(), StoreSource::Steam)
            .await
            .unwrap()
            .problem
            .unwrap();
        assert_eq!(problem.kind, StoreProblemKind::PrivateProfile);
        assert_eq!(
            problem.help_url.as_deref(),
            Some("https://steamcommunity.com/my/edit/settings")
        );
    }

    #[tokio::test]
    async fn syncs_achievements_of_played_games_and_skips_broken_ones() {
        let rpc = FakeRpc::new(|method, params| match method {
            "store.loginComplete" => Ok(
                json!({ "store": "steam", "accountId": "1", "displayName": "x",
                                                "accessToken": null, "refreshToken": null, "expiresAt": null }),
            ),
            "store.library" => Ok(json!({ "games": [
                { "storeGameId": "620", "title": "Portal 2", "playtimeMinutes": 10, "lastPlayedAt": null, "imageUrl": null },
                { "storeGameId": "999", "title": "Broken", "playtimeMinutes": 5, "lastPlayedAt": null, "imageUrl": null },
                { "storeGameId": "400", "title": "Never played", "playtimeMinutes": 0, "lastPlayedAt": null, "imageUrl": null } ] })),
            "store.achievements" if params["storeGameId"] == "999" => {
                Err(AppError::StoreUnavailable("kaputt".into()))
            }
            "store.achievements" => Ok(json!({ "achievements": [
                { "id": "A", "title": "Erster", "description": "", "iconUrl": null, "iconLockedUrl": null,
                  "hidden": false, "unlockedAt": "2026-09-01T10:00:00Z", "rarityPercent": 50.0 },
                { "id": "B", "title": "Zweiter", "description": "", "iconUrl": null, "iconLockedUrl": null,
                  "hidden": false, "unlockedAt": null, "rarityPercent": 1.5 } ] })),
            _ => Ok(json!({ "games": [] })),
        });
        let (stores, _) = service(rpc.clone()).await;
        stores
            .login_complete(StoreSource::Steam, Some("cb"), None, None)
            .await
            .unwrap();
        stores
            .import_library(StoreSource::Steam, &noop())
            .await
            .unwrap();

        let summary = stores
            .sync_achievements(StoreSource::Steam, &noop())
            .await
            .unwrap();
        assert_eq!(
            summary,
            AchievementsSyncSummary {
                games: 1,
                achievements: 2,
                newly_unlocked: 1
            }
        );
        let achievement_calls = rpc
            .methods()
            .iter()
            .filter(|m| *m == "store.achievements")
            .count();
        assert_eq!(achievement_calls, 2, "never-played games are skipped");
        let item = db::library::require(stores.pool(), "steam-620")
            .await
            .unwrap();
        assert_eq!(
            (
                item.entry.achievements.unlocked,
                item.entry.achievements.total
            ),
            (1, 2)
        );
    }

    #[tokio::test]
    async fn not_signed_in_and_lost_keyring() {
        let rpc = FakeRpc::new(|_, _| Ok(json!({ "games": [] })));
        let (stores, _) = service(rpc).await;
        assert!(matches!(
            stores.session(StoreSource::Gog).await,
            Err(AppError::InvalidInput(_))
        ));
        // Connected in the DB but the keyring entry is gone → reauth.
        db::sources::set_connected(stores.pool(), StoreSource::Gog, "x")
            .await
            .unwrap();
        assert!(matches!(
            stores.session(StoreSource::Gog).await,
            Err(AppError::ReauthRequired(_))
        ));
        assert!(matches!(
            stores.logout(StoreSource::Manual, true).await,
            Err(AppError::InvalidInput(_))
        ));
        // Not signed in: import still brings in installed games.
        let (summary, _) = stores
            .import_library(StoreSource::Epic, &noop())
            .await
            .unwrap();
        assert_eq!(summary.total, 0);
    }

    #[test]
    fn maps_connector_error_kinds() {
        let err = |kind: &str| {
            connector_rpc_error(RpcError {
                code: 2000,
                message: "m".into(),
                data: Some(json!({ "kind": kind })),
            })
        };
        assert!(matches!(err("authFailed"), AppError::LoginFailed(_)));
        assert!(matches!(err("privateProfile"), AppError::PrivateProfile(_)));
        assert!(matches!(err("network"), AppError::Offline(_)));
        assert!(matches!(err("invalidRequest"), AppError::InvalidInput(_)));
        assert!(matches!(err("unavailable"), AppError::StoreUnavailable(_)));
        assert!(matches!(
            connector_rpc_error(RpcError {
                code: -32601,
                message: "x".into(),
                data: None
            }),
            AppError::StoreUnavailable(_)
        ));
    }
}

/// Runs against the real connectors binary (`npm run build:connectors -w @cove/desktop`).
#[cfg(test)]
mod sidecar_contract {
    use super::*;
    use crate::sidecar::{is_placeholder, SidecarConfig};
    use std::path::PathBuf;

    fn built() -> Option<Sidecar> {
        if !cfg!(all(target_os = "linux", target_arch = "x86_64")) {
            return None;
        }
        let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("binaries/cove-connectors-x86_64-unknown-linux-gnu");
        (!is_placeholder(&path)).then(|| {
            Sidecar::with_errors(
                Some(SidecarConfig::new("connectors", path, Vec::new())),
                CONNECTOR_ERRORS,
            )
        })
    }

    #[tokio::test]
    #[allow(clippy::too_many_lines)] // one walk through every connector method
    async fn real_connectors_understand_core_requests() {
        let Some(sidecar) = built() else {
            eprintln!("skipped: connectors not built (npm run build:connectors -w @cove/desktop)");
            return;
        };
        let rpc: Arc<dyn ConnectorRpc> = Arc::new(sidecar);
        let pool = crate::db::open_in_memory().await.unwrap();
        let stores = StoreService::new(
            rpc.clone(),
            pool,
            Arc::new(crate::catalog::secrets::MemoryStore::default()),
            None,
            Duration::ZERO,
        );

        let steam = stores
            .login_start(StoreSource::Steam, "http://127.0.0.1:4711/callback", "s1")
            .await
            .unwrap();
        assert_eq!(steam.mode, crate::models::ConnectorLoginMode::Browser);
        assert!(steam
            .url
            .contains("openid.return_to=http%3A%2F%2F127.0.0.1%3A4711%2Fcallback%3Fstate%3Ds1"));

        let gog = stores.login_start(StoreSource::Gog, "", "s").await.unwrap();
        assert_eq!(
            gog.callback_prefix.as_deref(),
            Some("https://embed.gog.com/on_login_success")
        );
        let epic = stores
            .login_start(StoreSource::Epic, "", "s")
            .await
            .unwrap();
        assert!(epic.fallback_url.is_some());

        // Error kinds survive the trip (no network needed for an invalid code).
        let error = stores
            .login_complete(StoreSource::Epic, None, Some("not a code!"), None)
            .await
            .unwrap_err();
        assert!(matches!(error, AppError::InvalidInput(_)), "{error:?}");
        crate::i18n::set_thread_english(Some(true));
        let english = stores
            .login_complete(StoreSource::Epic, None, Some("not a code!"), None)
            .await
            .unwrap_err();
        crate::i18n::set_thread_english(None);
        assert_eq!(
            english.user_message(),
            "That's not a valid Epic sign-in code."
        );

        // GOG install plans: the method exists and checks its input before any request.
        let plan = rpc
            .call(
                "gog.plan",
                json!({ "session": { "store": "gog", "accountId": "1", "displayName": "x", "accessToken": "a",
                                     "refreshToken": "r", "expiresAt": null },
                        "productId": "../1", "platform": "linux", "language": "de", "proxyUrl": null }),
            )
            .await
            .unwrap_err();
        assert!(matches!(plan, AppError::InvalidInput(_)), "{plan:?}");
        let dlcs = rpc
            .call(
                "gog.dlcs",
                json!({ "session": { "store": "gog", "accountId": "1", "displayName": "x", "accessToken": "a",
                                     "refreshToken": "r", "expiresAt": null },
                        "productId": "../1", "platform": "linux", "proxyUrl": null }),
            )
            .await
            .unwrap_err();
        assert!(matches!(dlcs, AppError::InvalidInput(_)), "{dlcs:?}");
        let saves = rpc
            .call(
                "gog.cloudSaves",
                json!({ "session": { "store": "gog", "accountId": "1", "displayName": "x", "accessToken": "a",
                                     "refreshToken": "r", "expiresAt": null },
                        "productId": "../1", "platform": "windows", "proxyUrl": null }),
            )
            .await
            .unwrap_err();
        assert!(matches!(saves, AppError::InvalidInput(_)), "{saves:?}");
        for method in ["epic.installInfo", "epic.launch", "epic.saves", "epic.dlcs"] {
            let error = rpc
                .call(
                    method,
                    json!({ "session": { "store": "epic", "accountId": "1", "displayName": "x", "accessToken": "a",
                                         "refreshToken": "r", "expiresAt": null },
                            "appName": "../x", "platform": "linux", "ownershipToken": false, "paths": null, "proxyUrl": null }),
                )
                .await
                .unwrap_err();
            assert!(
                matches!(error, AppError::InvalidInput(_)),
                "{method}: {error:?}"
            );
        }

        // Steam DLC lists refuse odd app ids before any network call.
        let error = rpc
            .call(
                "steam.dlcs",
                json!({ "appId": "../x", "home": "/nowhere", "programData": null }),
            )
            .await
            .unwrap_err();
        assert!(matches!(error, AppError::InvalidInput(_)), "{error:?}");

        // Local detection with a fake home folder.
        let home = tempfile::tempdir().unwrap();
        let apps = home.path().join(".steam/steam/steamapps");
        std::fs::create_dir_all(&apps).unwrap();
        std::fs::write(apps.join("libraryfolders.vdf"), "\"libraryfolders\" { }").unwrap();
        std::fs::write(
            apps.join("appmanifest_105600.acf"),
            "\"AppState\" { \"appid\" \"105600\" \"name\" \"Terraria\" \"installdir\" \"Terraria\" \"StateFlags\" \"4\" }",
        )
        .unwrap();
        let value = rpc
            .call(
                "store.detectLocal",
                json!({ "home": home.path(), "programData": null, "store": null }),
            )
            .await
            .unwrap();
        let games: Vec<LocalGame> = serde_json::from_value(value["games"].clone()).unwrap();
        assert_eq!(games.len(), 1);
        assert_eq!(
            (games[0].store.as_str(), games[0].title.as_str()),
            ("steam", "Terraria")
        );
    }
}
