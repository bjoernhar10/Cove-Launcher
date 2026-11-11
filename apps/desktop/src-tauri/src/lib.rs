//! Cove core: owns the game library, launches games and supervises the
//! sidecar services (connectors, recommender) over JSON-RPC/stdio.

// First, so `tr!` is in scope for every other module.
#[macro_use]
mod i18n;
mod auth;
mod catalog;
mod commands;
mod cove_achievements;
mod db;
mod downloads;
mod error;
mod folders;
mod launcher;
mod models;
mod mover;
mod plugins;
mod protondb;
mod recommend;
mod scanner;
mod sidecar;
mod state;
mod steam;
mod stores;
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
            $crate::commands::library_achievements,
            $crate::commands::library_playtime,
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

    let stores = Arc::new(stores::StoreService::new(
        Arc::new(sidecar::Sidecar::with_errors(
            sidecar::resolve_connectors(),
            stores::CONNECTOR_ERRORS,
        )),
        pool.clone(),
        Arc::new(catalog::secrets::KeyringStore::app()),
        catalog::proxy::build_time_url(),
        std::time::Duration::from_millis(1100),
    ));

    let downloads = build_downloads(app, &pool, &defaults, &stores, open_url.clone());

    Ok(AppState {
        recommender,
        downloads,
        mover: build_mover(app, &pool),
        stores,
        login_flows: Arc::new(stores::login::LoginFlows::default()),
        cove_achievements,
        open_url: open_url.clone(),
        protondb: Arc::new(protondb::ProtonDb::new(pool.clone())),
        launcher: Launcher::new(pool.clone(), emit, open_url, Timing::default()),
        defaults,
        catalog,
        host,
        pool,
        plugins,
    })
}

/// After a session: Epic cloud saves of games Cove installed go up (in the background).
fn after_session<R: tauri::Runtime>(app: &tauri::AppHandle<R>, game_id: String) {
    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        let Some(state) = app.try_state::<AppState>() else {
            return;
        };
        if let Err(error) = state.downloads.after_session(&game_id, &state.host).await {
            eprintln!("[cove] Cloud-Spielstände von {game_id}: {error}");
        }
    });
}

/// "Spiel verschieben", reporting progress as `game-move-progress`.
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

/// Builds and runs the Tauri application.
pub fn run() {
    let started = std::time::Instant::now();
    let logged_start = std::sync::atomic::AtomicBool::new(false);
    tauri::Builder::default()
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
        .setup(|app| {
            let state = match init_state(app) {
                Ok(state) => state,
                Err(error) => {
                    report_startup_failure(app, error.as_ref());
                    return Ok(());
                }
            };
            let downloads = state.downloads.clone();
            app.manage(state);
            app.manage(updates::UpdaterRegistered);
            // After `manage`: the download hooks look up the state.
            downloads.start();
            Ok(())
        })
        .invoke_handler(cove_handlers!())
        .run(tauri::generate_context!())
        .expect("error while running Cove");
}
