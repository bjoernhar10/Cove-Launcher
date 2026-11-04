//! Tauri commands. Argument names are camelCase on the JS side
//! (`invoke("library_update", { gameId, patch })`).

use std::path::Path;
use std::sync::Arc;
use std::time::Duration;

use tauri::{AppHandle, Emitter, Manager, Runtime, State};

use crate::catalog::CatalogService;
use crate::db;
use crate::error::{AppError, AppResult};
use crate::models::{
    AboutInfo, Achievement, AchievementUnlock, AchievementsSyncSummary, AppSettings,
    CatalogCredentials, CatalogFilter, CatalogGameDetails, CatalogPage, CatalogProviderId,
    CatalogSort, CatalogStatus, ComponentVersion, ConnectorLoginFinished, ConnectorLoginStart,
    CoveAchievement, DownloadItem, DownloadStatus, Game, GameId, ImportSummary, LaunchResult,
    LibraryItem, LibraryMatchEvent, LibraryUpdate, LocalStoreGame, ManualGameInput, MatchCandidate,
    Platform, PlaytimeStats, PluginInfo, Recommendation, RunningGame, ScanCandidate, StoreAccount,
    StoreSource, SyncPhase, SyncProgress, UserProfile,
};
use crate::models::{
    DirectDownloadInput, ItchGame, ItchStatus, ItchUpload, PluginGameInput, PluginManifestPreview,
    PluginPermission, ServiceStatus, UpdateCheck,
};
use crate::scanner;
use crate::sidecar::SidecarStatus;
use crate::state::AppState;
use crate::stores::login::{self, LOGIN_FINISHED};
use crate::stores::ProgressFn;

pub const DOWNLOADS_CHANGED: &str = "downloads-changed";
pub const LIBRARY_MATCH_CHANGED: &str = "library-match-changed";
pub const DOWNLOAD_PROGRESS: &str = "download-progress";
pub const STORE_SYNC_PROGRESS: &str = "store-sync-progress";
pub const ACHIEVEMENT_UNLOCKED: &str = "achievement-unlocked";

/// Pause between catalogue matches of imported games (proxy: 60 requests/min per IP).
const MATCH_PACING: Duration = Duration::from_millis(1100);

// ───────────── library ─────────────

#[tauri::command]
pub async fn library_list(state: State<'_, AppState>) -> AppResult<Vec<LibraryItem>> {
    db::library::list(&state.pool).await
}

#[tauri::command]
pub async fn library_get(
    state: State<'_, AppState>,
    game_id: String,
) -> AppResult<Option<LibraryItem>> {
    db::library::get(&state.pool, &game_id).await
}

#[tauri::command]
pub async fn library_add_manual(
    state: State<'_, AppState>,
    input: ManualGameInput,
) -> AppResult<LibraryItem> {
    let path = input.executable_path.trim();
    if !path.is_empty() && !Path::new(path).is_file() {
        return Err(AppError::InvalidInput(format!(
            "Datei nicht gefunden: {path}"
        )));
    }
    let item = db::library::add_manual(&state.pool, &input).await?;
    state.cove_achievements.check_in_background();
    Ok(item)
}

/// Adds a catalogue game with its metadata.
#[tauri::command]
pub async fn library_add_from_catalog(
    state: State<'_, AppState>,
    game: Game,
) -> AppResult<LibraryItem> {
    let item = db::library::add_from_catalog(&state.pool, &game).await?;
    state.cove_achievements.check_in_background();
    Ok(item)
}

#[tauri::command]
pub async fn library_remove(state: State<'_, AppState>, game_id: String) -> AppResult<()> {
    if state
        .launcher
        .running()
        .iter()
        .any(|r| r.game_id == game_id)
    {
        return Err(AppError::AlreadyRunning(game_id));
    }
    db::library::remove(&state.pool, &game_id).await
}

#[tauri::command]
pub async fn library_update(
    state: State<'_, AppState>,
    game_id: String,
    patch: LibraryUpdate,
) -> AppResult<LibraryItem> {
    if let Some(path) = patch.executable_path.as_deref().map(str::trim) {
        if !path.is_empty() && !Path::new(path).is_file() {
            return Err(AppError::InvalidInput(format!(
                "Datei nicht gefunden: {path}"
            )));
        }
    }
    db::library::update(&state.pool, &game_id, &patch).await
}

