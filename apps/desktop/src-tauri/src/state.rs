//! Shared application state managed by Tauri.

use std::path::PathBuf;

use sqlx::SqlitePool;

use std::sync::Arc;

use crate::catalog::CatalogService;
use crate::cove_achievements::CoveAchievements;
use crate::downloads::DownloadManager;
use crate::launcher::{HostEnv, Launcher};
use crate::models::{
    AppSettings, AppearanceSettings, CatalogSettings, DownloadSettings, GeneralSettings,
    LibrarySettings,
};
use crate::recommend::Recommender;
use crate::stores::login::{LoginFlows, OpenUrl};
use crate::stores::StoreService;

pub struct AppState {
    pub pool: SqlitePool,
    pub launcher: Launcher,
    pub host: HostEnv,
    pub defaults: AppSettings,
    pub catalog: Arc<CatalogService>,
    pub recommender: Arc<Recommender>,
    /// Installed plugins, their permissions and data.
    pub plugins: Arc<crate::plugins::Plugins>,
    pub stores: Arc<StoreService>,
    pub login_flows: Arc<LoginFlows>,
    pub cove_achievements: Arc<CoveAchievements>,
    pub downloads: Arc<DownloadManager>,
    /// "Spiel verschieben" in the background.
    pub mover: Arc<crate::mover::Mover>,
    /// Opens a URL in the system browser.
    pub open_url: OpenUrl,
}

pub fn default_settings(home: Option<&PathBuf>) -> AppSettings {
    let games_dir = home.map_or_else(|| PathBuf::from("Games"), |h| h.join("Games"));
    AppSettings {
        general: GeneralSettings {
            language: "de".into(),
            launch_on_startup: false,
            minimize_to_tray: false,
            start_page: "home".into(),
        },
        appearance: AppearanceSettings {
            theme: "system".into(),
            accent: "blue".into(),
        },
        library: LibrarySettings {
            scan_folders: Vec::new(),
            scan_on_startup: false,
            game_folders: vec![games_dir.to_string_lossy().into_owned()],
            steam_silent_start: true,
            epic_cloud_saves: true,
        },
        downloads: DownloadSettings {
            directory: games_dir.to_string_lossy().into_owned(),
            bandwidth_limit_bytes_per_second: None,
            max_concurrent: 1,
        },
        catalog: CatalogSettings {
            use_cove_service: true,
        },
    }
}
