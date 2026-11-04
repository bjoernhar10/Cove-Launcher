//! Cove core: owns the game library, launches games and supervises the
//! sidecar services (connectors, recommender) over JSON-RPC/stdio.

mod auth;
mod catalog;
mod commands;
mod cove_achievements;
mod db;
mod error;
mod launcher;
mod models;
mod plugins;
mod recommend;
mod scanner;
mod sidecar;
mod state;
mod stores;

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
            $crate::commands::library_achievements,
            $crate::commands::library_playtime,
            $crate::commands::achievements_recent,
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
            $crate::commands::downloads_list,
            $crate::commands::downloads_enqueue,
            $crate::commands::downloads_set_status,
            $crate::commands::downloads_reorder,
            $crate::commands::downloads_clear_history,
            $crate::commands::system_profile,
            $crate::commands::system_about,
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
    let data_dir = app.path().app_data_dir()?;
    let pool = tauri::async_runtime::block_on(db::open(&data_dir.join("cove.db")))?;
    let closed = tauri::async_runtime::block_on(db::sessions::close_dangling(&pool))?;
    if closed > 0 {
        eprintln!("[cove] {closed} unterbrochene Spielsitzung(en) abgeschlossen");
    }

    let host = HostEnv::detect(data_dir.join("compat"));

    let mut plugin_dirs = vec![data_dir.join("plugins")];
    if cfg!(debug_assertions) {
        // Development: also show the example plugins from the repository.
        plugin_dirs
            .push(std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../../plugins"));
    }

    let unlock_handle = app.handle().clone();
    let cove_achievements = Arc::new(cove_achievements::CoveAchievements::new(
        pool.clone(),
        plugin_dirs.clone(),
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
    let defaults = default_settings(host.home.as_ref());
    let catalog = Arc::new(catalog::CatalogService::new(
        pool.clone(),
        Arc::new(catalog::secrets::KeyringStore),
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
        Arc::new(catalog::secrets::KeyringStore),
        catalog::proxy::build_time_url(),
        std::time::Duration::from_millis(1100),
    ));

    Ok(AppState {
        recommender,
        stores,
        login_flows: Arc::new(stores::login::LoginFlows::default()),
        cove_achievements,
        open_url: open_url.clone(),
        launcher: Launcher::new(pool.clone(), emit, open_url, Timing::default()),
        defaults,
        catalog,
        host,
        pool,
        plugin_dirs,
    })
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

/// Builds and runs the Tauri application.
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
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
            let state = init_state(app)?;
            app.manage(state);
            Ok(())
        })
        .invoke_handler(cove_handlers!())
        .run(tauri::generate_context!())
        .expect("error while running Cove");
}
