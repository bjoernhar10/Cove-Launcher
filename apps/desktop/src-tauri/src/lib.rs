//! Cove core: owns the game library, launches games and supervises the
//! sidecar services (connectors, recommender) over JSON-RPC/stdio.

// First, so `tr!` is in scope for every other module.
#[macro_use]
mod i18n;
mod auth;
mod backups;
mod boost;
mod catalog;
mod commands;
mod cove_achievements;
mod db;
mod discord;
mod downloads;
mod emulators;
mod error;
mod external;
mod flatpak;
mod folders;
mod fps;
mod free_games;
mod gamepad;
mod launcher;
mod models;
mod mods;
mod mover;
mod news;
mod plugins;
mod prefix;
mod prices;
mod proton;
mod protondb;
mod recommend;
mod savesync;
mod scanner;
mod shortcuts;
mod sidecar;
mod state;
mod steam;
mod storage;
mod stores;
mod sunshine;
mod transfer;
mod umu;
mod updates;

#[cfg(test)]
mod contract_tests;
#[cfg(test)]
mod ipc_tests;

use std::sync::Arc;

use tauri::{Emitter, Manager};
use tauri_plugin_opener::OpenerExt;

use crate::error::AppError;
use crate::launcher::{HostEnv, Launcher, Timing};
use crate::state::{default_settings, AppState};