#[tauri::command]
pub async fn library_scan_folder(
    state: State<'_, AppState>,
    path: String,
) -> AppResult<Vec<ScanCandidate>> {
    let known = db::library::executable_paths(&state.pool).await?;
    tauri::async_runtime::spawn_blocking(move || scanner::scan(Path::new(path.trim()), &known))
        .await
        .map_err(|e| AppError::InvalidInput(format!("Scan abgebrochen: {e}")))?
}

#[tauri::command]
pub async fn library_achievements(
    state: State<'_, AppState>,
    game_id: String,
) -> AppResult<Vec<Achievement>> {
    db::achievements::list(&state.pool, &game_id).await
}

#[tauri::command]
pub async fn library_playtime(
    state: State<'_, AppState>,
    game_id: String,
) -> AppResult<PlaytimeStats> {
    db::library::require(&state.pool, &game_id).await?;
    db::sessions::stats(&state.pool, &game_id, chrono::Utc::now().date_naive()).await
}

#[tauri::command]
pub async fn achievements_recent(
    state: State<'_, AppState>,
    limit: i64,
) -> AppResult<Vec<AchievementUnlock>> {
    db::achievements::recent(&state.pool, limit.clamp(1, 50)).await
}

// ───────────── running games ─────────────

#[tauri::command]
pub async fn game_launch(state: State<'_, AppState>, game_id: String) -> AppResult<LaunchResult> {
    let item = db::library::require(&state.pool, &game_id).await?;
    state.launcher.launch(&item, &state.host)
}

#[tauri::command]
pub fn game_stop(state: State<'_, AppState>, game_id: String) -> AppResult<()> {
    state.launcher.stop(&game_id)
}

#[tauri::command]
pub fn game_running_list(state: State<'_, AppState>) -> Vec<RunningGame> {
    state.launcher.running()
}

// ───────────── settings, sources, plugins ─────────────

#[tauri::command]
pub async fn settings_get(state: State<'_, AppState>) -> AppResult<AppSettings> {
    db::settings::load(&state.pool, &state.defaults).await
}

#[tauri::command]
pub async fn settings_update(
    state: State<'_, AppState>,
    settings: AppSettings,
) -> AppResult<AppSettings> {
    db::settings::save(&state.pool, &settings).await?;
    // Bandwidth limit and parallel downloads apply at once.
    state.downloads.wake();
    db::settings::load(&state.pool, &state.defaults).await
}

#[tauri::command]
pub async fn plugins_list(state: State<'_, AppState>) -> AppResult<Vec<PluginInfo>> {
    state.plugins.list().await
}

#[tauri::command]
pub async fn plugins_set_enabled(
    state: State<'_, AppState>,
    id: String,
    enabled: bool,
) -> AppResult<PluginInfo> {
    let info = state.plugins.set_enabled(&id, enabled).await?;
    // Plugin achievements count only while the plugin is active.
    state.cove_achievements.check_in_background();
    Ok(info)
}

/// The user confirmed the permissions in the dialog.
#[tauri::command]
pub async fn plugins_approve(state: State<'_, AppState>, id: String) -> AppResult<PluginInfo> {
    state.plugins.approve(&id).await
}

#[tauri::command]
pub fn plugins_inspect(
    state: State<'_, AppState>,
    path: String,
) -> AppResult<PluginManifestPreview> {
    state.plugins.inspect(Path::new(path.trim()))
}

#[tauri::command]
pub async fn plugins_install(state: State<'_, AppState>, path: String) -> AppResult<PluginInfo> {
    state.plugins.install(Path::new(path.trim())).await
}

#[tauri::command]
pub async fn plugins_uninstall(state: State<'_, AppState>, id: String) -> AppResult<()> {
    state.plugins.uninstall(&id).await
}

#[tauri::command]
pub fn plugins_open_folder<R: Runtime>(
    app: AppHandle<R>,
    state: State<'_, AppState>,
) -> AppResult<()> {
    use tauri_plugin_opener::OpenerExt;
    let dir = state.plugins.user_dir();
    std::fs::create_dir_all(dir)?;
    app.opener()
        .open_path(dir.to_string_lossy(), None::<&str>)
        .map_err(|e| {
            AppError::Launch(format!(
                "Der Plugin-Ordner konnte nicht geöffnet werden: {e}"
            ))
        })
}

