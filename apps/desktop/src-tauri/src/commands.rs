//! Tauri commands. Argument names are camelCase on the JS side
//! (`invoke("library_update", { gameId, patch })`).

use std::path::Path;

use tauri::{AppHandle, Emitter, Runtime, State};

use crate::catalog::CatalogService;
use crate::db;
use crate::error::{AppError, AppResult};
use crate::models::{
    AboutInfo, Achievement, AchievementUnlock, AppSettings, CatalogCredentials, CatalogFilter,
    CatalogGameDetails, CatalogPage, CatalogProviderId, CatalogSort, CatalogStatus,
    ComponentVersion, DownloadItem, DownloadStatus, Game, LaunchResult, LibraryItem,
    LibraryMatchEvent, LibraryUpdate, ManualGameInput, MatchCandidate, Platform, PlaytimeStats,
    PluginInfo, RunningGame, ScanCandidate, StoreAccount, StoreSource, UserProfile,
};
use crate::state::AppState;
use crate::{plugins, scanner};

pub const DOWNLOADS_CHANGED: &str = "downloads-changed";
pub const LIBRARY_MATCH_CHANGED: &str = "library-match-changed";

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
    db::library::add_manual(&state.pool, &input).await
}

/// Adds a catalogue game (metadata comes from the catalogue, which is still mocked).
#[tauri::command]
pub async fn library_add_from_catalog(
    state: State<'_, AppState>,
    game: Game,
) -> AppResult<LibraryItem> {
    db::library::add_from_catalog(&state.pool, &game).await
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
    db::settings::load(&state.pool, &state.defaults).await
}

#[tauri::command]
pub async fn sources_list(state: State<'_, AppState>) -> AppResult<Vec<StoreAccount>> {
    db::sources::list(&state.pool).await
}

/// Store accounts are connected by the C# connector service (next step).
#[tauri::command]
pub fn sources_connect(store: StoreSource) -> AppResult<StoreAccount> {
    Err(AppError::NotSupported(format!(
        "Die Verbindung zu {} kommt mit dem Connector-Service.",
        match store {
            StoreSource::Steam => "Steam",
            StoreSource::Gog => "GOG",
            StoreSource::Epic => "Epic",
            StoreSource::Manual => "Manuell",
        }
    )))
}

#[tauri::command]
pub async fn plugins_list(state: State<'_, AppState>) -> AppResult<Vec<PluginInfo>> {
    plugins::list(&state.pool, &state.plugin_dirs).await
}

#[tauri::command]
pub async fn plugins_set_enabled(
    state: State<'_, AppState>,
    id: String,
    enabled: bool,
) -> AppResult<PluginInfo> {
    plugins::set_enabled(&state.pool, &state.plugin_dirs, &id, enabled).await
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

#[tauri::command]
pub async fn downloads_enqueue<R: Runtime>(
    app: AppHandle<R>,
    state: State<'_, AppState>,
    game_id: String,
) -> AppResult<()> {
    db::downloads::enqueue(&state.pool, &game_id).await?;
    downloads_changed(&app, &state).await
}

#[tauri::command]
pub async fn downloads_set_status<R: Runtime>(
    app: AppHandle<R>,
    state: State<'_, AppState>,
    id: String,
    status: DownloadStatus,
) -> AppResult<()> {
    db::downloads::set_status(&state.pool, &id, status).await?;
    downloads_changed(&app, &state).await
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
pub fn system_about() -> AboutInfo {
    let version = env!("CARGO_PKG_VERSION").to_owned();
    AboutInfo {
        app_version: version.clone(),
        components: vec![
            ComponentVersion {
                name: "core".into(),
                version,
            },
            ComponentVersion {
                name: "connectors".into(),
                version: "nicht gestartet".into(),
            },
            ComponentVersion {
                name: "recommender".into(),
                version: "nicht gestartet".into(),
            },
        ],
        platform: Platform::current(),
    }
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
