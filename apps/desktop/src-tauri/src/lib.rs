//! Cove core: owns the game library, launches games and supervises the
//! sidecar services (connectors, recommender) over JSON-RPC/stdio.

mod commands;
mod db;
mod error;
mod launcher;
mod models;
mod plugins;
mod scanner;
mod state;

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
            $crate::commands::sources_list,
            $crate::commands::sources_connect,
            $crate::commands::plugins_list,
            $crate::commands::plugins_set_enabled,
            $crate::commands::downloads_list,
            $crate::commands::downloads_enqueue,
            $crate::commands::downloads_set_status,
            $crate::commands::downloads_reorder,
            $crate::commands::downloads_clear_history,
            $crate::commands::system_profile,
            $crate::commands::system_about,
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
    let handle = app.handle().clone();
    let emit = Arc::new(move |event| {
        let _ = handle.emit(GAME_STATE_CHANGED, event);
    });
    let opener_handle = app.handle().clone();
    let open_url = Arc::new(move |url: &str| {
        opener_handle
            .opener()
            .open_url(url, None::<&str>)
            .map_err(|e| AppError::Launch(e.to_string()))
    });

    let mut plugin_dirs = vec![data_dir.join("plugins")];
    if cfg!(debug_assertions) {
        // Development: also show the example plugins from the repository.
        plugin_dirs
            .push(std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../../plugins"));
    }

    Ok(AppState {
        launcher: Launcher::new(pool.clone(), emit, open_url, Timing::default()),
        defaults: default_settings(host.home.as_ref()),
        host,
        pool,
        plugin_dirs,
    })
}

/// Builds and runs the Tauri application.
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .setup(|app| {
            let state = init_state(app)?;
            app.manage(state);
            Ok(())
        })
        .invoke_handler(cove_handlers!())
        .run(tauri::generate_context!())
        .expect("error while running Cove");
}
