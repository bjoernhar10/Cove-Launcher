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
    LaunchVia, LibraryItem, LibraryMatchEvent, LibraryUpdate, LocalStoreGame, ManualGameInput,
    MatchCandidate, Platform, PlayStats, PlaytimeStats, PluginInfo, Recommendation, RunningGame,
    ScanCandidate, StoreAccount, StoreSource, SyncPhase, SyncProgress, UserProfile,
};
use crate::models::{
    DirectDownloadInput, FolderInfo, GameUpdateInfo, InstallPreview, InstallTarget, ItchGame,
    ItchStatus, ItchUpload, PluginGameInput, PluginManifestPreview, PluginPermission,
    ServiceStatus, UninstallMethod, UpdateCheck,
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
pub const GAME_MOVE_PROGRESS: &str = "game-move-progress";
pub const PROTON_PROGRESS: &str = "proton-progress";

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
        return Err(AppError::InvalidInput(tr!(
            "Datei nicht gefunden: {path}",
            "File not found: {path}"
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
    db::library::remove(&state.pool, &game_id).await?;
    // A shortcut to a game that isn't in the library would only fail.
    state.shortcuts.remove_all(&game_id).await;
    Ok(())
}

/// A desktop shortcut asked for a game (`--launch <id>`): start it like the
/// play button does. When that fails, Cove comes to the front and says why.
pub fn launch_from_shortcut<R: Runtime>(app: AppHandle<R>, game_id: String) {
    use tauri::Manager;
    use tauri_plugin_notification::NotificationExt;
    tauri::async_runtime::spawn(async move {
        let Some(state) = app.try_state::<AppState>() else {
            return;
        };
        if let Err(error) = launch_game(&state, &game_id).await {
            eprintln!("[shortcuts] {game_id}: {error}");
            if let Some(window) = app.get_webview_window("main") {
                let _ = window.unminimize();
                let _ = window.show();
                let _ = window.set_focus();
            }
            let title = db::library::get(&state.pool, &game_id)
                .await
                .ok()
                .flatten()
                .map_or_else(|| tr!("Spiel", "Game"), |item| item.game.title);
            let _ = app
                .notification()
                .builder()
                .title(tr!("{title} startet nicht", "{title} doesn't start"))
                .body(error.user_message())
                .show();
        }
    });
}

/// "Exportieren": the user picks where the file goes.
#[tauri::command]
pub async fn data_export<R: Runtime>(
    app: AppHandle<R>,
    state: State<'_, AppState>,
) -> AppResult<Option<String>> {
    use tauri_plugin_dialog::DialogExt;
    let (tx, rx) = tokio::sync::oneshot::channel();
    let name = format!("cove-export-{}.json", &crate::models::now_iso()[..10]);
    let mut dialog = app
        .dialog()
        .file()
        .set_file_name(name)
        .add_filter("Cove", &["json"]);
    if let Ok(dir) = app.path().document_dir() {
        dialog = dialog.set_directory(dir);
    }
    dialog.save_file(move |picked| {
        let _ = tx.send(picked);
    });
    let Some(path) = rx.await.ok().flatten().and_then(|p| p.into_path().ok()) else {
        return Ok(None);
    };
    let settings = db::settings::load(&state.pool, &state.defaults).await?;
    let file = crate::transfer::export(&state.pool, &settings).await?;
    crate::transfer::write(&path, &file).await?;
    Ok(Some(path.to_string_lossy().into_owned()))
}

/// "Importieren": the user picks an export; Cove says what's in it.
#[tauri::command]
pub async fn data_import_pick<R: Runtime>(
    app: AppHandle<R>,
) -> AppResult<Option<crate::models::ImportPreview>> {
    use tauri_plugin_dialog::DialogExt;
    let (tx, rx) = tokio::sync::oneshot::channel();
    app.dialog()
        .file()
        .add_filter("Cove", &["json"])
        .pick_file(move |picked| {
            let _ = tx.send(picked);
        });
    let Some(path) = rx.await.ok().flatten().and_then(|p| p.into_path().ok()) else {
        return Ok(None);
    };
    let file = crate::transfer::read(&path).await?;
    Ok(Some(crate::transfer::preview(&path, &file)))
}

#[tauri::command]
pub async fn data_import(
    state: State<'_, AppState>,
    path: String,
    settings: bool,
) -> AppResult<crate::models::ImportResult> {
    let file = crate::transfer::read(Path::new(&path)).await?;
    let local = db::settings::load(&state.pool, &state.defaults).await?;
    let result = crate::transfer::import(&state.pool, &file, &local, settings).await?;
    if result.settings {
        let loaded = db::settings::load(&state.pool, &state.defaults).await?;
        settings_changed(&state, &local, &loaded).await;
    }
    Ok(result)
}

/// Mod changes write into the game folder: never while the game runs.
async fn mod_game(state: &AppState, game_id: &str) -> AppResult<LibraryItem> {
    if state
        .launcher
        .running()
        .iter()
        .any(|r| r.game_id == game_id)
    {
        return Err(AppError::InvalidInput(tr!(
            "Beende das Spiel, bevor du Mods änderst.",
            "Quit the game before you change mods."
        )));
    }
    db::library::require(&state.pool, game_id).await
}

#[tauri::command]
pub async fn mods_list(
    state: State<'_, AppState>,
    game_id: String,
) -> AppResult<Vec<crate::models::ModInfo>> {
    state.mods.list(&game_id).await
}

/// A file (archive or single mod file) or a folder, picked by the user.
#[tauri::command]
pub async fn mods_pick<R: Runtime>(app: AppHandle<R>, folder: bool) -> AppResult<Option<String>> {
    use tauri_plugin_dialog::DialogExt;
    let (tx, rx) = tokio::sync::oneshot::channel();
    let mut dialog = app.dialog().file();
    if let Ok(dir) = app.path().download_dir() {
        dialog = dialog.set_directory(dir);
    }
    let send = move |picked| {
        let _ = tx.send(picked);
    };
    if folder {
        dialog.pick_folder(send);
    } else {
        dialog.pick_file(send);
    }
    Ok(rx
        .await
        .ok()
        .flatten()
        .and_then(|p| p.into_path().ok())
        .map(|p| p.to_string_lossy().into_owned()))
}

#[tauri::command]
pub async fn mods_add(
    state: State<'_, AppState>,
    game_id: String,
    source: String,
    name: String,
    target: String,
) -> AppResult<Vec<crate::models::ModInfo>> {
    let item = mod_game(&state, &game_id).await?;
    state
        .mods
        .add(&item, Path::new(&source), &name, &target)
        .await
}

#[tauri::command]
pub async fn mods_set_enabled(
    state: State<'_, AppState>,
    game_id: String,
    mod_id: String,
    enabled: bool,
) -> AppResult<Vec<crate::models::ModInfo>> {
    let item = mod_game(&state, &game_id).await?;
    state.mods.set_enabled(&item, &mod_id, enabled).await
}

#[tauri::command]
pub async fn mods_move(
    state: State<'_, AppState>,
    game_id: String,
    mod_id: String,
    position: usize,
) -> AppResult<Vec<crate::models::ModInfo>> {
    let item = mod_game(&state, &game_id).await?;
    state.mods.move_to(&item, &mod_id, position).await
}

#[tauri::command]
pub async fn mods_remove(
    state: State<'_, AppState>,
    game_id: String,
    mod_id: String,
) -> AppResult<Vec<crate::models::ModInfo>> {
    let item = mod_game(&state, &game_id).await?;
    state.mods.remove(&item, &mod_id).await
}

/// Games from Heroic (added by hand) and Lutris.
#[tauri::command]
pub async fn launchers_scan(
    state: State<'_, AppState>,
) -> AppResult<Vec<crate::models::ExternalGame>> {
    crate::external::scan(&state.pool, &state.launcher_dirs).await
}

#[tauri::command]
pub async fn launchers_import(
    state: State<'_, AppState>,
    ids: Vec<String>,
) -> AppResult<Vec<String>> {
    let added = crate::external::add(&state.pool, &state.launcher_dirs, &ids).await?;
    state.cove_achievements.check_in_background();
    Ok(added)
}

/// The game's shortcuts and the places this system has for them.
#[tauri::command]
pub async fn game_shortcuts(
    state: State<'_, AppState>,
    game_id: String,
) -> AppResult<crate::models::GameShortcuts> {
    db::library::require(&state.pool, &game_id).await?;
    Ok(crate::models::GameShortcuts {
        places: state.shortcuts.places(),
        shortcuts: state.shortcuts.list(&game_id).await?,
    })
}

/// A shortcut that starts the game through Cove, with the cover as its icon.
#[tauri::command]
pub async fn game_create_shortcut(
    state: State<'_, AppState>,
    game_id: String,
    place: crate::models::ShortcutPlace,
) -> AppResult<crate::models::GameShortcut> {
    let item = db::library::require(&state.pool, &game_id).await?;
    let cover = match item.game.artwork.cover.url.as_deref() {
        Some(url) => state
            .catalog
            .images
            .get(url)
            .await
            .ok()
            .map(|(bytes, _)| bytes),
        None => None,
    };
    state.shortcuts.create(&item, place, cover).await
}

#[tauri::command]
pub async fn game_remove_shortcut(
    state: State<'_, AppState>,
    game_id: String,
    place: crate::models::ShortcutPlace,
) -> AppResult<()> {
    state.shortcuts.remove(&game_id, place).await
}

#[tauri::command]
pub async fn library_update(
    state: State<'_, AppState>,
    game_id: String,
    patch: LibraryUpdate,
) -> AppResult<LibraryItem> {
    if let Some(path) = patch.executable_path.as_deref().map(str::trim) {
        if !path.is_empty() && !Path::new(path).is_file() {
            return Err(AppError::InvalidInput(tr!(
                "Datei nicht gefunden: {path}",
                "File not found: {path}"
            )));
        }
    }
    let item = db::library::update(&state.pool, &game_id, &patch).await?;
    // A Proton build the user just picked is installed right away (Linux).
    if patch.launch_options.is_some() && state.host.platform == Platform::Linux {
        let settings = db::settings::load(&state.pool, &state.defaults).await?;
        if let Some(id) =
            crate::proton::wanted(&item.entry.launch_options, &settings.library.default_proton)
        {
            state.proton.prepare(&id);
        }
    }
    Ok(item)
}

#[tauri::command]
pub async fn collections_list(
    state: State<'_, AppState>,
) -> AppResult<Vec<crate::models::Collection>> {
    db::collections::list(&state.pool).await
}

#[tauri::command]
pub async fn collections_create(
    state: State<'_, AppState>,
    name: String,
) -> AppResult<crate::models::Collection> {
    db::collections::create(&state.pool, &name).await
}

#[tauri::command]
pub async fn collections_rename(
    state: State<'_, AppState>,
    collection_id: String,
    name: String,
) -> AppResult<crate::models::Collection> {
    db::collections::rename(&state.pool, &collection_id, &name).await
}

#[tauri::command]
pub async fn collections_delete(
    state: State<'_, AppState>,
    collection_id: String,
) -> AppResult<()> {
    db::collections::delete(&state.pool, &collection_id).await
}

#[tauri::command]
pub async fn collections_set_game(
    state: State<'_, AppState>,
    collection_id: String,
    game_id: String,
    member: bool,
) -> AppResult<()> {
    db::collections::set_game(&state.pool, &collection_id, &game_id, member).await
}

#[tauri::command]
pub async fn library_scan_folder(
    state: State<'_, AppState>,
    path: String,
) -> AppResult<Vec<ScanCandidate>> {
    let known = db::library::executable_paths(&state.pool).await?;
    tauri::async_runtime::spawn_blocking(move || scanner::scan(Path::new(path.trim()), &known))
        .await
        .map_err(|e| AppError::InvalidInput(tr!("Scan abgebrochen: {e}", "Scan aborted: {e}")))?
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

pub const PRICE_ALERT: &str = "price-alert";

/// Checks the wishlist's prices and announces alerts (in the app and as a
/// desktop notification). `forced` checks even with alerts switched off.
pub async fn check_prices<R: tauri::Runtime>(
    app: &tauri::AppHandle<R>,
    state: &AppState,
    forced: bool,
) -> Option<Vec<crate::models::WishlistItem>> {
    use tauri_plugin_notification::NotificationExt;
    let settings = match db::settings::load(&state.pool, &state.defaults).await {
        Ok(settings) => settings,
        Err(error) => {
            eprintln!("[prices] {error}");
            return None;
        }
    };
    if !forced && !settings.wishlist.price_alerts {
        return None;
    }
    match state.prices.check(&settings.wishlist.country).await {
        Ok((items, alerts)) => {
            for alert in alerts {
                let price = format!(
                    "{:.2} {}",
                    alert.deal.price.amount, alert.deal.price.currency
                );
                let body = match alert.reason {
                    crate::models::AlertReason::Target => tr!(
                        "Jetzt {price} bei {}: dein Wunschpreis ist erreicht.",
                        "Now {price} at {}: your target price is reached.",
                        alert.deal.shop
                    ),
                    crate::models::AlertReason::Sale => tr!(
                        "−{} % bei {}: jetzt {price}.",
                        "−{} % at {}: now {price}.",
                        alert.deal.cut,
                        alert.deal.shop
                    ),
                };
                let _ = app
                    .notification()
                    .builder()
                    .title(&alert.title)
                    .body(&body)
                    .show();
                let _ = app.emit(PRICE_ALERT, &alert);
            }
            Some(items)
        }
        Err(error) => {
            eprintln!("[prices] {error}");
            None
        }
    }
}

#[tauri::command]
pub async fn wishlist_list(
    state: State<'_, AppState>,
) -> AppResult<Vec<crate::models::WishlistItem>> {
    db::wishlist::list(&state.pool).await
}

/// Adds a catalogue game with its prices (when they arrive within a few seconds).
#[tauri::command]
pub async fn wishlist_add(
    state: State<'_, AppState>,
    game_id: String,
) -> AppResult<crate::models::WishlistItem> {
    let game = state.catalog.details(&game_id).await?.game;
    db::wishlist::add(&state.pool, &game).await?;
    let settings = db::settings::load(&state.pool, &state.defaults).await?;
    let lookup = state.prices.game_price(&game, &settings.wishlist.country);
    if let Ok(Ok(Some(price))) =
        tokio::time::timeout(std::time::Duration::from_secs(8), lookup).await
    {
        db::wishlist::set_price(&state.pool, &game.id, Some(&price)).await?;
    }
    db::wishlist::get(&state.pool, &game.id)
        .await?
        .ok_or_else(|| AppError::InvalidInput(tr!("Ungültiges Spiel.", "Invalid game.")))
}

#[tauri::command]
pub async fn wishlist_remove(state: State<'_, AppState>, game_id: String) -> AppResult<()> {
    db::wishlist::remove(&state.pool, &game_id).await
}

#[tauri::command]
pub async fn wishlist_update(
    state: State<'_, AppState>,
    game_id: String,
    target_price: Option<f64>,
    notify_on_sale: bool,
) -> AppResult<crate::models::WishlistItem> {
    db::wishlist::update(&state.pool, &game_id, target_price, notify_on_sale).await
}

/// Checks the prices now ("Preise prüfen").
#[tauri::command]
pub async fn wishlist_refresh<R: Runtime>(
    app: AppHandle<R>,
    state: State<'_, AppState>,
) -> AppResult<Vec<crate::models::WishlistItem>> {
    match check_prices(&app, &state, true).await {
        Some(items) => Ok(items),
        None => Err(AppError::StoreUnavailable(tr!(
            "Preise sind gerade nicht verfügbar. Versuch es später noch einmal.",
            "Prices aren't available right now. Please try again later."
        ))),
    }
}

/// Shows the newest running game as the Discord status (when switched on), else none.
pub async fn refresh_discord(state: &AppState) {
    use crate::discord::Status;
    let enabled = db::settings::load(&state.pool, &state.defaults)
        .await
        .is_ok_and(|s| s.general.discord_status);
    let running = state.launcher.running();
    let status = match running.last().filter(|_| enabled) {
        Some(game) => match db::library::require(&state.pool, &game.game_id).await {
            Ok(item) => Status::Playing {
                title: item.game.title,
                image: item.game.artwork.cover.url,
                started_at: chrono::DateTime::parse_from_rfc3339(&game.started_at)
                    .map_or_else(|_| chrono::Utc::now().timestamp(), |t| t.timestamp()),
            },
            Err(_) => Status::Clear,
        },
        None => Status::Clear,
    };
    state.discord.set(status);
}

/// "Speicherplatz": measures the installed games and the drives they are on.
#[tauri::command]
pub async fn storage_overview(
    state: State<'_, AppState>,
) -> AppResult<crate::models::StorageOverview> {
    let settings = db::settings::load(&state.pool, &state.defaults).await?;
    crate::storage::overview(&state.pool, &settings.library.game_folders).await
}

/// A game's announcements and patch notes (Steam's news).
#[tauri::command]
pub async fn game_news(
    state: State<'_, AppState>,
    game_id: String,
) -> AppResult<Vec<crate::models::NewsItem>> {
    let item = db::library::require(&state.pool, &game_id).await?;
    state
        .news
        .for_game(
            &state.protondb,
            item.entry.source,
            item.entry.store_game_id.as_deref(),
            &item.game.title,
        )
        .await
}

/// Home: recent posts of the games played last.
#[tauri::command]
pub async fn news_recent(state: State<'_, AppState>) -> AppResult<Vec<crate::models::GameNews>> {
    let library = db::library::list(&state.pool).await?;
    Ok(state.news.feed(&state.protondb, &library).await)
}

/// Games free to keep now and soon (Epic, GOG, other shops), without those in the library.
#[tauri::command]
pub async fn free_games_list(
    state: State<'_, AppState>,
) -> AppResult<Vec<crate::models::FreeGame>> {
    let settings = db::settings::load(&state.pool, &state.defaults).await?;
    state
        .free_games
        .list(
            &settings.wishlist.country,
            &settings.general.language,
            false,
        )
        .await
}

/// Announces free games that are new since the last check (setting `wishlist.freeGames`).
pub async fn check_free_games<R: tauri::Runtime>(app: &tauri::AppHandle<R>, state: &AppState) {
    use tauri_plugin_notification::NotificationExt;
    let Ok(settings) = db::settings::load(&state.pool, &state.defaults).await else {
        return;
    };
    if !settings.wishlist.free_games {
        return;
    }
    let language = &settings.general.language;
    let fresh = match state
        .free_games
        .list(&settings.wishlist.country, language, true)
        .await
    {
        Ok(games) => state.free_games.new_ones(&games).await,
        Err(error) => Err(error),
    };
    match fresh {
        // Many at once (the first check, a big bundle day): one summary instead.
        Ok(games) if games.len() > 3 => {
            let shops: std::collections::BTreeSet<&str> =
                games.iter().map(|g| g.shop.as_str()).collect();
            let shops = shops.into_iter().collect::<Vec<_>>().join(", ");
            let _ = app
                .notification()
                .builder()
                .title(tr!(
                    "{} Spiele gerade gratis",
                    "{} games free right now",
                    games.len()
                ))
                .body(tr!(
                    "Bei {shops}. Du findest sie auf der Startseite.",
                    "On {shops}. You'll find them on Home."
                ))
                .show();
        }
        Ok(games) => {
            for game in games {
                let store = &game.shop;
                let until = game
                    .ends_at
                    .as_deref()
                    .and_then(|t| chrono::DateTime::parse_from_rfc3339(t).ok())
                    .map(|t| t.with_timezone(&chrono::Local));
                let body = if let Some(t) = until {
                    tr!(
                        "Gratis bei {store}, bis zum {}.",
                        "Free on {store} until {}.",
                        if language == "de" {
                            t.format("%-d.%-m.").to_string()
                        } else {
                            t.format("%B %-d").to_string()
                        }
                    )
                } else {
                    tr!("Gerade gratis bei {store}.", "Free on {store} right now.")
                };
                let _ = app
                    .notification()
                    .builder()
                    .title(&game.title)
                    .body(&body)
                    .show();
            }
        }
        Err(error) => eprintln!("[free-games] {error}"),
    }
}

/// Prices of a catalogue game (catalogue page).
#[tauri::command]
pub async fn game_price(
    state: State<'_, AppState>,
    game_id: String,
) -> AppResult<Option<crate::models::GamePrice>> {
    let game = state.catalog.details(&game_id).await?.game;
    let settings = db::settings::load(&state.pool, &state.defaults).await?;
    state
        .prices
        .game_price(&game, &settings.wishlist.country)
        .await
}

/// Connected controllers.
#[tauri::command]
pub fn gamepad_list(state: State<'_, AppState>) -> Vec<crate::models::GamepadInfo> {
    state.gamepads.list()
}

/// True on the Steam Deck's gaming mode and in gamescope sessions (start in the TV mode).
#[tauri::command]
pub fn system_tv_hint() -> bool {
    crate::gamepad::tv_hint()
}

/// The TV mode fills the screen.
#[tauri::command]
pub fn window_set_fullscreen<R: Runtime>(app: AppHandle<R>, fullscreen: bool) -> AppResult<()> {
    if let Some(window) = app.get_webview_window("main") {
        window.set_fullscreen(fullscreen).map_err(|e| {
            eprintln!("[cove] Vollbild: {e}");
            AppError::InvalidInput(tr!(
                "Der Vollbildmodus ließ sich nicht umschalten.",
                "Full screen couldn't be switched."
            ))
        })?;
    }
    Ok(())
}

/// The statistics page: a year (None = all time).
#[tauri::command]
pub async fn stats_overview(state: State<'_, AppState>, year: Option<i32>) -> AppResult<PlayStats> {
    db::stats::overview(&state.pool, year, chrono::Utc::now().date_naive()).await
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
    launch_game(&state, &game_id).await
}

/// Starts a library game (the play button and desktop shortcuts).
pub async fn launch_game(state: &AppState, game_id: &str) -> AppResult<LaunchResult> {
    let item = db::library::require(&state.pool, game_id).await?;
    if item.entry.launch_via == Some(LaunchVia::Steam) {
        steam_in_background(state).await?;
    }
    let mut host = launch_host(state, &item).await?;
    if host.platform == Platform::Linux && item.entry.launch_options.tweaks.fps_log {
        host.fps_log_dir = state.fps.prepare(game_id);
    }
    if item.entry.source == StoreSource::Gog && item.entry.installed_by_cove {
        state.downloads.gog_saves_before_launch(&item, &host).await;
    }
    if item.entry.source == StoreSource::Epic && item.entry.installed_by_cove {
        let args = state.downloads.epic_launch_args(&item, &host).await?;
        return state.launcher.launch_with(&item, &host, args, Vec::new());
    }
    if item.entry.source == StoreSource::Amazon && item.entry.installed_by_cove {
        let env = state.downloads.amazon_launch_env(&item, &host).await?;
        return state.launcher.launch_with(&item, &host, Vec::new(), env);
    }
    state.launcher.launch(&item, &host)
}

/// The host for a launch. A Windows game on Linux gets its Proton build
/// first: a GE-Proton build that is missing is downloaded before it starts.
async fn launch_host(state: &AppState, item: &LibraryItem) -> AppResult<crate::launcher::HostEnv> {
    let host = state.proton.host(&state.host).await?;
    let windows_exe = item.entry.executable_path.as_deref().is_some_and(|p| {
        Path::new(p)
            .extension()
            .is_some_and(|e| e.eq_ignore_ascii_case("exe"))
    });
    if host.platform != Platform::Linux || !windows_exe {
        return Ok(host);
    }
    let Some(id) = crate::proton::wanted(&item.entry.launch_options, &host.default_proton) else {
        return Ok(host);
    };
    state.proton.ensure(&id).await?;
    let mut host = state.proton.host(&state.host).await?;
    if let Some(program) = state.proton.ensure_umu().await? {
        // protonfixes are keyed by the Steam app id; don't hold the start up for long.
        let app_id = tokio::time::timeout(
            std::time::Duration::from_secs(5),
            state.protondb.steam_app_id(
                item.entry.source,
                item.entry.store_game_id.as_deref(),
                &item.game.title,
            ),
        )
        .await
        .ok()
        .flatten();
        host.umu = Some(crate::umu::UmuLaunch::new(
            program,
            item.entry.source,
            app_id.as_deref(),
        ));
    }
    Ok(host)
}

/// Proton builds: GE-Proton releases, Cove's installs and Steam's builds.
#[tauri::command]
pub async fn proton_list(state: State<'_, AppState>) -> AppResult<crate::models::ProtonOverview> {
    state.proton.overview(false).await
}

/// Loads the release list again and installs a new "latest" in the background.
#[tauri::command]
pub async fn proton_check(state: State<'_, AppState>) -> AppResult<crate::models::ProtonOverview> {
    let overview = state.proton.overview(true).await?;
    let proton = state.proton.clone();
    let may_clean = state.launcher.running().is_empty();
    tauri::async_runtime::spawn(async move {
        if let Err(error) = proton.check_updates(false, may_clean).await {
            eprintln!("[cove] Proton-Update: {error}");
        }
    });
    Ok(overview)
}

/// Starts installing a GE-Proton build (progress: `proton-progress`).
#[tauri::command]
pub async fn proton_install(state: State<'_, AppState>, version_id: String) -> AppResult<()> {
    if version_id == crate::proton::LATEST {
        state.proton.prepare(&version_id);
        return Ok(());
    }
    state.proton.start_install(&version_id, false).map(|_| ())
}

/// Deletes a GE-Proton build no game uses.
#[tauri::command]
pub async fn proton_remove(state: State<'_, AppState>, version_id: String) -> AppResult<()> {
    state.proton.remove(&version_id).await
}

/// Save backups of a game: known to the manifest, files found now, backups.
#[tauri::command]
pub async fn game_backups(
    state: State<'_, AppState>,
    game_id: String,
) -> AppResult<crate::models::GameBackups> {
    state.backups.overview(&game_id, &state.host).await
}

/// Backs a game's saves up now; null when nothing changed since the last backup.
#[tauri::command]
pub async fn game_backup_now(
    state: State<'_, AppState>,
    game_id: String,
) -> AppResult<Option<crate::models::BackupInfo>> {
    state.backups.back_up(&game_id, &state.host).await
}

/// Puts a backup back (refused while the game runs); returns the files restored.
#[tauri::command]
pub async fn game_backup_restore(
    state: State<'_, AppState>,
    game_id: String,
    backup_id: String,
) -> AppResult<i64> {
    if state
        .launcher
        .running()
        .iter()
        .any(|r| r.game_id == game_id)
    {
        return Err(AppError::InvalidInput(tr!(
            "Beende das Spiel, bevor du einen Spielstand zurückholst.",
            "Quit the game before you restore a save."
        )));
    }
    let count = state
        .backups
        .restore(&game_id, &backup_id, &state.host)
        .await?;
    Ok(i64::try_from(count).unwrap_or(i64::MAX))
}

/// The systems Cove plays ROMs of, with their emulators (installed or not).
#[tauri::command]
pub async fn emulators_systems(
    state: State<'_, AppState>,
) -> AppResult<Vec<crate::models::SystemInfo>> {
    let env = crate::emulators::Env::detect(state.host.home.clone());
    tokio::task::spawn_blocking(move || crate::emulators::systems(&env))
        .await
        .map_err(|e| AppError::Launch(e.to_string()))
}

/// Adds the ROMs of every ROM folder to the library (a collection per system)
/// and matches the new games with the catalogue in the background.
#[tauri::command]
pub async fn emulators_scan<R: Runtime>(
    app: AppHandle<R>,
    state: State<'_, AppState>,
) -> AppResult<crate::models::RomScanResult> {
    let settings = db::settings::load(&state.pool, &state.defaults).await?;
    let folders = settings.emulators.rom_folders;
    let found = tokio::task::spawn_blocking(move || {
        folders
            .into_iter()
            .map(|f| {
                let roms = crate::emulators::scan(&f);
                (f, roms)
            })
            .collect::<Vec<_>>()
    })
    .await
    .map_err(|e| AppError::Launch(e.to_string()))?;
    let mut collections = db::collections::list(&state.pool).await?;
    let mut added = Vec::new();
    let mut total = 0i64;
    for (folder, roms) in found {
        let Some(system) = crate::emulators::system(&folder.system) else {
            continue;
        };
        for rom in roms {
            total += 1;
            let input = ManualGameInput {
                title: rom.title.clone(),
                executable_path: rom.path.to_string_lossy().into_owned(),
                size_bytes: i64::try_from(rom.size).ok(),
            };
            let item = match db::library::add_manual(&state.pool, &input).await {
                Ok(item) => item,
                Err(AppError::AlreadyInLibrary(_)) => continue,
                Err(error) => return Err(error),
            };
            let existing = collections
                .iter()
                .find(|c| c.name.to_lowercase() == system.name.to_lowercase())
                .map(|c| c.id.clone());
            let collection = if let Some(id) = existing {
                id
            } else {
                let created = db::collections::create(&state.pool, system.name).await?;
                let id = created.id.clone();
                collections.push(created);
                id
            };
            db::collections::set_game(&state.pool, &collection, &item.game.id, true).await?;
            added.push(item.game.id);
        }
    }
    let count = i64::try_from(added.len()).unwrap_or(i64::MAX);
    if !added.is_empty() {
        let catalog = state.catalog.clone();
        let app = app.clone();
        tauri::async_runtime::spawn(async move {
            for (i, game_id) in added.into_iter().enumerate() {
                if i > 0 {
                    tokio::time::sleep(MATCH_PACING).await;
                }
                match catalog.match_library_game(&game_id).await {
                    Ok(status) => {
                        let _ =
                            app.emit(LIBRARY_MATCH_CHANGED, LibraryMatchEvent { game_id, status });
                    }
                    Err(error) => eprintln!("[cove] Katalog-Zuordnung für {game_id}: {error}"),
                }
            }
        });
        state.cove_achievements.check_in_background();
    }
    Ok(crate::models::RomScanResult {
        found: total,
        added: count,
    })
}

/// Linux: which game tools (`mangohud`, `gamemoderun`, `gamescope`) are installed.
#[tauri::command]
pub fn system_game_tools() -> crate::models::GameTools {
    crate::launcher::ToolPaths::detect().installed()
}

/// Stops a running install (a GE-Proton build or umu-launcher).
#[tauri::command]
pub fn proton_cancel(state: State<'_, AppState>, version_id: String) -> AppResult<()> {
    state.proton.cancel(&version_id)
}

/// The build id a game starts with (its own choice or the default).
fn proton_choice(item: &LibraryItem, host: &crate::launcher::HostEnv) -> String {
    crate::proton::wanted(&item.entry.launch_options, &host.default_proton)
        .unwrap_or_else(|| host.default_proton.clone())
}

/// Prefix tools work on Cove's own prefixes only.
fn foreign_prefix(item: &LibraryItem) -> AppResult<()> {
    if item.entry.launch_options.wine_prefix.is_some() {
        return Err(AppError::InvalidInput(tr!(
            "Dieses Spiel nutzt einen eigenen Wine-Prefix (aus Heroic oder Lutris). Verwalte ihn dort.",
            "This game uses its own Wine prefix (from Heroic or Lutris). Manage it there."
        )));
    }
    Ok(())
}

/// A game's Proton prefix: does it exist, how big is it, is winetricks there.
#[tauri::command]
pub async fn game_prefix_info(
    state: State<'_, AppState>,
    game_id: String,
) -> AppResult<crate::models::PrefixInfo> {
    let item = db::library::require(&state.pool, &game_id).await?;
    foreign_prefix(&item)?;
    let root = state.host.compat_data_root.clone();
    let host = state.proton.host(&state.host).await?;
    let proton = crate::proton::resolve(&host.protons, &proton_choice(&item, &host))
        .map(|build| build.path.clone());
    tokio::task::spawn_blocking(move || crate::prefix::info(&root, &game_id, proton.as_deref()))
        .await
        .map_err(|e| AppError::Launch(e.to_string()))?
}

/// Opens, resets or runs winetricks on a game's Proton prefix.
#[tauri::command]
pub async fn game_prefix_action<R: Runtime>(
    app: AppHandle<R>,
    state: State<'_, AppState>,
    game_id: String,
    action: crate::models::PrefixAction,
) -> AppResult<()> {
    use crate::models::PrefixAction;
    let item = db::library::require(&state.pool, &game_id).await?;
    // Never reset or change a prefix that belongs to Heroic or Lutris.
    foreign_prefix(&item)?;
    let root = state.host.compat_data_root.clone();
    match action {
        PrefixAction::Open => {
            use tauri_plugin_opener::OpenerExt;
            let folder = crate::prefix::folder(&root, &item.game.id)?;
            app.opener()
                .open_path(folder.to_string_lossy(), None::<&str>)
                .map_err(|e| {
                    AppError::Launch(tr!(
                        "Der Ordner konnte nicht geöffnet werden: {e}",
                        "The folder could not be opened: {e}"
                    ))
                })
        }
        PrefixAction::Reset => {
            if state
                .launcher
                .running()
                .iter()
                .any(|r| r.game_id == item.game.id)
            {
                return Err(AppError::InvalidInput(tr!(
                    "Beende das Spiel, bevor du den Prefix zurücksetzt.",
                    "Quit the game before you reset its prefix."
                )));
            }
            tokio::task::spawn_blocking(move || crate::prefix::reset(&root, &item.game.id))
                .await
                .map_err(|e| AppError::Launch(e.to_string()))?
        }
        PrefixAction::Winetricks => {
            let host = state.proton.host(&state.host).await?;
            let wanted = proton_choice(&item, &host);
            let build = crate::proton::resolve(&host.protons, &wanted).ok_or_else(|| {
                AppError::CompatToolNotFound(crate::proton::display_name(&wanted).to_owned())
            })?;
            crate::prefix::winetricks(&root, &item.game.id, &build.path)
        }
    }
}

/// Starts Steam silently (tray only) first, when the user wants that.
async fn steam_in_background(state: &AppState) -> AppResult<()> {
    let settings = db::settings::load(&state.pool, &state.defaults).await?;
    if settings.library.steam_silent_start {
        crate::steam::ensure_running(state.host.home.clone()).await?;
    }
    Ok(())
}

/// DLCs of a Steam game; which ones Steam installs is changed in Steam
/// (`game_manage_dlcs`).
#[tauri::command]
pub async fn game_dlcs(
    state: State<'_, AppState>,
    game_id: String,
) -> AppResult<crate::models::StoreDlcList> {
    let item = db::library::require(&state.pool, &game_id).await?;
    let store = item.entry.source;
    let Some(store_game_id) = item.entry.store_game_id.clone() else {
        return Err(no_dlc_list());
    };
    match store {
        StoreSource::Steam => state.stores.steam_dlcs(&store_game_id).await,
        StoreSource::Amazon | StoreSource::Manual => Err(no_dlc_list()),
        StoreSource::Gog | StoreSource::Epic => {
            let owned = state
                .stores
                .owned_dlcs(
                    store,
                    &store_game_id,
                    crate::downloads::gog::host_platform(),
                )
                .await?;
            let excluded = db::store_installs::excluded_dlcs(&state.pool, &game_id).await?;
            let saved: std::collections::HashSet<String> =
                db::store_installs::dlcs(&state.pool, &game_id)
                    .await?
                    .into_iter()
                    .map(|d| d.app_name)
                    .collect();
            let game_installed = item.entry.installed && item.entry.installed_by_cove;
            let dlcs: Vec<crate::models::StoreDlc> = owned
                .into_iter()
                .map(|d| crate::models::StoreDlc {
                    // Epic keeps installed DLCs apart; GOG installs every chosen one with the game.
                    installed: game_installed
                        && !d.unlock_only
                        && if store == StoreSource::Epic {
                            saved.contains(&d.id)
                        } else {
                            !excluded.contains(&d.id)
                        },
                    id: d.id,
                    title: d.title,
                    unlock_only: d.unlock_only,
                })
                .collect();
            let total = i64::try_from(dlcs.len()).unwrap_or(i64::MAX);
            Ok(crate::models::StoreDlcList { dlcs, total })
        }
    }
}

fn no_dlc_list() -> AppError {
    AppError::InvalidInput(tr!(
        "Die DLC-Liste gibt es nur für Steam-, GOG- und Epic-Spiele.",
        "The DLC list is only available for Steam, GOG and Epic games."
    ))
}

/// Opens the game in Steam, where its DLCs are ticked under Properties → DLC.
#[tauri::command]
pub async fn game_manage_dlcs(state: State<'_, AppState>, game_id: String) -> AppResult<()> {
    let app_id = steam_app(&state, &game_id).await?;
    steam_in_background(&state).await?;
    let id = percent_encoding::utf8_percent_encode(&app_id, percent_encoding::NON_ALPHANUMERIC);
    (state.open_url)(&format!("steam://nav/games/details/{id}")).map_err(|_| {
        AppError::Launch(tr!(
            "Steam konnte nicht geöffnet werden. Ist Steam installiert?",
            "Steam could not be opened. Is Steam installed?"
        ))
    })
}

/// `ProtonDB`'s rating of the game (Linux); null when there is none.
#[tauri::command]
pub async fn game_proton_rating(
    state: State<'_, AppState>,
    game_id: String,
) -> AppResult<Option<crate::models::ProtonRating>> {
    let item = db::library::require(&state.pool, &game_id).await?;
    state
        .protondb
        .rating(
            item.entry.source,
            item.entry.store_game_id.as_deref(),
            &item.game.title,
        )
        .await
}

async fn steam_app(state: &AppState, game_id: &str) -> AppResult<String> {
    let item = db::library::require(&state.pool, game_id).await?;
    match (item.entry.source, item.entry.store_game_id) {
        (StoreSource::Steam, Some(app_id)) => Ok(app_id),
        _ => Err(AppError::InvalidInput(tr!(
            "Die DLC-Liste gibt es nur für Steam-Spiele.",
            "The DLC list is only available for Steam games."
        ))),
    }
}

/// Steam: Steam's uninstall dialog. Games Cove installed: deletes the game folder.
#[tauri::command]
pub async fn game_uninstall<R: Runtime>(
    app: AppHandle<R>,
    state: State<'_, AppState>,
    game_id: String,
) -> AppResult<UninstallMethod> {
    if state
        .launcher
        .running()
        .iter()
        .any(|r| r.game_id == game_id)
    {
        return Err(AppError::AlreadyRunning(game_id));
    }
    let item = db::library::require(&state.pool, &game_id).await?;
    let entry = &item.entry;
    if let (StoreSource::Steam, Some(app_id)) = (entry.source, entry.store_game_id.clone()) {
        steam_in_background(&state).await?;
        let id = percent_encoding::utf8_percent_encode(&app_id, percent_encoding::NON_ALPHANUMERIC);
        (state.open_url)(&format!("steam://uninstall/{id}")).map_err(|_| {
            AppError::Launch(tr!(
                "Steam konnte nicht geöffnet werden. Ist Steam installiert?",
                "Steam could not be opened. Is Steam installed?"
            ))
        })?;
        state.downloads.watch_uninstall(game_id, app_id);
        return Ok(UninstallMethod::Steam);
    }
    let folder = entry
        .install_path
        .clone()
        .filter(|_| entry.installed)
        .ok_or_else(|| {
            AppError::InvalidInput(tr!(
                "Dieses Spiel ist nicht installiert.",
                "This game isn't installed."
            ))
        })?;
    if !entry.installed_by_cove {
        return Err(AppError::InvalidInput(tr!(
            "Cove hat dieses Spiel nicht installiert. Deinstalliere es dort, wo du es installiert hast.",
            "Cove didn't install this game. Uninstall it where you installed it."
        )));
    }
    let path = std::path::PathBuf::from(&folder);
    tokio::task::spawn_blocking(move || match std::fs::remove_dir_all(&path) {
        Err(e) if e.kind() != std::io::ErrorKind::NotFound => Err(e),
        _ => Ok(()),
    })
    .await
    .map_err(|e| AppError::Io(std::io::Error::other(e.to_string())))??;
    db::library::mark_uninstalled(&state.pool, &game_id).await?;
    downloads_changed(&app, &state).await?;
    Ok(UninstallMethod::Removed)
}

/// Moves the game folder into `folder` in the background (`game-move-progress`).
#[tauri::command]
pub async fn game_move(
    state: State<'_, AppState>,
    game_id: String,
    folder: String,
) -> AppResult<()> {
    if state
        .launcher
        .running()
        .iter()
        .any(|r| r.game_id == game_id)
    {
        return Err(AppError::AlreadyRunning(game_id));
    }
    if db::downloads::has_unfinished(&state.pool, &game_id).await? {
        return Err(AppError::InvalidInput(tr!(
            "Für dieses Spiel läuft gerade ein Download. Verschiebe es, wenn er fertig ist.",
            "A download for this game is running. Move it when it has finished."
        )));
    }
    let item = db::library::require(&state.pool, &game_id).await?;
    state.mover.start(&item, &folder)
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
    let before = db::settings::load(&state.pool, &state.defaults).await?;
    db::settings::save(&state.pool, &settings).await?;
    settings_changed(&state, &before, &settings).await;
    db::settings::load(&state.pool, &state.defaults).await
}

/// What has to follow new settings at once.
async fn settings_changed(state: &AppState, before: &AppSettings, settings: &AppSettings) {
    crate::i18n::set_language(&settings.general.language);
    // A new default Proton build is installed right away (Linux).
    if state.host.platform == Platform::Linux
        && before.library.default_proton != settings.library.default_proton
    {
        state.proton.prepare(&settings.library.default_proton);
    }
    // Bandwidth limit and parallel downloads apply at once.
    state.downloads.wake();
    if before.general.discord_status != settings.general.discord_status {
        refresh_discord(state).await;
    }
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
            AppError::Launch(tr!(
                "Der Plugin-Ordner konnte nicht geöffnet werden: {e}",
                "The plugin folder could not be opened: {e}"
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
                return Err(AppError::InvalidInput(tr!(
                    "Datei nicht gefunden: {path}",
                    "File not found: {path}"
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
pub async fn downloads_enqueue(
    state: State<'_, AppState>,
    game_id: String,
    target: Option<InstallTarget>,
) -> AppResult<()> {
    let item = db::library::require(&state.pool, &game_id).await?;
    // Epic without a target: the game stays with the Epic Launcher (anti-cheat …).
    let own_install = matches!(item.entry.source, StoreSource::Gog | StoreSource::Amazon)
        || (item.entry.source == StoreSource::Epic && target.is_some());
    if own_install {
        let target = target.ok_or_else(|| {
            AppError::InvalidInput(tr!(
                "Bitte wähle einen Zielordner.",
                "Please choose a target folder."
            ))
        })?;
        state.downloads.install_store(&game_id, &target).await?;
        return Ok(());
    }
    if item.entry.source == StoreSource::Steam {
        // Only Steam's install dialog shows up, not its main window.
        steam_in_background(&state).await?;
    }
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

/// "Jetzt starten": starts a download outside the schedule's window.
#[tauri::command]
pub async fn downloads_start_now(state: State<'_, AppState>, id: String) -> AppResult<()> {
    state.downloads.start_now(&id).await
}

#[tauri::command]
pub async fn downloads_preview_store(
    state: State<'_, AppState>,
    game_id: String,
) -> AppResult<InstallPreview> {
    state.downloads.preview_store(&game_id).await
}

#[tauri::command]
pub async fn game_update_check(
    state: State<'_, AppState>,
    game_id: String,
) -> AppResult<GameUpdateInfo> {
    state.downloads.check_update(&game_id).await
}

#[tauri::command]
pub async fn game_update(state: State<'_, AppState>, game_id: String) -> AppResult<()> {
    update_game(&state, &game_id, None).await
}

/// "DLCs ändern" for GOG/Epic games Cove installed: new choice, then an update.
#[tauri::command]
pub async fn game_set_dlcs(
    state: State<'_, AppState>,
    game_id: String,
    excluded_dlcs: Vec<String>,
) -> AppResult<()> {
    update_game(&state, &game_id, Some(&excluded_dlcs)).await
}

async fn update_game(
    state: &AppState,
    game_id: &str,
    excluded_dlcs: Option<&[String]>,
) -> AppResult<()> {
    if state
        .launcher
        .running()
        .iter()
        .any(|r| r.game_id == game_id)
    {
        return Err(AppError::AlreadyRunning(game_id.to_owned()));
    }
    if state.mover.is_moving(game_id) {
        return Err(AppError::InvalidInput(tr!(
            "Dieses Spiel wird gerade verschoben. Aktualisiere es danach.",
            "This game is being moved. Update it afterwards."
        )));
    }
    match excluded_dlcs {
        Some(excluded) => state.downloads.change_dlcs(game_id, excluded).await,
        None => state.downloads.update_store(game_id).await,
    }
    .map(|_| ())
}

#[tauri::command]
pub async fn downloads_preview_url(
    state: State<'_, AppState>,
    input: DirectDownloadInput,
) -> AppResult<InstallPreview> {
    state.downloads.preview_url(&input).await
}

#[tauri::command]
pub async fn downloads_add_url(
    state: State<'_, AppState>,
    input: DirectDownloadInput,
    target: InstallTarget,
) -> AppResult<DownloadItem> {
    let id = state.downloads.add_url(&input, &target).await?;
    db::downloads::get(&state.pool, &id).await
}

#[tauri::command]
pub async fn downloads_add_itch(
    state: State<'_, AppState>,
    game_id: i64,
    upload_id: i64,
    download_key_id: Option<i64>,
    title: String,
    target: InstallTarget,
    extras: Vec<i64>,
) -> AppResult<DownloadItem> {
    let id = state
        .downloads
        .add_itch(
            &db::downloads::ItchChoice {
                game_id,
                upload_id,
                download_key_id,
                extras,
            },
            &title,
            &target,
        )
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

/// Native folder picker; None when the user cancels.
#[tauri::command]
pub async fn system_pick_folder<R: Runtime>(
    app: AppHandle<R>,
    default_path: Option<String>,
) -> AppResult<Option<String>> {
    use tauri_plugin_dialog::DialogExt;
    let (tx, rx) = tokio::sync::oneshot::channel();
    let mut dialog = app.dialog().file();
    if let Some(dir) = default_path
        .map(std::path::PathBuf::from)
        .and_then(|p| p.ancestors().find(|a| a.is_dir()).map(Path::to_path_buf))
    {
        dialog = dialog.set_directory(dir);
    }
    dialog.pick_folder(move |picked| {
        let _ = tx.send(picked);
    });
    let picked = rx.await.ok().flatten();
    Ok(picked
        .and_then(|p| p.into_path().ok())
        .map(|p| p.to_string_lossy().into_owned()))
}

/// Free space, write access and the "Program Files" warning for the install dialog.
#[tauri::command]
pub async fn system_folder_info(path: String) -> AppResult<FolderInfo> {
    tokio::task::spawn_blocking(move || crate::folders::info(&path))
        .await
        .map_err(|e| AppError::Io(std::io::Error::other(e.to_string())))?
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
        discord: crate::discord::available(),
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

/// How long games take, per catalogue id (at most 5000 ids per call).
#[tauri::command]
pub async fn catalog_time_to_beat(
    state: State<'_, AppState>,
    catalog_ids: Vec<String>,
) -> AppResult<std::collections::HashMap<String, crate::models::TimeToBeat>> {
    if catalog_ids.len() > 5000 {
        return Err(AppError::InvalidInput(tr!(
            "Zu viele Spiele auf einmal.",
            "Too many games at once."
        )));
    }
    state.catalog.time_to_beat(&catalog_ids).await
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
        return Err(AppError::InvalidInput(tr!(
            "Bitte füge den Anmeldecode ein.",
            "Please paste the sign-in code."
        )));
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

// ───────────── frame rates, streaming ─────────────

/// The game's recorded sessions (frame rates), newest first.
#[tauri::command]
pub async fn game_fps_logs(
    state: State<'_, AppState>,
    game_id: String,
) -> AppResult<Vec<crate::models::FpsLog>> {
    state.fps.list(&game_id).await
}

#[tauri::command]
pub async fn game_fps_logs_clear(state: State<'_, AppState>, game_id: String) -> AppResult<()> {
    state.fps.clear(&game_id).await
}

/// Sunshine on this computer and how many of Cove's games are in its list.
#[tauri::command]
pub async fn sunshine_status(
    state: State<'_, AppState>,
) -> AppResult<crate::models::SunshineStatus> {
    let sunshine = state.sunshine.clone();
    tokio::task::spawn_blocking(move || sunshine.status())
        .await
        .map_err(|e| AppError::Io(std::io::Error::other(e.to_string())))
}

/// Puts every installed game Cove can start into Sunshine's app list.
#[tauri::command]
pub async fn sunshine_sync(state: State<'_, AppState>) -> AppResult<crate::models::SunshineStatus> {
    let mut games = Vec::new();
    for item in db::library::list(&state.pool).await? {
        if !item.entry.installed || item.entry.launch_via.is_none() {
            continue;
        }
        let cover = match item.game.artwork.cover.url.as_deref() {
            Some(url) => state
                .catalog
                .images
                .get(url)
                .await
                .ok()
                .map(|(bytes, _)| bytes),
            None => None,
        };
        games.push(crate::sunshine::StreamGame {
            id: item.game.id,
            title: item.game.title,
            cover,
        });
    }
    let sunshine = state.sunshine.clone();
    tokio::task::spawn_blocking(move || sunshine.sync(&games))
        .await
        .map_err(|e| AppError::Io(std::io::Error::other(e.to_string())))?
}

/// Takes Cove's games out of Sunshine's app list.
#[tauri::command]
pub async fn sunshine_remove(
    state: State<'_, AppState>,
) -> AppResult<crate::models::SunshineStatus> {
    let sunshine = state.sunshine.clone();
    tokio::task::spawn_blocking(move || sunshine.remove())
        .await
        .map_err(|e| AppError::Io(std::io::Error::other(e.to_string())))?
}