/// All commands, shared by the app and the IPC tests.
macro_rules! cove_handlers {
    () => {
        tauri::generate_handler![
            $crate::commands::library_list,
            $crate::commands::library_get,
            $crate::commands::library_add_manual,
            $crate::commands::library_add_from_catalog,
            $crate::commands::library_remove,
            $crate::commands::library_update,
            $crate::commands::library_scan_folder,
            $crate::commands::collections_list,
            $crate::commands::collections_create,
            $crate::commands::collections_rename,
            $crate::commands::collections_delete,
            $crate::commands::collections_set_game,
            $crate::commands::proton_list,
            $crate::commands::proton_check,
            $crate::commands::proton_install,
            $crate::commands::proton_remove,
            $crate::commands::proton_cancel,
            $crate::commands::game_backups,
            $crate::commands::emulators_systems,
            $crate::commands::emulators_scan,
            $crate::commands::game_backup_now,
            $crate::commands::game_backup_restore,
            $crate::commands::system_game_tools,
            $crate::commands::game_prefix_info,
            $crate::commands::game_prefix_action,
            $crate::commands::catalog_time_to_beat,
            $crate::commands::library_achievements,
            $crate::commands::library_playtime,
            $crate::commands::stats_overview,
            $crate::commands::gamepad_list,
            $crate::commands::system_tv_hint,
            $crate::commands::window_set_fullscreen,
            $crate::commands::wishlist_list,
            $crate::commands::wishlist_add,
            $crate::commands::wishlist_remove,
            $crate::commands::wishlist_update,
            $crate::commands::wishlist_refresh,
            $crate::commands::game_price,
            $crate::commands::achievements_recent,
            $crate::commands::game_uninstall,
            $crate::commands::game_dlcs,
            $crate::commands::game_manage_dlcs,
            $crate::commands::game_proton_rating,
            $crate::commands::game_move,
            $crate::commands::downloads_preview_url,
            $crate::commands::downloads_preview_store,
            $crate::commands::game_update_check,
            $crate::commands::game_update,
            $crate::commands::game_set_dlcs,
            $crate::commands::system_pick_folder,
            $crate::commands::system_folder_info,
            $crate::commands::game_launch,
            $crate::commands::game_stop,
            $crate::commands::game_running_list,
            $crate::commands::settings_get,
            $crate::commands::settings_update,
            $crate::commands::connectors_list,
            $crate::commands::connector_login_start,
            $crate::commands::connector_login_complete,
            $crate::commands::connector_login_cancel,
            $crate::commands::connector_refresh,
            $crate::commands::connector_logout,
            $crate::commands::connector_detect_local,
            $crate::commands::connector_import_library,
            $crate::commands::achievements_sync,
            $crate::commands::cove_achievements_list,
            $crate::commands::plugins_list,
            $crate::commands::plugins_set_enabled,
            $crate::commands::plugins_approve,
            $crate::commands::plugins_inspect,
            $crate::commands::plugins_install,
            $crate::commands::plugins_uninstall,
            $crate::commands::plugins_open_folder,
            $crate::commands::plugin_storage_get,
            $crate::commands::plugin_storage_set,
            $crate::commands::plugin_storage_remove,
            $crate::commands::plugin_storage_keys,
            $crate::commands::plugin_define_achievements,
            $crate::commands::plugin_library_add,
            $crate::commands::downloads_list,
            $crate::commands::downloads_enqueue,
            $crate::commands::downloads_add_url,
            $crate::commands::downloads_add_itch,
            $crate::commands::itch_status,
            $crate::commands::itch_set_api_key,
            $crate::commands::itch_clear_api_key,
            $crate::commands::itch_owned_games,
            $crate::commands::itch_lookup,
            $crate::commands::itch_uploads,
            $crate::commands::downloads_set_status,
            $crate::commands::downloads_start_now,
            $crate::commands::free_games_list,
            $crate::commands::storage_overview,
            $crate::commands::game_news,
            $crate::commands::news_recent,
            $crate::commands::game_shortcuts,
            $crate::commands::game_create_shortcut,
            $crate::commands::game_remove_shortcut,
            $crate::commands::data_export,
            $crate::commands::data_import_pick,
            $crate::commands::data_import,
            $crate::commands::launchers_scan,
            $crate::commands::launchers_import,
            $crate::commands::mods_list,
            $crate::commands::mods_pick,
            $crate::commands::mods_add,
            $crate::commands::mods_set_enabled,
            $crate::commands::mods_move,
            $crate::commands::mods_remove,
            $crate::commands::game_fps_logs,
            $crate::commands::game_fps_logs_clear,
            $crate::commands::sunshine_status,
            $crate::commands::sunshine_sync,
            $crate::commands::sunshine_remove,
            $crate::commands::save_sync_status,
            $crate::commands::save_sync_connect,
            $crate::commands::save_sync_disconnect,
            $crate::commands::game_save_sync,
            $crate::commands::game_save_sync_now,
            $crate::commands::game_save_sync_resolve,
            $crate::commands::downloads_reorder,
            $crate::commands::downloads_clear_history,
            $crate::commands::system_profile,
            $crate::commands::system_about,
            $crate::commands::updates_check,
            $crate::commands::updates_install,
            $crate::commands::catalog_status,
            $crate::commands::catalog_set_credentials,
            $crate::commands::catalog_clear_credentials,
            $crate::commands::catalog_search,
            $crate::commands::catalog_browse,
            $crate::commands::catalog_game_details,
            $crate::commands::catalog_similar,
            $crate::commands::library_match_candidates,
            $crate::commands::library_link_catalog,
            $crate::commands::library_reject_match,
            $crate::commands::library_rematch,
            $crate::commands::recommendations_for_user,
            $crate::commands::recommendations_discover,
            $crate::commands::recommendations_similar,
            $crate::commands::recommendation_explain,
        ]
    };
}
#[cfg(test)]
pub(crate) use cove_handlers;

pub const GAME_STATE_CHANGED: &str = "game-state-changed";

/// Store accounts through the C# connectors (keyring for the sessions).
fn build_stores(pool: &sqlx::SqlitePool) -> Arc<stores::StoreService> {
    Arc::new(stores::StoreService::new(
        Arc::new(sidecar::Sidecar::with_errors(
            sidecar::resolve_connectors(),
            stores::CONNECTOR_ERRORS,
        )),
        pool.clone(),
        Arc::new(catalog::secrets::KeyringStore::app()),
        catalog::proxy::build_time_url(),
        std::time::Duration::from_millis(1100),
    ))
}

