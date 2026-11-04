/**
 * Every Tauri command of the Rust core: name → arguments → result.
 * Argument keys are camelCase (Tauri maps them to snake_case Rust params).
 * Keep in sync with `apps/desktop/src-tauri/src/commands.rs`.
 */
import type {
  AboutInfo,
  Achievement,
  AchievementUnlock,
  AppSettings,
  DownloadItem,
  DownloadStatus,
  Game,
  GameId,
  LaunchResult,
  LibraryItem,
  LibraryUpdate,
  ManualGameInput,
  PlaytimeStats,
  PluginInfo,
  RunningGame,
  ScanCandidate,
  StoreAccount,
  ConnectableStore,
  UserProfile,
  GameStateEvent,
  CatalogCredentials,
  CatalogFilter,
  CatalogGameDetails,
  CatalogPage,
  CatalogProviderId,
  CatalogSort,
  CatalogStatus,
  LibraryMatchEvent,
  MatchCandidate,
  Recommendation,
  ConnectorLoginStart,
  DirectDownloadInput,
  DownloadProgress,
  ItchGame,
  ItchStatus,
  ItchUpload,
  ConnectorLoginFinished,
  LocalStoreGame,
  ImportSummary,
  SyncProgress,
  AchievementsSyncSummary,
  CoveAchievement,
} from "./models.ts";

type NoArgs = Record<string, never>;

export interface CoveCommands {
  library_list: { args: NoArgs; result: LibraryItem[] };
  library_get: { args: { gameId: GameId }; result: LibraryItem | null };
  library_add_manual: { args: { input: ManualGameInput }; result: LibraryItem };
  library_add_from_catalog: { args: { game: Game }; result: LibraryItem };
  library_remove: { args: { gameId: GameId }; result: null };
  library_update: { args: { gameId: GameId; patch: LibraryUpdate }; result: LibraryItem };
  library_scan_folder: { args: { path: string }; result: ScanCandidate[] };
  library_achievements: { args: { gameId: GameId }; result: Achievement[] };
  library_playtime: { args: { gameId: GameId }; result: PlaytimeStats };
  achievements_recent: { args: { limit: number }; result: AchievementUnlock[] };
  game_launch: { args: { gameId: GameId }; result: LaunchResult };
  game_stop: { args: { gameId: GameId }; result: null };
  game_running_list: { args: NoArgs; result: RunningGame[] };
  settings_get: { args: NoArgs; result: AppSettings };
  settings_update: { args: { settings: AppSettings }; result: AppSettings };
  connectors_list: { args: NoArgs; result: StoreAccount[] };
  /** Opens the login (browser or own window); the result arrives as `connector-login-finished`. */
  connector_login_start: { args: { store: ConnectableStore }; result: ConnectorLoginStart };
  /** Finishes a login with a pasted code (Epic "Alternativer Login"). */
  connector_login_complete: { args: { store: ConnectableStore; code: string }; result: StoreAccount };
  connector_login_cancel: { args: { store: ConnectableStore }; result: null };
  connector_refresh: { args: { store: ConnectableStore }; result: StoreAccount };
  connector_logout: { args: { store: ConnectableStore; keepGames: boolean }; result: StoreAccount };
  /** Installed games found on this computer (no login needed); null = all stores. */
  connector_detect_local: { args: { store: ConnectableStore | null }; result: LocalStoreGame[] };
  /** Imports owned games (when signed in) and installed games of one store. */
  connector_import_library: { args: { store: ConnectableStore }; result: ImportSummary };
  achievements_sync: { args: { store: ConnectableStore | null }; result: AchievementsSyncSummary };
  cove_achievements_list: { args: NoArgs; result: CoveAchievement[] };
  plugins_list: { args: NoArgs; result: PluginInfo[] };
  plugins_set_enabled: { args: { id: string; enabled: boolean }; result: PluginInfo };
  downloads_list: { args: NoArgs; result: DownloadItem[] };
  /** Store games: hands the installation over to Steam/GOG/Epic. */
  downloads_enqueue: { args: { gameId: GameId }; result: null };
  downloads_add_url: { args: { input: DirectDownloadInput }; result: DownloadItem };
  downloads_add_itch: {
    args: { gameId: number; uploadId: number; downloadKeyId: number | null; title: string };
    result: DownloadItem;
  };
  itch_status: { args: NoArgs; result: ItchStatus };
  itch_set_api_key: { args: { apiKey: string }; result: ItchStatus };
  itch_clear_api_key: { args: NoArgs; result: ItchStatus };
  itch_owned_games: { args: NoArgs; result: ItchGame[] };
  /** A free game by its page, e.g. https://someone.itch.io/game */
  itch_lookup: { args: { url: string }; result: ItchGame };
  itch_uploads: { args: { gameId: number; downloadKeyId: number | null }; result: ItchUpload[] };
  downloads_set_status: { args: { id: string; status: DownloadStatus }; result: null };
  downloads_reorder: { args: { orderedIds: string[] }; result: null };
  downloads_clear_history: { args: NoArgs; result: null };
  system_profile: { args: NoArgs; result: UserProfile };
  system_about: { args: NoArgs; result: AboutInfo };
  catalog_status: { args: NoArgs; result: CatalogStatus };
  catalog_set_credentials: { args: { credentials: CatalogCredentials }; result: CatalogStatus };
  catalog_clear_credentials: { args: { provider: CatalogProviderId }; result: CatalogStatus };
  catalog_search: { args: { query: string }; result: CatalogPage };
  catalog_browse: { args: { filter: CatalogFilter; sort: CatalogSort; page: number }; result: CatalogPage };
  catalog_game_details: { args: { gameId: string }; result: CatalogGameDetails };
  catalog_similar: { args: { gameId: string }; result: CatalogPage };
  library_match_candidates: { args: { gameId: GameId }; result: MatchCandidate[] };
  library_link_catalog: { args: { gameId: GameId; catalogId: string }; result: LibraryItem };
  library_reject_match: { args: { gameId: GameId }; result: LibraryItem };
  library_rematch: { args: { gameId: GameId }; result: null };
  recommendations_for_user: { args: { limit: number }; result: Recommendation[] };
  recommendations_discover: { args: { limit: number }; result: Recommendation[] };
  recommendations_similar: { args: { gameId: string; limit: number }; result: Recommendation[] };
  recommendation_explain: { args: { gameId: string }; result: string };
}

export type CoveCommand = keyof CoveCommands;

/** Events emitted by the core (name → payload). */
export interface CoveEvents {
  "game-state-changed": GameStateEvent;
  "downloads-changed": DownloadItem[];
  "download-progress": DownloadProgress;
  "library-match-changed": LibraryMatchEvent;
  "connector-login-finished": ConnectorLoginFinished;
  "store-sync-progress": SyncProgress;
  "achievement-unlocked": CoveAchievement;
}

export type CoveEvent = keyof CoveEvents;
