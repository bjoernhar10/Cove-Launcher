//! Wire types shared with the frontend. Every struct mirrors a type in
//! `packages/shared/src/models.ts` (camelCase JSON). A contract test validates
//! serialized values against `packages/shared/schemas/models.schema.json`.

use serde::{Deserialize, Serialize};

pub type GameId = String;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum StoreSource {
    Steam,
    Gog,
    Epic,
    Amazon,
    Manual,
}

impl StoreSource {
    pub fn display_name(self) -> &'static str {
        match self {
            Self::Steam => "Steam",
            Self::Gog => "GOG",
            Self::Epic => "Epic",
            Self::Amazon => "Amazon",
            Self::Manual => "Manuell",
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Steam => "steam",
            Self::Gog => "gog",
            Self::Epic => "epic",
            Self::Amazon => "amazon",
            Self::Manual => "manual",
        }
    }

    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "steam" => Some(Self::Steam),
            "gog" => Some(Self::Gog),
            "epic" => Some(Self::Epic),
            "amazon" => Some(Self::Amazon),
            "manual" => Some(Self::Manual),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
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

/// Image reference. Remote http(s) URLs are rewritten to the local image cache
/// (`cove-img://…`) when sent to the UI and restored when received back, so the
/// database and caches always hold the original URLs.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ImageRef {
    pub url: Option<String>,
    pub alt: String,
}

#[derive(Serialize, Deserialize)]
struct ImageRefWire {
    url: Option<String>,
    alt: String,
}

impl Serialize for ImageRef {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        ImageRefWire {
            url: crate::catalog::images::proxied(self.url.clone()),
            alt: self.alt.clone(),
        }
        .serialize(serializer)
    }
}

impl<'de> Deserialize<'de> for ImageRef {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let wire = ImageRefWire::deserialize(deserializer)?;
        Ok(Self {
            url: crate::catalog::images::unproxied(wire.url),
            alt: wire.alt,
        })
    }
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
    /// Extra descriptors for the recommender; None when the source does not provide them.
    #[serde(default)]
    pub features: Option<GameFeatures>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct GameFeatures {
    pub themes: Vec<String>,
    pub game_modes: Vec<String>,
    pub perspectives: Vec<String>,
    pub keywords: Vec<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "kebab-case")]
pub enum CompatibilityTool {
    #[default]
    Native,
    Proton,
    Wine,
}

impl CompatibilityTool {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Native => "native",
            Self::Proton => "proton",
            Self::Wine => "wine",
        }
    }

    pub fn parse(value: &str) -> Self {
        match value {
            "proton" => Self::Proton,
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
    /// With Proton: the build to use (`proton::LATEST`, a GE-Proton release
    /// name or `steam:<folder>`); `None` = the default from the settings.
    #[serde(default)]
    pub proton_version: Option<String>,
    /// Linux: `MangoHud`, `GameMode`, gamescope, FSR, DXVK/VKD3D.
    #[serde(default)]
    pub tweaks: GameTweaks,
}

/// Per-game options on Linux for games Cove starts itself.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
#[allow(clippy::struct_excessive_bools)] // independent switches
pub struct GameTweaks {
    /// Performance overlay (`mangohud`).
    #[serde(default)]
    pub mangohud: bool,
    /// Feral `GameMode` (`gamemoderun`).
    #[serde(default)]
    pub gamemode: bool,
    /// Run inside Valve's gamescope compositor.
    #[serde(default)]
    pub gamescope: bool,
    /// gamescope: the resolution the game renders at (`-w`/`-h`); None = the game's own.
    #[serde(default)]
    pub gamescope_width: Option<u32>,
    #[serde(default)]
    pub gamescope_height: Option<u32>,
    /// gamescope: fullscreen (`-f`).
    #[serde(default)]
    pub gamescope_fullscreen: bool,
    /// AMD FSR 1 upscaling: gamescope's filter, else GE-Proton's `WINE_FULLSCREEN_FSR`.
    #[serde(default)]
    pub fsr: bool,
    /// Proton: Direct3D 9 to 11 through DXVK (off = `WineD3D`).
    #[serde(default = "default_true")]
    pub dxvk: bool,
    /// Proton: Direct3D 12 through VKD3D-Proton (off = no D3D12).
    #[serde(default = "default_true")]
    pub vkd3d: bool,
}

impl Default for GameTweaks {
    fn default() -> Self {
        Self {
            mangohud: false,
            gamemode: false,
            gamescope: false,
            gamescope_width: None,
            gamescope_height: None,
            gamescope_fullscreen: false,
            fsr: false,
            dxvk: true,
            vkd3d: true,
        }
    }
}

/// Which Linux game tools are installed.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct GameTools {
    pub mangohud: bool,
    pub gamemode: bool,
    pub gamescope: bool,
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
    /// How "Spielen" starts the game; null when it can't be started yet.
    pub launch_via: Option<LaunchVia>,
    /// Installed by Cove's download manager: Cove may move and uninstall it.
    pub installed_by_cove: bool,
}