#[allow(clippy::too_many_lines)] // the one place that builds every service
fn init_state(app: &tauri::App) -> Result<AppState, Box<dyn std::error::Error>> {
    // Until the saved settings are read (or if they can't be), follow the system.
    i18n::set_language(i18n::system_default());
    let data_dir = app.path().app_data_dir()?;
    let pool = tauri::async_runtime::block_on(db::open(&data_dir.join("cove.db")))?;
    let closed = tauri::async_runtime::block_on(db::sessions::close_dangling(&pool))?;
    if closed > 0 {
        eprintln!("[cove] {closed} unterbrochene Spielsitzung(en) abgeschlossen");
    }

    let host = HostEnv::detect(data_dir.join("compat"));

    // Development builds also show the example plugins from the repository.
    let dev_plugins = if cfg!(debug_assertions) {
        vec![std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../../plugins")]
    } else {
        Vec::new()
    };
    let plugins = Arc::new(plugins::Plugins::new(
        pool.clone(),
        data_dir.join("plugins"),
        dev_plugins,
    ));

    let unlock_handle = app.handle().clone();
    let cove_achievements = Arc::new(cove_achievements::CoveAchievements::new(
        pool.clone(),
        plugins.clone(),
        Arc::new(move |achievement| {
            let _ = unlock_handle.emit(commands::ACHIEVEMENT_UNLOCKED, achievement);
        }),
    ));
    let handle = app.handle().clone();
    let checker = cove_achievements.clone();
    let emit = Arc::new(move |event: models::GameStateEvent| {
        // A finished session may unlock playtime or night-owl achievements.
        if event.session_seconds.is_some() {
            checker.check_in_background();
            after_session(&handle, event.game_id.clone());
        }
        let discord_handle = handle.clone();
        tauri::async_runtime::spawn(async move {
            if let Some(state) = discord_handle.try_state::<AppState>() {
                commands::refresh_discord(&state).await;
            }
        });
        let _ = handle.emit(GAME_STATE_CHANGED, event);
    });
    let opener_handle = app.handle().clone();
    let open_url = Arc::new(move |url: &str| {
        opener_handle
            .opener()
            .open_url(url, None::<&str>)
            .map_err(|e| AppError::Launch(e.to_string()))
    });

    let pruned = tauri::async_runtime::block_on(db::catalog_cache::prune(
        &pool,
        catalog::CACHE_MAX_AGE_DAYS,
    ))?;
    if pruned > 0 {
        eprintln!("[cove] {pruned} veraltete Katalog-Einträge entfernt");
    }
    let mut defaults = default_settings(host.home.as_ref());
    defaults.general.language = i18n::system_default().into();
    let saved = tauri::async_runtime::block_on(db::settings::load(&pool, &defaults))?;
    i18n::set_language(&saved.general.language);
    let catalog = Arc::new(catalog::CatalogService::new(
        pool.clone(),
        Arc::new(catalog::secrets::KeyringStore::app()),
        app.path().app_cache_dir()?.join("images"),
        defaults.clone(),
        catalog::Endpoints::default(),
    ));

    let recommender = Arc::new(recommend::Recommender::new(
        sidecar::Sidecar::new(sidecar::resolve_recommender()),
        pool.clone(),
        catalog.clone(),
    ));
    // Start the sidecar early: a PyInstaller binary needs a few seconds to unpack.
    let warm = recommender.clone();
    tauri::async_runtime::spawn(async move {
        if let Err(error) = warm.warm_up().await {
            eprintln!("[cove] {error}");
        }
    });

    let stores = build_stores(&pool);

    let downloads = build_downloads(app, &pool, &defaults, &stores, open_url.clone());
    let proton = build_proton(app, &pool, &data_dir, &host, &defaults);
    let backups = Arc::new(backups::BackupService::new(
        pool.clone(),
        defaults.clone(),
        data_dir.clone(),
        backups::manifest::MANIFEST_URL.into(),
    ));

    Ok(AppState {
        recommender,
        downloads,
        mover: build_mover(app, &pool),
        stores,
        login_flows: Arc::new(stores::login::LoginFlows::default()),
        cove_achievements,
        open_url: open_url.clone(),
        protondb: Arc::new(protondb::ProtonDb::new(pool.clone())),
        prices: Arc::new(prices::PriceService::new(pool.clone())),
        free_games: Arc::new(free_games::FreeGames::new(pool.clone())),
        news: Arc::new(news::News::new(pool.clone())),
        shortcuts: Arc::new(build_shortcuts(app, &pool, &data_dir)),
        mods: Arc::new(mods::Mods::new(pool.clone(), data_dir.join("mods"))),
        fps: Arc::new(fps::FpsLogs::new(pool.clone(), data_dir.join("fps-logs"))),
        sunshine: Arc::new(sunshine::Sunshine::new(
            sunshine::config_dirs(host.home.as_deref()),
            shortcuts::CoveCommand::current(),
        )),
        launcher_dirs: external::LauncherDirs::for_home(
            host.home.as_deref(),
            app.path().config_dir().ok().as_deref(),
        ),
        discord: Arc::new(discord::Discord::default()),
        gamepads: Arc::new(gamepad::Gamepads::default()),
        proton,
        save_sync: build_save_sync(app, &pool, &defaults, &backups),
        backups,
        launcher: Launcher::new(pool.clone(), emit, open_url, Timing::default()),
        defaults,
        catalog,
        host,
        pool,
        plugins,
    })
}

/// Linux: shortly after the start and then every few hours, a newer GE-Proton
/// is installed for the games that use "latest". Old builds are only removed
/// while no game runs.
fn keep_proton_up_to_date<R: tauri::Runtime>(app: tauri::AppHandle<R>) {
    if models::Platform::current() != models::Platform::Linux {
        return;
    }
    tauri::async_runtime::spawn(async move {
        tokio::time::sleep(std::time::Duration::from_secs(30)).await;
        loop {
            if let Some(state) = app.try_state::<AppState>() {
                let may_clean = state.launcher.running().is_empty();
                if let Err(error) = state.proton.check_updates(false, may_clean).await {
                    eprintln!("[cove] Proton-Update: {error}");
                }
            }
            tokio::time::sleep(proton::CHECK_INTERVAL).await;
        }
    });
}

/// A few minutes after the start and then every few hours: the wishlist's
/// prices; alerts go out as `price-alert` and as a desktop notification.
fn watch_prices<R: tauri::Runtime>(app: tauri::AppHandle<R>) {
    tauri::async_runtime::spawn(async move {
        tokio::time::sleep(std::time::Duration::from_secs(120)).await;
        loop {
            if let Some(state) = app.try_state::<AppState>() {
                commands::check_prices(&app, &state, false).await;
            }
            tokio::time::sleep(prices::CHECK_EVERY).await;
        }
    });
}

/// A few minutes after the start and then every few hours: new free games
/// (Epic, GOG) as desktop notifications.
fn watch_free_games<R: tauri::Runtime>(app: tauri::AppHandle<R>) {
    tauri::async_runtime::spawn(async move {
        tokio::time::sleep(std::time::Duration::from_secs(180)).await;
        loop {
            if let Some(state) = app.try_state::<AppState>() {
                commands::check_free_games(&app, &state).await;
            }
            tokio::time::sleep(free_games::CHECK_EVERY).await;
        }
    });
}

/// After a session (in the background): cloud saves of Epic and GOG games Cove
/// installed go up, then the save backup.
fn after_session<R: tauri::Runtime>(app: &tauri::AppHandle<R>, game_id: String) {
    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        let Some(state) = app.try_state::<AppState>() else {
            return;
        };
        if let Err(error) = state.downloads.after_session(&game_id, &state.host).await {
            eprintln!("[cove] Cloud-Spielstände von {game_id}: {error}");
        }
        state.backups.after_session(&game_id, &state.host).await;
        state.save_sync.after_session(&game_id, &state.host).await;
        if let Err(error) = state.fps.collect(&game_id).await {
            eprintln!("[cove] FPS-Log von {game_id}: {error}");
        }
    });
}