// ───────────── plugin API (called by the plugin host) ─────────────

#[tauri::command]
pub async fn plugin_storage_get(
    state: State<'_, AppState>,
    plugin_id: String,
    key: String,
) -> AppResult<serde_json::Value> {
    state.plugins.storage_get(&plugin_id, &key).await
}

#[tauri::command]
pub async fn plugin_storage_set(
    state: State<'_, AppState>,
    plugin_id: String,
    key: String,
    value: serde_json::Value,
) -> AppResult<()> {
    state.plugins.storage_set(&plugin_id, &key, &value).await
}

#[tauri::command]
pub async fn plugin_storage_remove(
    state: State<'_, AppState>,
    plugin_id: String,
    key: String,
) -> AppResult<()> {
    state.plugins.storage_remove(&plugin_id, &key).await
}

#[tauri::command]
pub async fn plugin_storage_keys(
    state: State<'_, AppState>,
    plugin_id: String,
) -> AppResult<Vec<String>> {
    state.plugins.storage_keys(&plugin_id).await
}

#[tauri::command]
pub async fn plugin_define_achievements(
    state: State<'_, AppState>,
    plugin_id: String,
    achievements: Vec<serde_json::Value>,
) -> AppResult<usize> {
    let count = state
        .plugins
        .define_achievements(&plugin_id, &achievements)
        .await?;
    state.cove_achievements.check_in_background();
    Ok(count)
}

/// `library.add`: with a path like "Spiel hinzufügen", without one as a not installed game.
#[tauri::command]
pub async fn plugin_library_add<R: Runtime>(
    app: AppHandle<R>,
    state: State<'_, AppState>,
    plugin_id: String,
    game: PluginGameInput,
) -> AppResult<LibraryItem> {
    state
        .plugins
        .require(&plugin_id, PluginPermission::LibraryAdd)
        .await?;
    let item = match game
        .executable_path
        .as_deref()
        .map(str::trim)
        .filter(|p| !p.is_empty())
    {
        Some(path) => {
            if !Path::new(path).is_file() {
                return Err(AppError::InvalidInput(format!(
                    "Datei nicht gefunden: {path}"
                )));
            }
            let input = ManualGameInput {
                title: game.title.clone(),
                executable_path: path.to_owned(),
                size_bytes: None,
            };
            db::library::add_manual(&state.pool, &input).await?
        }
        None => db::library::add_not_installed(&state.pool, &game.title).await?,
    };
    after_install(&app, &state, item.game.id.clone());
    Ok(item)
}

// ───────────── downloads ─────────────

async fn downloads_changed<R: Runtime>(app: &AppHandle<R>, state: &AppState) -> AppResult<()> {
    let items = db::downloads::list(&state.pool).await?;
    // Event delivery failures (no window yet) are not fatal.
    let _ = app.emit(DOWNLOADS_CHANGED, items);
    Ok(())
}

#[tauri::command]
pub async fn downloads_list(state: State<'_, AppState>) -> AppResult<Vec<DownloadItem>> {
    db::downloads::list(&state.pool).await
}

/// Store games: hands the installation to the store's client.
#[tauri::command]
pub async fn downloads_enqueue(state: State<'_, AppState>, game_id: String) -> AppResult<()> {
    state.downloads.hand_off(&game_id).await
}

/// Pause (`paused`), resume or retry (`queued`), cancel (`cancelled`).
#[tauri::command]
pub async fn downloads_set_status(
    state: State<'_, AppState>,
    id: String,
    status: DownloadStatus,
) -> AppResult<()> {
    state.downloads.control(&id, status).await
}

#[tauri::command]
pub async fn downloads_add_url(
    state: State<'_, AppState>,
    input: DirectDownloadInput,
) -> AppResult<DownloadItem> {
    let id = state.downloads.add_url(&input).await?;
    db::downloads::get(&state.pool, &id).await
}

