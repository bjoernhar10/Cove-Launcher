//! Wire types shared with the frontend. Every struct mirrors a type in
//! `packages/shared/src/models.ts` (camelCase JSON). A contract test validates
//! serialized values against `packages/shared/schemas/models.schema.json`.

use serde::{Deserialize, Serialize};

pub type GameId = String;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum StoreSource {
    Steam,
    Gog,
    Epic,
    Manual,
}

impl StoreSource {
    #[cfg(test)]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Steam => "steam",
            Self::Gog => "gog",
            Self::Epic => "epic",
            Self::Manual => "manual",
        }
    }

    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "steam" => Some(Self::Steam),
            "gog" => Some(Self::Gog),
            "epic" => Some(Self::Epic),
            "manual" => Some(Self::Manual),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Platform {
    Windows,
    Linux,
    Macos,
}

impl Platform {
    pub fn current() -> Self {
        match std::env::consts::OS {
            "windows" => Self::Windows,
            "macos" => Self::Macos,
            _ => Self::Linux,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ImageRef {
    pub url: Option<String>,
    pub alt: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct GameArtwork {
    pub cover: ImageRef,
    pub header: ImageRef,
    pub screenshots: Vec<ImageRef>,
}

/// Genres are kept as strings in Rust; the TS side narrows them to `Genre`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Game {
    pub id: GameId,
    pub title: String,
    pub summary: String,
    pub description: String,
    pub genres: Vec<String>,
    pub platforms: Vec<Platform>,
    pub release_date: Option<String>,
    pub developer: String,
    pub publisher: String,
    pub rating: Option<i64>,
    pub popularity: i64,
    pub artwork: GameArtwork,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "kebab-case")]
pub enum CompatibilityTool {
    #[default]
    Native,
    Proton,
    ProtonExperimental,
    Wine,
}

impl CompatibilityTool {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Native => "native",
            Self::Proton => "proton",
            Self::ProtonExperimental => "proton-experimental",
            Self::Wine => "wine",
        }
    }