pub const SAVE_SYNC_CHANGED: &str = "save-sync-changed";

/// Save sync through the user's cloud; a conflict also gets a desktop notification.
fn build_save_sync(
    app: &tauri::App,
    pool: &sqlx::SqlitePool,
    defaults: &models::AppSettings,
    backups: &Arc<backups::BackupService>,
) -> Arc<savesync::SaveSync> {
    use tauri_plugin_notification::NotificationExt;
    let handle = app.handle().clone();
    let notify = Arc::new(move |item: &models::LibraryItem, notice| {
        let _ = handle.emit(
            SAVE_SYNC_CHANGED,
            serde_json::json!({ "gameId": item.game.id }),
        );
        if notice == savesync::Notice::Conflict {
            let title = &item.game.title;
            let _ = handle
                .notification()
                .builder()
                .title(tr!(
                    "Spielstände von {title} weichen ab",
                    "Saves of {title} differ"
                ))
                .body(tr!(
                    "Auf einem anderen Gerät wurde auch gespielt. Wähle auf der Seite des Spiels, welcher Stand bleibt.",
                    "It was played on another device too. Choose on the game's page which save stays."
                ))
                .show();
        }
    });
    Arc::new(savesync::SaveSync::new(
        pool.clone(),
        defaults.clone(),
        backups.clone(),
        Arc::new(catalog::secrets::KeyringStore::app()),
        savesync::cloud::Endpoints::default(),
        notify,
    ))
}