#[tauri::command]
pub async fn downloads_add_itch(
    state: State<'_, AppState>,
    game_id: i64,
    upload_id: i64,
    download_key_id: Option<i64>,
    title: String,
) -> AppResult<DownloadItem> {
    let id = state
        .downloads
        .add_itch(game_id, upload_id, download_key_id, &title)
        .await?;
    db::downloads::get(&state.pool, &id).await
}

#[tauri::command]
pub async fn itch_status(state: State<'_, AppState>) -> AppResult<ItchStatus> {
    state.downloads.itch.status().await
}

#[tauri::command]
pub async fn itch_set_api_key(
    state: State<'_, AppState>,
    api_key: String,
) -> AppResult<ItchStatus> {
    state.downloads.itch.set_key(&api_key).await
}

#[tauri::command]
pub fn itch_clear_api_key(state: State<'_, AppState>) -> AppResult<ItchStatus> {
    state.downloads.itch.clear_key()
}

#[tauri::command]
pub async fn itch_owned_games(state: State<'_, AppState>) -> AppResult<Vec<ItchGame>> {
    state.downloads.itch.owned().await
}

#[tauri::command]
pub async fn itch_lookup(state: State<'_, AppState>, url: String) -> AppResult<ItchGame> {
    state.downloads.itch.lookup(&url).await
}

#[tauri::command]
pub async fn itch_uploads(
    state: State<'_, AppState>,
    game_id: i64,
    download_key_id: Option<i64>,
) -> AppResult<Vec<ItchUpload>> {
    state.downloads.itch.uploads(game_id, download_key_id).await
}

/// A downloaded or store game was installed: complete its metadata and check achievements.
pub fn after_install<R: Runtime>(app: &AppHandle<R>, state: &AppState, game_id: GameId) {
    spawn_match(app, state, game_id);
    state.cove_achievements.check_in_background();
}

#[tauri::command]
pub async fn downloads_reorder<R: Runtime>(
    app: AppHandle<R>,
    state: State<'_, AppState>,
    ordered_ids: Vec<String>,
) -> AppResult<()> {
    db::downloads::reorder(&state.pool, &ordered_ids).await?;
    downloads_changed(&app, &state).await
}

#[tauri::command]
pub async fn downloads_clear_history<R: Runtime>(
    app: AppHandle<R>,
    state: State<'_, AppState>,
) -> AppResult<()> {
    db::downloads::clear_history(&state.pool).await?;
    downloads_changed(&app, &state).await
}

// ───────────── system ─────────────

#[tauri::command]
pub fn system_profile() -> UserProfile {
    let user = std::env::var("USER")
        .or_else(|_| std::env::var("USERNAME"))
        .unwrap_or_else(|_| "Spieler".into());
    let mut chars = user.chars();
    let display_name = chars
        .next()
        .map_or_else(String::new, |c| c.to_uppercase().chain(chars).collect());
    UserProfile {
        id: "local".into(),
        display_name,
        avatar_url: None,
    }
}

#[tauri::command]
pub fn system_about(state: State<'_, AppState>) -> AboutInfo {
    about(state.recommender.status(), state.stores.connector_status())
}

/// Version and service health for Settings → Über Cove. Never starts a sidecar.
pub fn about(recommender: SidecarStatus, connectors: SidecarStatus) -> AboutInfo {
    let version = env!("CARGO_PKG_VERSION").to_owned();
    let status = |s: SidecarStatus| match s {
        SidecarStatus::Running => ServiceStatus::Running,
        SidecarStatus::Idle => ServiceStatus::Idle,
        SidecarStatus::Unavailable => ServiceStatus::Unavailable,
        SidecarStatus::NotInstalled => ServiceStatus::NotInstalled,
    };
    AboutInfo {
        app_version: version.clone(),
        components: vec![
            ComponentVersion {
                name: "core".into(),
                version: version.clone(),
                status: ServiceStatus::Running,
            },
            ComponentVersion {
                name: "connectors".into(),
                version: version.clone(),
                status: status(connectors),
            },
            ComponentVersion {
                name: "recommender".into(),
                version,
                status: status(recommender),
            },
        ],
        platform: Platform::current(),
    }
}

// ───────────── updates ─────────────

#[tauri::command]
pub async fn updates_check<R: Runtime>(app: AppHandle<R>) -> AppResult<UpdateCheck> {
    crate::updates::check(&app).await
}