    pub fn parse(value: &str) -> Self {
        match value {
            "proton" => Self::Proton,
            "proton-experimental" => Self::ProtonExperimental,
            "wine" => Self::Wine,
            _ => Self::Native,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct EnvVar {
    pub key: String,
    pub value: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct LaunchOptions {
    pub arguments: String,
    pub compatibility_tool: CompatibilityTool,
    pub env: Vec<EnvVar>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct AchievementProgress {
    pub unlocked: i64,
    pub total: i64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LibraryEntry {
    pub game_id: GameId,
    pub source: StoreSource,
    pub store_game_id: Option<String>,
    pub installed: bool,
    pub install_path: Option<String>,
    pub executable_path: Option<String>,
    pub install_size_bytes: Option<i64>,
    pub favorite: bool,
    pub playtime_minutes: i64,
    pub last_played_at: Option<String>,
    pub added_at: String,
    pub achievements: AchievementProgress,
    pub launch_options: LaunchOptions,
    pub catalog_id: Option<String>,
    pub match_status: MatchStatus,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum MatchStatus {
    #[default]
    None,
    Pending,
    Matched,
    Rejected,
}

impl MatchStatus {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::None => "none",
            Self::Pending => "pending",
            Self::Matched => "matched",
            Self::Rejected => "rejected",
        }
    }

    pub fn parse(value: &str) -> Self {
        match value {
            "pending" => Self::Pending,
            "matched" => Self::Matched,
            "rejected" => Self::Rejected,
            _ => Self::None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LibraryItem {
    pub entry: LibraryEntry,
    pub game: Game,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Achievement {
    pub id: String,
    pub game_id: GameId,
    pub title: String,
    pub description: String,
    pub unlocked_at: Option<String>,
    pub rarity_percent: Option<f64>,
    pub hidden: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AchievementUnlock {
    pub achievement: Achievement,
    pub game: Game,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PlaytimeDay {
    pub date: String,
    pub minutes: i64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PlaytimeStats {
    pub game_id: GameId,
    pub total_minutes: i64,
    pub last14_days_minutes: i64,
    pub session_count: i64,
    pub days: Vec<PlaytimeDay>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum DownloadStatus {
    Downloading,
    Paused,
    Queued,
    Completed,
    Failed,
    Cancelled,
}

impl DownloadStatus {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Downloading => "downloading",
            Self::Paused => "paused",
            Self::Queued => "queued",
            Self::Completed => "completed",
            Self::Failed => "failed",
            Self::Cancelled => "cancelled",
        }
    }

    pub fn parse(value: &str) -> Self {
        match value {
            "downloading" => Self::Downloading,
            "paused" => Self::Paused,
            "completed" => Self::Completed,
            "failed" => Self::Failed,
            "cancelled" => Self::Cancelled,
            _ => Self::Queued,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Download {
    pub id: String,
    pub game_id: GameId,
    pub source: StoreSource,
    pub status: DownloadStatus,
    pub total_bytes: i64,
    pub downloaded_bytes: i64,
    pub bytes_per_second: i64,
    pub queue_position: Option<i64>,
    pub queued_at: String,
    pub finished_at: Option<String>,
    pub error: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DownloadItem {
    pub download: Download,
    pub game: Game,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UserProfile {
    pub id: String,
    pub display_name: String,
    pub avatar_url: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StoreAccount {
    pub store: StoreSource,
    pub connected: bool,
    pub account_name: Option<String>,
    pub game_count: i64,
    pub last_synced_at: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PluginInfo {
    pub id: String,
    pub name: String,
    pub version: String,
    pub author: String,
    pub description: String,
    pub enabled: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ComponentVersion {
    pub name: String,
    pub version: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AboutInfo {
    pub app_version: String,
    pub components: Vec<ComponentVersion>,
    pub platform: Platform,
}

// ───────────── settings ─────────────

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GeneralSettings {
    pub language: String,
    pub launch_on_startup: bool,
    pub minimize_to_tray: bool,
    pub start_page: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AppearanceSettings {
    pub theme: String,
    pub accent: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LibrarySettings {
    pub scan_folders: Vec<String>,
    pub scan_on_startup: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DownloadSettings {
    pub directory: String,
    pub bandwidth_limit_bytes_per_second: Option<i64>,
    pub max_concurrent: i64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CatalogSettings {
    /// Preferred catalogue source; the other configured one is the fallback.
    pub provider: CatalogProviderId,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AppSettings {
    pub general: GeneralSettings,
    pub appearance: AppearanceSettings,
    pub library: LibrarySettings,
    pub downloads: DownloadSettings,
    pub catalog: CatalogSettings,
}

// ───────────── catalogue ─────────────

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum CatalogProviderId {
    #[default]
    Igdb,
    Rawg,
}

impl CatalogProviderId {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Igdb => "igdb",
            Self::Rawg => "rawg",
        }
    }

    pub fn other(self) -> Self {
        match self {
            Self::Igdb => Self::Rawg,
            Self::Rawg => Self::Igdb,
        }
    }

    /// Splits a catalogue id like `igdb-1942` into provider and native id.
    pub fn parse_catalog_id(catalog_id: &str) -> Option<(Self, &str)> {
        let (prefix, native) = catalog_id.split_once('-')?;
        let provider = match prefix {
            "igdb" => Self::Igdb,
            "rawg" => Self::Rawg,
            _ => return None,
        };
        (!native.is_empty()).then_some((provider, native))
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum CatalogSort {
    #[default]
    Recommended,
    Popular,
    Newest,
    Title,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct YearRange {
    pub from: i32,
    pub to: i32,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct CatalogFilter {
    pub query: String,
    pub genres: Vec<String>,
    pub platforms: Vec<Platform>,
    pub release_years: Option<YearRange>,
    pub min_rating: Option<i64>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CatalogPage {
    pub items: Vec<Game>,
    /// 0-based page index.
    pub page: u32,
    pub has_more: bool,
    /// Total number of matches if the source reports it.
    pub total: Option<i64>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CatalogGameDetails {
    pub game: Game,
    /// Library game linked to this catalogue entry, if any.
    pub library_game_id: Option<GameId>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProvidersConfigured {
    pub igdb: bool,
    pub rawg: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CatalogStatus {
    pub preferred: CatalogProviderId,
    pub configured: ProvidersConfigured,
    /// False after the last request failed for network reasons (cached data is shown).
    pub online: bool,
}

/// Credentials entered in Settings. Never sent back to the frontend.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "provider", rename_all = "lowercase")]
pub enum CatalogCredentials {
    #[serde(rename_all = "camelCase")]
    Igdb { client_id: String, client_secret: String },
    #[serde(rename_all = "camelCase")]
    Rawg { api_key: String },
}

impl CatalogCredentials {
    pub fn provider(&self) -> CatalogProviderId {
        match self {
            Self::Igdb { .. } => CatalogProviderId::Igdb,
            Self::Rawg { .. } => CatalogProviderId::Rawg,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct MatchCandidate {
    pub game: Game,
    /// Name similarity 0–1.
    pub score: f64,
}

/// Payload of the `library-match-changed` event.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LibraryMatchEvent {
    pub game_id: GameId,
    pub status: MatchStatus,
}

// ───────────── library commands ─────────────

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ManualGameInput {
    pub title: String,
    pub executable_path: String,
    pub size_bytes: Option<i64>,
}

/// Partial update for a library entry. Absent fields stay unchanged.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct LibraryUpdate {
    #[serde(default)]
    pub title: Option<String>,
    #[serde(default)]
    pub favorite: Option<bool>,
    #[serde(default)]
    pub executable_path: Option<String>,
    #[serde(default)]
    pub launch_options: Option<LaunchOptions>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum ExecutableKind {
    WindowsExe,
    LinuxBinary,
    Appimage,
    DesktopEntry,
    Script,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ScanCandidate {
    pub title: String,
    pub executable_path: String,
    pub size_bytes: i64,
    pub kind: ExecutableKind,
    pub already_in_library: bool,
}

// ───────────── game runtime ─────────────

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum GameState {
    Running,
    Stopped,
}

/// Payload of the `game-state-changed` event.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GameStateEvent {
    pub game_id: GameId,
    pub state: GameState,
    pub started_at: String,
    /// Length of the finished session; null while running.
    pub session_seconds: Option<i64>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RunningGame {
    pub game_id: GameId,
    pub started_at: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum LaunchMethod {
    Process,
    Url,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LaunchResult {
    pub game_id: GameId,
    pub method: LaunchMethod,
    /// False when Cove cannot observe the game (e.g. started via a store client without known install dir).
    pub tracked: bool,
}

/// Current UTC time as ISO-8601 with second precision (`2026-11-03T09:52:57Z`).
pub fn now_iso() -> String {
    chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Secs, true)
}