/// Shortcuts go to the desktop and the app menu of this system.
fn build_shortcuts(
    app: &tauri::App,
    pool: &sqlx::SqlitePool,
    data_dir: &std::path::Path,
) -> shortcuts::Shortcuts {
    let paths = app.path();
    let menu = if cfg!(windows) {
        paths
            .data_dir()
            .ok()
            .map(|d| d.join("Microsoft/Windows/Start Menu/Programs/Cove"))
    } else if cfg!(target_os = "macos") {
        paths.home_dir().ok().map(|h| h.join("Applications/Cove"))
    } else if flatpak::active() {
        // The sandbox's data folder is not the one the host's menu reads.
        paths
            .home_dir()
            .ok()
            .map(|h| h.join(".local/share/applications"))
    } else {
        paths.data_dir().ok().map(|d| d.join("applications"))
    };
    shortcuts::Shortcuts::new(
        pool.clone(),
        shortcuts::ShortcutDirs {
            desktop: paths.desktop_dir().ok(),
            menu,
            icons: data_dir.join("shortcut-icons"),
        },
        shortcuts::CoveCommand::current(),
        shortcuts::Kind::native(),
    )
}

/// "Spiel verschieben", reporting progress as `game-move-progress`.
fn build_proton(
    app: &tauri::App,
    pool: &sqlx::SqlitePool,
    data_dir: &std::path::Path,
    host: &HostEnv,
    defaults: &models::AppSettings,
) -> Arc<proton::ProtonManager> {
    let handle = app.handle().clone();
    Arc::new(proton::ProtonManager::new(
        pool.clone(),
        data_dir.join("proton"),
        host.home.clone(),
        defaults.clone(),
        Arc::new(move |progress| {
            let _ = handle.emit(commands::PROTON_PROGRESS, progress);
        }),
    ))
}

fn build_mover(app: &tauri::App, pool: &sqlx::SqlitePool) -> Arc<mover::Mover> {
    let handle = app.handle().clone();
    Arc::new(mover::Mover::new(
        pool.clone(),
        Arc::new(move |progress| {
            let _ = handle.emit(commands::GAME_MOVE_PROGRESS, progress);
        }),
    ))
}