/// How a game is started, shown per game ("Direkt", "Über Steam" …).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum LaunchVia {
    /// Cove starts the executable itself.
    Direct,
    Steam,
    EpicLauncher,
    GogGalaxy,
    AmazonGames,
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
    pub icon: ImageRef,
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

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Money {
    pub amount: f64,
    pub currency: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PriceDeal {
    pub shop: String,
    pub price: Money,
    pub regular: Money,
    pub cut: i64,
    pub url: String,
    pub expiry: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GamePrice {
    pub url: Option<String>,
    pub best: Option<PriceDeal>,
    pub deals: Vec<PriceDeal>,
    pub history_low: Option<Money>,
    pub checked_at: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WishlistItem {
    pub game: Game,
    pub added_at: String,
    pub target_price: Option<f64>,
    pub notify_on_sale: bool,
    pub price: Option<GamePrice>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum AlertReason {
    Target,
    Sale,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PriceAlert {
    pub game_id: GameId,
    pub title: String,
    pub deal: PriceDeal,
    pub reason: AlertReason,
}

/// Which button names and symbols the UI shows.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ControllerKind {
    Xbox,
    Playstation,
    Switch,
    Steam,
    Generic,
}

/// A controller button's meaning (face buttons by position).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum PadAction {
    Up,
    Down,
    Left,
    Right,
    Confirm,
    Back,
    Secondary,
    Tertiary,
    PrevTab,
    NextTab,
    Menu,
    View,
    Home,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GamepadInfo {
    pub id: u32,
    pub name: String,
    pub kind: ControllerKind,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GamepadInput {
    pub action: PadAction,
    pub kind: ControllerKind,
}

/// Statistics page (`stats_overview`), mirrors `PlayStats`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PlayStats {
    pub year: Option<i32>,
    pub years: Vec<i32>,
    pub total_minutes: i64,
    pub session_count: i64,
    pub games_played: i64,
    pub days_played: i64,
    pub longest_streak_days: i64,
    pub longest_session: Option<StatsSession>,
    pub achievements_unlocked: i64,
    pub months: Vec<StatsMonth>,
    pub weekdays: Vec<i64>,
    pub top_games: Vec<StatsGame>,
    pub genres: Vec<StatsGenre>,
    pub stores: Vec<StatsStore>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StatsMonth {
    pub month: String,
    pub minutes: i64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StatsSession {
    pub game_id: GameId,
    pub title: String,
    pub minutes: i64,
    pub started_at: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StatsGame {
    pub game: Game,
    pub minutes: i64,
    pub session_count: i64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StatsGenre {
    pub genre: String,
    pub minutes: i64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StatsStore {
    pub store: StoreSource,
    pub minutes: i64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum DownloadStatus {
    Downloading,
    Paused,
    Queued,
    Verifying,
    Extracting,
    Completed,
    Failed,
    Cancelled,
    HandedOff,
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
            Self::Verifying => "verifying",
            Self::Extracting => "extracting",
            Self::HandedOff => "handedOff",
        }
    }

    pub fn parse(value: &str) -> Self {
        match value {
            "downloading" => Self::Downloading,
            "paused" => Self::Paused,
            "verifying" => Self::Verifying,
            "extracting" => Self::Extracting,
            "handedOff" => Self::HandedOff,
            "completed" => Self::Completed,
            "failed" => Self::Failed,
            "cancelled" => Self::Cancelled,
            _ => Self::Queued,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum DownloadOrigin {
    Direct,
    Itch,
    Steam,
    Gog,
    Epic,
    Amazon,
}

impl DownloadOrigin {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Direct => "direct",
            Self::Itch => "itch",
            Self::Steam => "steam",
            Self::Gog => "gog",
            Self::Epic => "epic",
            Self::Amazon => "amazon",
        }
    }

    pub fn parse(value: &str) -> Self {
        match value {
            "itch" => Self::Itch,
            "amazon" => Self::Amazon,
            "steam" => Self::Steam,
            "gog" => Self::Gog,
            "epic" => Self::Epic,
            _ => Self::Direct,
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
    pub origin: DownloadOrigin,
    pub file_name: Option<String>,
    pub checksum_verified: bool,
    pub install_path: Option<String>,
    pub executable_path: Option<String>,
    /// itch.io extra (soundtrack …) of the game: its file name; null for the game itself.
    pub extra: Option<String>,
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

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum StoreAccountStatus {
    Disconnected,
    Connected,
    NeedsReauth,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ConnectorLoginMode {
    Browser,
    Webview,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum StoreProblemKind {
    PrivateProfile,
    Unavailable,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StoreProblem {
    pub kind: StoreProblemKind,
    pub message: String,
    pub help_url: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StoreAccount {
    pub store: StoreSource,
    pub status: StoreAccountStatus,
    pub connected: bool,
    pub account_name: Option<String>,
    pub game_count: i64,
    pub last_synced_at: Option<String>,
    pub login_mode: ConnectorLoginMode,
    pub problem: Option<StoreProblem>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ConnectorLoginStart {
    pub store: StoreSource,
    pub mode: ConnectorLoginMode,
    pub fallback_url: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ConnectorLoginFinished {
    pub store: StoreSource,
    pub account: Option<StoreAccount>,
    pub error: Option<crate::error::AppErrorPayload>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LocalStoreGame {
    pub store: StoreSource,
    pub store_game_id: String,
    pub title: String,
    pub install_path: String,
    pub executable_path: Option<String>,
    pub size_bytes: Option<i64>,
    pub in_library: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ImportSummary {
    pub store: StoreSource,
    pub added: i64,
    pub updated: i64,
    pub total: i64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum SyncPhase {
    Library,
    Matching,
    Achievements,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SyncProgress {
    pub store: StoreSource,
    pub phase: SyncPhase,
    pub done: i64,
    pub total: i64,
    pub finished: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct AchievementsSyncSummary {
    pub games: i64,
    pub achievements: i64,
    pub newly_unlocked: i64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CoveAchievement {
    pub id: String,
    pub title: String,
    pub description: String,
    pub unlocked_at: Option<String>,
    pub current: i64,
    pub target: i64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub enum PluginPermission {
    #[serde(rename = "library.read")]
    LibraryRead,
    #[serde(rename = "library.add")]
    LibraryAdd,
    #[serde(rename = "catalog.search")]
    CatalogSearch,
    #[serde(rename = "achievements.define")]
    AchievementsDefine,
    #[serde(rename = "ui.addSidebarItem")]
    UiAddSidebarItem,
    #[serde(rename = "ui.notify")]
    UiNotify,
    #[serde(rename = "storage")]
    Storage,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum PluginStatus {
    Active,
    Disabled,
    NeedsApproval,
    Invalid,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PluginInfo {
    pub id: String,
    pub name: String,
    pub version: String,
    pub author: String,
    pub description: String,
    pub permissions: Vec<PluginPermission>,
    pub granted: Vec<PluginPermission>,
    pub enabled: bool,
    pub status: PluginStatus,
    pub error: Option<String>,
    pub removable: bool,
    pub entry_url: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PluginManifestPreview {
    pub id: String,
    pub name: String,
    pub version: String,
    pub author: String,
    pub description: String,
    pub permissions: Vec<PluginPermission>,
    pub already_installed: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PluginGameInput {
    pub title: String,
    pub executable_path: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ComponentVersion {
    pub name: String,
    pub version: String,
    pub status: ServiceStatus,
}

/// Health of a component (sidecars start on first use).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ServiceStatus {
    Running,
    Idle,
    Unavailable,
    NotInstalled,
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
    /// Open in the TV mode.
    #[serde(default)]
    pub start_in_tv_mode: bool,
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
#[allow(clippy::struct_excessive_bools)] // independent switches of the settings page
pub struct LibrarySettings {
    pub scan_folders: Vec<String>,
    pub scan_on_startup: bool,
    /// Folders games are installed into (one per drive, like Steam libraries).
    /// The default one is `downloads.directory`; it is always part of this list.
    #[serde(default)]
    pub game_folders: Vec<String>,
    /// Start Steam with `-silent` (tray only) before a Steam game starts.
    #[serde(default = "default_true")]
    pub steam_silent_start: bool,
    /// Sync Epic cloud saves before and after playing (games Cove installed).
    #[serde(default = "default_true")]
    pub epic_cloud_saves: bool,
    /// Sync GOG cloud saves before and after playing (games Cove installed).
    #[serde(default = "default_true")]
    pub gog_cloud_saves: bool,
    /// Proton build for games without a choice of their own (Linux).
    #[serde(default = "default_proton")]
    pub default_proton: String,
    /// Start Proton through umu-launcher (Steam Linux Runtime, per-game fixes).
    #[serde(default = "default_true")]
    pub use_umu: bool,
}

fn default_true() -> bool {
    true
}

fn default_proton() -> String {
    crate::proton::LATEST.to_owned()
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DownloadSettings {
    pub directory: String,
    pub bandwidth_limit_bytes_per_second: Option<i64>,
    pub max_concurrent: i64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CatalogSettings {
    /// Use the Cove catalogue proxy (no own API keys needed). On by default.
    pub use_cove_service: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AppSettings {
    pub general: GeneralSettings,
    pub appearance: AppearanceSettings,
    pub library: LibrarySettings,
    pub downloads: DownloadSettings,
    pub catalog: CatalogSettings,
    #[serde(default)]
    pub backups: BackupSettings,
    #[serde(default)]
    pub emulators: EmulatorSettings,
    #[serde(default)]
    pub wishlist: WishlistSettings,
}

/// Wishlist prices (`IsThereAnyDeal` through the Cove proxy).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WishlistSettings {
    /// Two-letter country code, e.g. `DE`.
    #[serde(default = "system_country")]
    pub country: String,
    #[serde(default = "enabled")]
    pub price_alerts: bool,
}

fn enabled() -> bool {
    true
}

/// The country of the system locale (`de-DE` → `DE`), else `US`.
pub fn system_country() -> String {
    sys_locale::get_locale()
        .and_then(|l| {
            l.split(['-', '_'])
                .nth(1)
                .map(|c| c.chars().take(2).collect::<String>().to_ascii_uppercase())
        })
        .filter(|c| c.len() == 2 && c.chars().all(|ch| ch.is_ascii_uppercase()))
        .unwrap_or_else(|| "US".to_owned())
}

impl Default for WishlistSettings {
    fn default() -> Self {
        Self {
            country: system_country(),
            price_alerts: true,
        }
    }
}

/// Save backups of every game (save locations from the Ludusavi manifest).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BackupSettings {
    /// Back up after every session.
    #[serde(default = "default_true")]
    pub enabled: bool,
    /// Where backups go; empty = `<app data>/backups`.
    #[serde(default)]
    pub directory: String,
    /// Backups kept per game (1 to 100).
    #[serde(default = "default_keep")]
    pub keep: u32,
}

fn default_keep() -> u32 {
    10
}

impl Default for BackupSettings {
    fn default() -> Self {
        Self {
            enabled: true,
            directory: String::new(),
            keep: default_keep(),
        }
    }
}

/// An emulator for a system and whether it is installed.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EmulatorInfo {
    pub id: String,
    pub name: String,
    pub installed: bool,
}

/// A console Cove can play ROMs of.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SystemInfo {
    pub id: String,
    pub name: String,
    /// ROM file extensions (lower case, without the dot).
    pub extensions: Vec<String>,
    /// `RetroArch` first.
    pub emulators: Vec<EmulatorInfo>,
}

/// Result of scanning the ROM folders.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RomScanResult {
    /// ROMs found in all folders.
    pub found: i64,
    /// New library games.
    pub added: i64,
}

/// One save backup of a game.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BackupInfo {
    /// Folder name (the time), used to restore it.
    pub id: String,
    pub created_at: String,
    pub files: i64,
    pub bytes: i64,
}

/// Save backups of a game, for its page.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GameBackups {
    /// The manifest knows where this game keeps its saves.
    pub known: bool,
    /// Save files found right now.
    pub files: i64,
    pub bytes: i64,
    /// Newest first.
    pub backups: Vec<BackupInfo>,
}

/// Folders with ROMs and the emulator that plays them.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct EmulatorSettings {
    #[serde(default)]
    pub rom_folders: Vec<RomFolder>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RomFolder {
    /// Stable id (chosen by the UI).
    pub id: String,
    /// `snes`, `ps2` … (see `emulators::SYSTEMS`).
    pub system: String,
    pub path: String,
    /// Emulator id of the system (`retroarch`, `dolphin` …) or `custom`.
    pub emulator: String,
    /// Program to run instead of the one found on the system (needed for `custom`).
    #[serde(default)]
    pub program: Option<String>,
    /// `custom`: the arguments, `{rom}` stands for the ROM file.
    #[serde(default)]
    pub arguments: String,
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
    /// True when the provider was unreachable and cached data is shown.
    pub offline: bool,
    /// Recommendation reasons by game id (sort "recommended", empty otherwise).
    #[serde(default)]
    pub reasons: std::collections::BTreeMap<String, String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CatalogGameDetails {
    pub game: Game,
    /// Library game linked to this catalogue entry, if any.
    pub library_game_id: Option<GameId>,
    /// True when the provider was unreachable and cached data is shown.
    pub offline: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProvidersConfigured {
    pub igdb: bool,
    pub rawg: bool,
}

/// Where catalogue data comes from, in fallback order.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum CatalogSource {
    /// Cove catalogue proxy (IGDB data, no keys needed).
    Cove,
    /// IGDB with the user's own Twitch keys.
    Igdb,
    /// RAWG with the user's own API key.
    Rawg,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct CoveServiceStatus {
    /// This build knows a proxy URL.
    pub available: bool,
    /// The user has not switched it off.
    pub enabled: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CatalogStatus {
    /// Sources in the order they are tried (empty = catalogue not available).
    pub sources: Vec<CatalogSource>,
    pub cove_service: CoveServiceStatus,
    /// Own credentials stored in the keyring.
    pub configured: ProvidersConfigured,
    /// False after the last request failed for network reasons (cached data is shown).
    pub online: bool,
}

/// Credentials entered in Settings. Never sent back to the frontend.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "provider", rename_all = "lowercase")]
pub enum CatalogCredentials {
    #[serde(rename_all = "camelCase")]
    Igdb {
        client_id: String,
        client_secret: String,
    },
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

// ───────────── recommender (services/recommender) ─────────────

/// A recommended catalogue game with a short reason for the UI.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Recommendation {
    pub game: Game,
    /// 0–1 relevance score.
    pub score: f64,
    pub reason: String,
}

/// A catalogue game as the recommender sees it (content features + prior).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RecommenderGame {
    pub id: String,
    pub title: String,
    pub genres: Vec<String>,
    pub themes: Vec<String>,
    pub game_modes: Vec<String>,
    pub perspectives: Vec<String>,
    pub keywords: Vec<String>,
    pub developer: String,
    pub release_year: Option<i32>,
    pub rating: Option<i64>,
    pub popularity: i64,
}

/// A library game as the recommender sees it (interaction signals).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RecommenderLibraryGame {
    /// Catalogue id when linked, otherwise the library game id.
    pub game_id: String,
    pub title: String,
    pub playtime_minutes: i64,
    pub last_played_at: Option<String>,
    pub favorite: bool,
    pub achievements: AchievementProgress,
    pub session_count: i64,
}

/// Current UTC time as ISO-8601 with second precision (`2026-11-03T09:52:57Z`).
pub fn now_iso() -> String {
    chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Secs, true)
}

/// A folder the user may install into (install dialog).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FolderInfo {
    pub path: String,
    /// Free space on its drive, null if unknown.
    pub free_bytes: Option<i64>,
    /// Cove may create files there (or create the folder).
    pub writable: bool,
    /// Windows: below "Program Files", which needs admin rights.
    pub needs_admin: bool,
}

/// Sizes shown in the install dialog before a download starts.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct InstallPreview {
    pub title: String,
    pub download_bytes: Option<i64>,
    /// Size after the install; null when it is only known after unpacking.
    pub install_bytes: Option<i64>,
    /// Why Cove can't install it and the store's launcher must (anti-cheat, third-party launcher).
    pub launcher_only: Option<String>,
    /// Owned DLCs that can be installed with the game (GOG, Epic); sizes above are the game alone.
    pub dlcs: Vec<DlcOption>,
}

/// One DLC in the install dialog.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DlcOption {
    /// GOG product id or Epic app name.
    pub id: String,
    pub title: String,
    pub download_bytes: Option<i64>,
    pub install_bytes: Option<i64>,
    /// Ticked: installed with the game (remembers the last choice for this game).
    pub selected: bool,
}

/// Where the install dialog puts a game.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct InstallTarget {
    /// Games folder; Cove creates a subfolder named after the game.
    pub folder: String,
    /// "Diesen Ordner als Standard verwenden".
    pub make_default: bool,
    /// DLCs left out (ids from `InstallPreview.dlcs`); kept for later updates.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub excluded_dlcs: Vec<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum MoveStatus {
    Copying,
    Verifying,
    Completed,
    Failed,
}

/// Event `game-move-progress` while a game folder is moved.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GameMoveProgress {
    pub game_id: GameId,
    pub status: MoveStatus,
    pub copied_bytes: i64,
    pub total_bytes: i64,
    pub error: Option<String>,
}

/// Update state of a store game Cove installed.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GameUpdateInfo {
    pub available: bool,
    pub installed_version: String,
    pub latest_version: String,
}

/// What `game_uninstall` did.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum UninstallMethod {
    /// Cove deleted the game folder.
    Removed,
    /// Steam asks the user and removes the game; Cove notices it in the app manifests.
    Steam,
}

/// A DLC of a store game as the store sees it (Steam: managed in Steam itself).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StoreDlc {
    pub id: String,
    pub title: String,
    /// Installed on this computer.
    pub installed: bool,
    /// GOG/Epic: no files of its own, the game unlocks it through the login.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub unlock_only: bool,
}

/// One of the user's own collections ("Mit Freunden", "Angefangen" ...).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Collection {
    pub id: String,
    pub name: String,
    /// The library games in it, oldest member first.
    pub game_ids: Vec<GameId>,
}

/// Where a Proton build comes from.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ProtonKind {
    /// GE-Proton: Cove downloads and updates it.
    Ge,
    /// Installed by Steam (Valve's Proton or a build in `compatibilitytools.d`).
    Steam,
}

/// A Proton build a game can use.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProtonVersion {
    /// GE-Proton release name, or `steam:<folder>`.
    pub id: String,
    pub name: String,
    pub kind: ProtonKind,
    pub installed: bool,
    /// Size of the download, for GE-Proton releases.
    pub download_bytes: Option<i64>,
    pub released_at: Option<String>,
    /// Library games that start with this build (directly or as "latest"/default).
    pub games: i64,
    /// Cove may delete it: a GE-Proton build Cove installed that no game uses.
    pub removable: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ProtonPhase {
    Downloading,
    Verifying,
    Unpacking,
    Done,
    Failed,
    /// The user cancelled the install.
    Cancelled,
}

/// Event `proton-progress`: one GE-Proton install, at most about 4 per second.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProtonProgress {
    pub id: String,
    pub phase: ProtonPhase,
    pub downloaded_bytes: i64,
    pub total_bytes: i64,
    /// User-facing reason when `phase` is `failed`.
    pub error: Option<String>,
}

/// Everything the Proton settings show.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProtonOverview {
    /// GE-Proton (newest first), then Steam's builds.
    pub versions: Vec<ProtonVersion>,
    /// The build "latest" stands for right now.
    pub latest: Option<String>,
    /// When the list of GE-Proton releases was last loaded.
    pub checked_at: Option<String>,
    /// The release list could not be loaded (only installed builds are listed).
    pub offline: bool,
    pub installing: Vec<ProtonProgress>,
    pub umu: UmuStatus,
}

/// Where the umu-launcher Cove uses comes from.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum UmuSource {
    /// Installed with the system's package manager (`umu-run` in `PATH`).
    System,
    /// Downloaded by Cove (zipapp from the GitHub releases).
    Cove,
}

/// umu-launcher: runs Proton inside the Steam Linux Runtime, like Steam does.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UmuStatus {
    /// `library.useUmu`.
    pub enabled: bool,
    /// null = not on this machine yet.
    pub source: Option<UmuSource>,
    /// Version of Cove's copy (null for the system's).
    pub version: Option<String>,
    /// Newest release, when the list could be loaded.
    pub latest: Option<String>,
    /// `python3` is there (Cove's copy needs it).
    pub python: bool,
}

/// A game's Proton prefix (Linux).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PrefixInfo {
    /// The prefix has been created (the game started at least once).
    pub exists: bool,
    pub size_bytes: Option<i64>,
    /// `winetricks` is installed.
    pub winetricks: bool,
}

/// What to do with a game's Proton prefix.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum PrefixAction {
    /// Open the prefix folder in the file manager.
    Open,
    /// Delete the prefix; the next start creates a fresh one.
    Reset,
    /// Start winetricks for the prefix.
    Winetricks,
}

/// How long a game takes, in seconds (IGDB "time to beat", from player reports).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TimeToBeat {
    /// Main story.
    pub hastily: Option<i64>,
    /// Main story and extras.
    pub normally: Option<i64>,
    /// Everything (100 %).
    pub completely: Option<i64>,
    /// Number of player reports.
    pub count: i64,
}

/// `ProtonDB` tier: how well a Windows game runs on Linux through Proton.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ProtonTier {
    Platinum,
    Gold,
    Silver,
    Bronze,
    Borked,
}

/// `ProtonDB`'s community rating of a game.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProtonRating {
    pub tier: ProtonTier,
    /// Number of user reports behind the tier.
    pub reports: u32,
    /// The Steam app the rating belongs to (`ProtonDB` rates Steam versions).
    pub steam_app_id: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StoreDlcList {
    pub dlcs: Vec<StoreDlc>,
    /// All DLCs of the game; the list is cut off for games with hundreds.
    pub total: i64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DownloadProgress<'a> {
    pub id: &'a str,
    pub status: DownloadStatus,
    pub downloaded_bytes: i64,
    pub total_bytes: i64,
    pub bytes_per_second: i64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DirectDownloadInput {
    pub url: String,
    pub title: Option<String>,
    pub sha256: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ItchStatus {
    pub connected: bool,
    pub user_name: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ItchGame {
    pub game_id: i64,
    pub title: String,
    pub cover_url: Option<String>,
    pub download_key_id: Option<i64>,
    pub url: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ItchUpload {
    pub upload_id: i64,
    pub file_name: String,
    pub size_bytes: Option<i64>,
    pub platforms: Vec<Platform>,
    /// Not the game itself (soundtrack, artbook …): offered as an extra.
    pub extra: bool,
}

// ───────────── updates ─────────────

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdateInfo {
    pub version: String,
    pub notes: Option<String>,
    pub date: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdateCheck {
    pub current_version: String,
    /// `None` when Cove is up to date.
    pub available: Option<UpdateInfo>,
    /// `false` for `.deb`/`.rpm` installs: the package manager updates those.
    pub can_install: bool,
    pub install_hint: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdateProgress {
    pub downloaded_bytes: u64,
    pub total_bytes: Option<u64>,
}