#[tauri::command]
pub async fn updates_install<R: Runtime>(app: AppHandle<R>) -> AppResult<()> {
    crate::updates::install(&app).await
}

// ───────────── catalogue ─────────────

/// Downloads a game's artwork in the background so it is available offline.
fn prefetch_images(state: &AppState, game: &Game) {
    let catalog = state.catalog.clone();
    let urls = CatalogService::image_urls(game);
    tauri::async_runtime::spawn(async move { catalog.images.prefetch(&urls).await });
}

/// Matches a library game against the catalogue in the background and reports the result.
fn spawn_match<R: Runtime>(app: &AppHandle<R>, state: &AppState, game_id: String) {
    let catalog = state.catalog.clone();
    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        match catalog.match_library_game(&game_id).await {
            Ok(status) => {
                let _ = app.emit(LIBRARY_MATCH_CHANGED, LibraryMatchEvent { game_id, status });
            }
            Err(error) => {
                eprintln!("[cove] Katalog-Zuordnung für {game_id} fehlgeschlagen: {error}");
            }
        }
    });
}

#[tauri::command]
pub async fn catalog_status(state: State<'_, AppState>) -> AppResult<CatalogStatus> {
    state.catalog.status().await
}

#[tauri::command]
pub async fn catalog_set_credentials(
    state: State<'_, AppState>,
    credentials: CatalogCredentials,
) -> AppResult<CatalogStatus> {
    state.catalog.set_credentials(credentials).await
}

#[tauri::command]
pub async fn catalog_clear_credentials(
    state: State<'_, AppState>,
    provider: CatalogProviderId,
) -> AppResult<CatalogStatus> {
    state.catalog.clear_credentials(provider).await
}

#[tauri::command]
pub async fn catalog_search(state: State<'_, AppState>, query: String) -> AppResult<CatalogPage> {
    state.catalog.search(&query).await
}

#[tauri::command]
pub async fn catalog_browse(
    state: State<'_, AppState>,
    filter: CatalogFilter,
    sort: CatalogSort,
    page: u32,
) -> AppResult<CatalogPage> {
    // "Empfohlen": the first pages are re-ranked for this user by the recommender.
    if sort == CatalogSort::Recommended && state.recommender.is_available() {
        match state.recommender.recommended_page(&filter, page).await {
            Ok(Some(personal)) => return Ok(personal),
            Ok(None) => {}
            // Catalogue errors (offline, not configured) surface below; recommender errors fall back.
            Err(error @ (AppError::RecommenderUnavailable(_) | AppError::Recommender(_))) => {
                eprintln!("[cove] Empfohlen ohne Personalisierung: {error}");
            }
            Err(other) => return Err(other),
        }
    }
    state.catalog.browse(filter, sort, page).await
}

#[tauri::command]
pub async fn catalog_game_details(
    state: State<'_, AppState>,
    game_id: String,
) -> AppResult<CatalogGameDetails> {
    let details = state.catalog.details(&game_id).await?;
    prefetch_images(&state, &details.game);
    Ok(details)
}

#[tauri::command]
pub async fn catalog_similar(
    state: State<'_, AppState>,
    game_id: String,
) -> AppResult<CatalogPage> {
    state.catalog.similar(&game_id).await
}

#[tauri::command]
pub async fn library_match_candidates(
    state: State<'_, AppState>,
    game_id: String,
) -> AppResult<Vec<MatchCandidate>> {
    db::library::candidates(&state.pool, &game_id).await
}

#[tauri::command]
pub async fn library_link_catalog(
    state: State<'_, AppState>,
    game_id: String,
    catalog_id: String,
) -> AppResult<LibraryItem> {
    let item = state.catalog.link(&game_id, &catalog_id).await?;
    prefetch_images(&state, &item.game);
    Ok(item)
}

#[tauri::command]
pub async fn library_reject_match(
    state: State<'_, AppState>,
    game_id: String,
) -> AppResult<LibraryItem> {
    db::library::reject_match(&state.pool, &game_id).await
}