/// The download manager with its hooks into the app (events, library follow-ups).
fn build_downloads(
    app: &tauri::App,
    pool: &sqlx::SqlitePool,
    defaults: &models::AppSettings,
    stores: &Arc<stores::StoreService>,
    open_url: launcher::OpenUrlFn,
) -> Arc<downloads::DownloadManager> {
    let list_handle = app.handle().clone();
    let progress_handle = app.handle().clone();
    let installed_handle = app.handle().clone();
    let list_pool = pool.clone();
    Arc::new(downloads::DownloadManager::new(
        pool.clone(),
        defaults.clone(),
        downloads::itch::ItchApi::new(
            Arc::new(catalog::secrets::KeyringStore::app()),
            downloads::itch::API_BASE,
        ),
        Some(stores.clone()),
        downloads::Hooks {
            changed: Arc::new(move || {
                let app = list_handle.clone();
                let pool = list_pool.clone();
                tauri::async_runtime::spawn(async move {
                    if let Ok(items) = db::downloads::list(&pool).await {
                        let _ = app.emit(commands::DOWNLOADS_CHANGED, items);
                    }
                });
            }),
            progress: Arc::new(move |progress| {
                let _ = progress_handle.emit(commands::DOWNLOAD_PROGRESS, progress);
            }),
            installed: Arc::new(move |game_id| {
                if let Some(state) = installed_handle.try_state::<AppState>() {
                    commands::after_install(&installed_handle, &state, game_id);
                }
            }),
            open_url,
        },
    ))
}

/// Serves a file of an active plugin to its worker (`cove-plugin://localhost/<id>/<path>`).
async fn serve_plugin_file<R: tauri::Runtime>(
    app: &tauri::AppHandle<R>,
    path: &str,
) -> tauri::http::Response<Vec<u8>> {
    use tauri::http::header;
    let respond = |status: u16, mime: &str, body: Vec<u8>| {
        tauri::http::Response::builder()
            .status(status)
            .header(header::CONTENT_TYPE, mime)
            // Module workers load cross-origin: allow it, but never cache plugin code.
            .header(header::ACCESS_CONTROL_ALLOW_ORIGIN, "*")
            .header(header::CACHE_CONTROL, "no-store")
            .header("X-Content-Type-Options", "nosniff")
            .body(body)
            .unwrap_or_default()
    };
    let Some(state) = app.try_state::<AppState>() else {
        return respond(503, "text/plain", Vec::new());
    };
    match state.plugins.resolve_file(path).await {
        Ok(file) => match tokio::fs::read(&file).await {
            Ok(bytes) => respond(200, plugins::mime(&file), bytes),
            Err(_) => respond(404, "text/plain", Vec::new()),
        },
        Err(error) => {
            eprintln!("[plugins] {path}: {error}");
            respond(404, "text/plain", Vec::new())
        }
    }
}

/// Answers a `cove-img://` request from the local image cache (downloading on first use).
async fn serve_image<R: tauri::Runtime>(
    app: &tauri::AppHandle<R>,
    path: &str,
) -> tauri::http::Response<Vec<u8>> {
    use catalog::images::{ImageCache, ImageError};
    let respond = |status: u16, mime: &str, body: Vec<u8>| {
        tauri::http::Response::builder()
            .status(status)
            .header(tauri::http::header::CONTENT_TYPE, mime)
            .header(tauri::http::header::CACHE_CONTROL, "max-age=604800")
            .body(body)
            .unwrap_or_default()
    };
    let Some(remote) = ImageCache::decode_request_path(path) else {
        return respond(400, "text/plain", Vec::new());
    };
    let Some(state) = app.try_state::<AppState>() else {
        return respond(503, "text/plain", Vec::new());
    };
    match state.catalog.images.get(&remote).await {
        Ok((bytes, mime)) => respond(200, mime, bytes),
        Err(ImageError::Forbidden) => respond(403, "text/plain", Vec::new()),
        Err(ImageError::BadRequest) => respond(400, "text/plain", Vec::new()),
        Err(ImageError::Unavailable | ImageError::TooLarge) => {
            respond(404, "text/plain", Vec::new())
        }
    }
}