/// Runs the automatic matching again (e.g. after the catalogue was set up).
#[tauri::command]
pub async fn library_rematch<R: Runtime>(
    app: AppHandle<R>,
    state: State<'_, AppState>,
    game_id: String,
) -> AppResult<()> {
    db::library::require(&state.pool, &game_id).await?;
    db::library::set_match_status(&state.pool, &game_id, crate::models::MatchStatus::None).await?;
    spawn_match(&app, &state, game_id);
    Ok(())
}

// ───────────── recommendations ─────────────

#[tauri::command]
pub async fn recommendations_for_user(
    state: State<'_, AppState>,
    limit: u32,
) -> AppResult<Vec<Recommendation>> {
    let items = state.recommender.for_user(limit.clamp(1, 50)).await?;
    log_recommended(&state, &items).await;
    Ok(items)
}

/// Remembers shown recommendations for the "Entdecker" achievement (best effort).
async fn log_recommended(state: &AppState, items: &[Recommendation]) {
    let ids: Vec<String> = items.iter().map(|r| r.game.id.clone()).collect();
    if let Err(error) = crate::cove_achievements::log_recommended(&state.pool, &ids).await {
        eprintln!("[cove] Empfehlungen merken: {error}");
    }
}

#[tauri::command]
pub async fn recommendations_discover(
    state: State<'_, AppState>,
    limit: u32,
) -> AppResult<Vec<Recommendation>> {
    let items = state.recommender.discover(limit.clamp(1, 50)).await?;
    log_recommended(&state, &items).await;
    Ok(items)
}

#[tauri::command]
pub async fn recommendations_similar(
    state: State<'_, AppState>,
    game_id: String,
    limit: u32,
) -> AppResult<Vec<Recommendation>> {
    state
        .recommender
        .similar(&game_id, limit.clamp(1, 50))
        .await
}

#[tauri::command]
pub async fn recommendation_explain(
    state: State<'_, AppState>,
    game_id: String,
) -> AppResult<String> {
    state.recommender.explain(&game_id).await
}

// ───────────── store accounts ─────────────

#[tauri::command]
pub async fn connectors_list(state: State<'_, AppState>) -> AppResult<Vec<StoreAccount>> {
    state.stores.list().await
}

#[tauri::command]
pub async fn connector_login_start<R: Runtime>(
    app: AppHandle<R>,
    state: State<'_, AppState>,
    store: StoreSource,
) -> AppResult<ConnectorLoginStart> {
    let sync_app = app.clone();
    let on_login: login::OnLogin = Arc::new(move |store| spawn_store_sync(&sync_app, store));
    login::start(
        &app,
        state.stores.clone(),
        state.login_flows.clone(),
        store,
        state.open_url.clone(),
        on_login,
    )
    .await
}

/// Finishes a login with a code the user pasted (Epic "Alternativer Login").
#[tauri::command]
pub async fn connector_login_complete<R: Runtime>(
    app: AppHandle<R>,
    state: State<'_, AppState>,
    store: StoreSource,
    code: String,
) -> AppResult<StoreAccount> {
    if code.trim().is_empty() {
        return Err(AppError::InvalidInput(
            "Bitte füge den Anmeldecode ein.".into(),
        ));
    }
    state.login_flows.cancel(store);
    close_login_window(&app, store);
    let account = state
        .stores
        .login_complete(store, None, Some(&code), None)
        .await?;
    spawn_store_sync(&app, store);
    Ok(account)
}

fn close_login_window<R: Runtime>(app: &AppHandle<R>, store: StoreSource) {
    if let Some(window) = app.get_webview_window(&login::window_label(store)) {
        let _ = window.destroy();
    }
}

#[tauri::command]
pub fn connector_login_cancel<R: Runtime>(
    app: AppHandle<R>,
    state: State<'_, AppState>,
    store: StoreSource,
) {
    if state.login_flows.cancel(store) {
        close_login_window(&app, store);
        let _ = app.emit(
            LOGIN_FINISHED,
            ConnectorLoginFinished {
                store,
                account: None,
                error: None,
            },
        );
    }
}

#[tauri::command]
pub async fn connector_refresh(
    state: State<'_, AppState>,
    store: StoreSource,
) -> AppResult<StoreAccount> {
    state.stores.refresh(store).await
}

#[tauri::command]
pub async fn connector_logout(
    state: State<'_, AppState>,
    store: StoreSource,
    keep_games: bool,
) -> AppResult<StoreAccount> {
    state.login_flows.cancel(store);
    state.stores.logout(store, keep_games).await
}

#[tauri::command]
pub async fn connector_detect_local(
    state: State<'_, AppState>,
    store: Option<StoreSource>,
) -> AppResult<Vec<LocalStoreGame>> {
    state.stores.detect_local(store).await
}

fn progress_emitter<R: Runtime>(app: &AppHandle<R>) -> ProgressFn {
    let app = app.clone();
    Arc::new(move |progress: SyncProgress| {
        let _ = app.emit(STORE_SYNC_PROGRESS, progress);
    })
}

#[tauri::command]
pub async fn connector_import_library<R: Runtime>(
    app: AppHandle<R>,
    state: State<'_, AppState>,
    store: StoreSource,
) -> AppResult<ImportSummary> {
    let (summary, added) = state
        .stores
        .import_library(store, &progress_emitter(&app))
        .await?;
    spawn_matching(&app, &state, store, added);
    state.cove_achievements.check_in_background();
    Ok(summary)
}

/// Matches imported games against the catalogue, one at a time.
fn spawn_matching<R: Runtime>(
    app: &AppHandle<R>,
    state: &AppState,
    store: StoreSource,
    game_ids: Vec<GameId>,
) {
    if game_ids.is_empty() {
        return;
    }
    let catalog = state.catalog.clone();
    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        let total = i64::try_from(game_ids.len()).unwrap_or(i64::MAX);
        for (i, game_id) in game_ids.into_iter().enumerate() {
            if i > 0 {
                tokio::time::sleep(MATCH_PACING).await;
            }
            match catalog.match_library_game(&game_id).await {
                Ok(status) => {
                    let _ = app.emit(LIBRARY_MATCH_CHANGED, LibraryMatchEvent { game_id, status });
                }
                Err(error) => {
                    eprintln!("[cove] Katalog-Zuordnung für {game_id} fehlgeschlagen: {error}");
                }
            }
            let done = i64::try_from(i + 1).unwrap_or(total);
            let _ = app.emit(
                STORE_SYNC_PROGRESS,
                SyncProgress {
                    store,
                    phase: SyncPhase::Matching,
                    done,
                    total,
                    finished: done == total,
                },
            );
        }
    });
}

/// After a login: import the library, then read achievements, in the background.
fn spawn_store_sync<R: Runtime>(app: &AppHandle<R>, store: StoreSource) {
    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        let Some(state) = app.try_state::<AppState>() else {
            return;
        };
        let progress = progress_emitter(&app);
        match state.stores.import_library(store, &progress).await {
            Ok((_, added)) => {
                spawn_matching(&app, &state, store, added);
                state.cove_achievements.check_in_background();
            }
            Err(error) => {
                eprintln!("[cove] {}-Import: {error}", store.display_name());
                return;
            }
        }
        if store == StoreSource::Steam {
            if let Err(error) = state.stores.sync_achievements(store, &progress).await {
                eprintln!("[cove] {}-Achievements: {error}", store.display_name());
            }
        }
    });
}

/// Reads store achievements (Steam today; GOG/Epic offer none through their APIs).
#[tauri::command]
pub async fn achievements_sync<R: Runtime>(
    app: AppHandle<R>,
    state: State<'_, AppState>,
    store: Option<StoreSource>,
) -> AppResult<AchievementsSyncSummary> {
    let progress = progress_emitter(&app);
    let targets: Vec<StoreSource> = match store {
        Some(store) => vec![store],
        None => state
            .stores
            .list()
            .await?
            .into_iter()
            .filter(|a| a.connected && a.store == StoreSource::Steam)
            .map(|a| a.store)
            .collect(),
    };
    let mut total = AchievementsSyncSummary::default();
    for store in targets {
        if store != StoreSource::Steam {
            continue;
        }
        let summary = state.stores.sync_achievements(store, &progress).await?;
        total.games += summary.games;
        total.achievements += summary.achievements;
        total.newly_unlocked += summary.newly_unlocked;
    }
    Ok(total)
}

#[tauri::command]
pub async fn cove_achievements_list(state: State<'_, AppState>) -> AppResult<Vec<CoveAchievement>> {
    state.cove_achievements.list().await
}