/// Shown instead of a panic when the core cannot start (e.g. a broken database).
fn report_startup_failure(app: &tauri::App, error: &dyn std::error::Error) {
    use tauri_plugin_dialog::{DialogExt, MessageDialogKind};
    eprintln!("[cove] Start fehlgeschlagen: {error}");
    if let Some(window) = app.get_webview_window("main") {
        let _ = window.hide();
    }
    let data_dir = app
        .path()
        .app_data_dir()
        .map(|d| d.display().to_string())
        .unwrap_or_default();
    let handle = app.handle().clone();
    app.dialog()
        .message(tr!(
            "Cove konnte seine Daten nicht laden und wird beendet.\n\n\
             Deine Spiele und Einstellungen liegen unverändert in:\n{data_dir}\n\n\
             Starte Cove noch einmal. Wenn der Fehler bleibt, melde ihn bitte auf GitHub \
             und füge diese Meldung bei:\n{error}",
            "Cove couldn't load its data and will close.\n\n\
             Your games and settings are unchanged in:\n{data_dir}\n\n\
             Start Cove again. If the error stays, please report it on GitHub \
             and include this message:\n{error}"
        ))
        .title(tr!("Cove konnte nicht starten", "Cove couldn't start"))
        .kind(MessageDialogKind::Error)
        .show(move |_| handle.exit(1));
}

/// Only one Cove runs. A second start (a shortcut, the app menu) hands its
/// command line over: `--launch <id>` starts that game, anything else brings
/// the window to the front. In Flatpak the D-Bus name has to start with the
/// app id, else the sandbox doesn't allow it.
fn single_instance<R: tauri::Runtime>() -> tauri::plugin::TauriPlugin<R> {
    let mut builder = tauri_plugin_single_instance::Builder::new().callback(|app, args, _cwd| {
        if let Some(game_id) = shortcuts::launch_arg(&args) {
            commands::launch_from_shortcut(app.clone(), game_id);
        } else if let Some(window) = app.get_webview_window("main") {
            let _ = window.unminimize();
            let _ = window.show();
            let _ = window.set_focus();
        }
    });
    if let Some(id) = std::env::var("FLATPAK_ID")
        .ok()
        .filter(|_| flatpak::active())
    {
        builder = builder.dbus_id(id);
    }
    builder.build()
}

/// Builds and runs the Tauri application.
pub fn run() {
    let started = std::time::Instant::now();
    let logged_start = std::sync::atomic::AtomicBool::new(false);
    tauri::Builder::default()
        // First: a second Cove (a desktop shortcut) hands its request to this one.
        .plugin(single_instance())
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_updater::Builder::new().build())
        .on_page_load(move |_, payload| {
            if payload.event() == tauri::webview::PageLoadEvent::Finished
                && !logged_start.swap(true, std::sync::atomic::Ordering::Relaxed)
            {
                eprintln!(
                    "[cove] Oberfläche geladen nach {} ms",
                    started.elapsed().as_millis()
                );
            }
        })
        .register_asynchronous_uri_scheme_protocol(plugins::SCHEME, |ctx, request, responder| {
            let app = ctx.app_handle().clone();
            let path = request.uri().path().to_owned();
            tauri::async_runtime::spawn(async move {
                responder.respond(serve_plugin_file(&app, &path).await);
            });
        })
        .register_asynchronous_uri_scheme_protocol(
            catalog::images::SCHEME,
            |ctx, request, responder| {
                let app = ctx.app_handle().clone();
                let path = request.uri().path().to_owned();
                tauri::async_runtime::spawn(async move {
                    responder.respond(serve_image(&app, &path).await);
                });
            },
        )
        .plugin(tauri_plugin_notification::init())
        .setup(|app| {
            let state = match init_state(app) {
                Ok(state) => state,
                Err(error) => {
                    report_startup_failure(app, error.as_ref());
                    return Ok(());
                }
            };
            let downloads = state.downloads.clone();
            let gamepads = state.gamepads.clone();
            app.manage(state);
            app.manage(updates::UpdaterRegistered);
            // After `manage`: the download hooks look up the state.
            downloads.start();
            keep_proton_up_to_date(app.handle().clone());
            watch_prices(app.handle().clone());
            watch_free_games(app.handle().clone());
            gamepad::watch(app.handle().clone(), gamepads);
            let args: Vec<String> = std::env::args().collect();
            if let Some(game_id) = shortcuts::launch_arg(&args) {
                commands::launch_from_shortcut(app.handle().clone(), game_id);
            }
            Ok(())
        })
        .invoke_handler(cove_handlers!())
        .run(tauri::generate_context!())
        .expect("error while running Cove");
}
