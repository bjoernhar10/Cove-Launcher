/**
 * Every Tauri command of the Rust core: name → arguments → result.
 * Argument keys are camelCase (Tauri maps them to snake_case Rust params).
 * Keep in sync with `apps/desktop/src-tauri/src/commands.rs`.
 */
import type {
  AboutInfo,
  UpdateCheck,
  UpdateProgress,
  Achievement,
  AchievementUnlock,
  Collection,
  CollectionId,
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
  ProtonOverview,
  ProtonProgress,
  ProtonRating,
  ProtonVersionId,
  PrefixAction,
  BackupInfo,
  GameBackups,
  RomScanResult,
  SystemInfo,
  GameTools,
  PrefixInfo,
  TimeToBeat,
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
  PluginGameInput,
  PluginManifestPreview,
  ItchStatus,
  ItchUpload,
  ConnectorLoginFinished,
  LocalStoreGame,
  ImportSummary,
  SyncProgress,
  AchievementsSyncSummary,
  CoveAchievement,
  FolderInfo,
  GameMoveProgress,
  GameUpdateInfo,
  InstallPreview,
  InstallTarget,
  StoreDlcList,
  UninstallMethod,
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
  /** The user's own collections, by name. */
  collections_list: { args: NoArgs; result: Collection[] };
  collections_create: { args: { name: string }; result: Collection };
  collections_rename: { args: { collectionId: CollectionId; name: string }; result: Collection };
  /** Deletes the collection; its games stay in the library. */
  collections_delete: { args: { collectionId: CollectionId }; result: null };
  /** Puts a library game into a collection (`member: true`) or takes it out. */
  collections_set_game: { args: { collectionId: CollectionId; gameId: GameId; member: boolean }; result: null };
  library_achievements: { args: { gameId: GameId }; result: Achievement[] };
  library_playtime: { args: { gameId: GameId }; result: PlaytimeStats };
  achievements_recent: { args: { limit: number }; result: AchievementUnlock[] };
  game_launch: { args: { gameId: GameId }; result: LaunchResult };
  game_stop: { args: { gameId: GameId }; result: null };
  game_running_list: { args: NoArgs; result: RunningGame[] };
  /** Steam: opens Steam's uninstall dialog. Games Cove installed: deletes the game folder. */
  game_uninstall: { args: { gameId: GameId }; result: UninstallMethod };
  /** DLCs of a store game: Steam lists all of them, GOG/Epic the owned ones (installed ones marked). */
  game_dlcs: { args: { gameId: GameId }; result: StoreDlcList };
  /** Opens the game in Steam, where its DLCs are ticked under Properties → DLC. */
  game_manage_dlcs: { args: { gameId: GameId }; result: null };
  /** ProtonDB's rating of the game (how well it runs on Linux); null when there is none. */
  game_proton_rating: { args: { gameId: GameId }; result: ProtonRating | null };
  /** Linux: GE-Proton releases, installed builds and Steam's builds. */
  proton_list: { args: NoArgs; result: ProtonOverview };
  /** Loads the release list again; a new "latest" is installed in the background. */
  proton_check: { args: NoArgs; result: ProtonOverview };
  /** Starts installing a GE-Proton build or umu-launcher (`"umu"`; progress: `proton-progress`). */
  proton_install: { args: { versionId: ProtonVersionId }; result: null };
  /** Deletes a GE-Proton build no game uses. */
  proton_remove: { args: { versionId: ProtonVersionId }; result: null };
  /** Save backups of a game: known to the manifest, files found now, backups. */
  game_backups: { args: { gameId: GameId }; result: GameBackups };
  /** Backs a game's saves up now; null when nothing changed since the last backup. */
  game_backup_now: { args: { gameId: GameId }; result: BackupInfo | null };
  /** Puts a backup back (the current saves are backed up first); returns the files restored. */
  game_backup_restore: { args: { gameId: GameId; backupId: string }; result: number };
  /** The systems Cove plays ROMs of, with their emulators. */
  emulators_systems: { args: NoArgs; result: SystemInfo[] };
  /** Adds the ROMs of every ROM folder to the library (a collection per system). */
  emulators_scan: { args: NoArgs; result: RomScanResult };
  /** Linux: which game tools (MangoHud, GameMode, gamescope) are installed. */
  system_game_tools: { args: NoArgs; result: GameTools };
  /** Stops a running install (a GE-Proton build or `"umu"`). */
  proton_cancel: { args: { versionId: ProtonVersionId }; result: null };
  /** Linux: a game's Proton prefix. */
  game_prefix_info: { args: { gameId: GameId }; result: PrefixInfo };
  /** Linux: opens, resets or runs winetricks on a game's Proton prefix. */
  game_prefix_action: { args: { gameId: GameId; action: PrefixAction }; result: null };
  /** Moves the game folder into another games folder; progress arrives as `game-move-progress`. */
  game_move: { args: { gameId: GameId; folder: string }; result: null };
  /** Installed and newest version of a GOG or Epic game Cove installed. */
  game_update_check: { args: { gameId: GameId }; result: GameUpdateInfo };
  /** Queues the update (only changed parts are downloaded). */
  game_update: { args: { gameId: GameId }; result: null };
  /** "DLCs ändern" (GOG/Epic games Cove installed): stores the choice and queues an update. */
  game_set_dlcs: { args: { gameId: GameId; excludedDlcs: string[] }; result: null };
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
  /** Grants the requested permissions (after the user confirmed them) and enables the plugin. */
  plugins_approve: { args: { id: string }; result: PluginInfo };
  /** Reads `plugin.json` of a folder for the install dialog. */
  plugins_inspect: { args: { path: string }; result: PluginManifestPreview };
  /** Copies the folder into Cove's plugin folder and grants the confirmed permissions. */
  plugins_install: { args: { path: string }; result: PluginInfo };
  plugins_uninstall: { args: { id: string }; result: null };
  plugins_open_folder: { args: NoArgs; result: null };
  /** Plugin API (called by the plugin host for a running plugin). */
  plugin_storage_get: { args: { pluginId: string; key: string }; result: unknown };
  plugin_storage_set: { args: { pluginId: string; key: string; value: unknown }; result: null };
  plugin_storage_remove: { args: { pluginId: string; key: string }; result: null };
  plugin_storage_keys: { args: { pluginId: string }; result: string[] };
  plugin_define_achievements: { args: { pluginId: string; achievements: unknown[] }; result: number };
  plugin_library_add: { args: { pluginId: string; game: PluginGameInput }; result: LibraryItem };
  downloads_list: { args: NoArgs; result: DownloadItem[] };
  /**
   * Store games: GOG and Epic games are installed by Cove into `target` (install dialog).
   * Steam installs, and Epic installs without `target` (anti-cheat …), go to the store's client.
   */
  downloads_enqueue: { args: { gameId: GameId; target: InstallTarget | null }; result: null };
  /** Sizes for the install dialog of a GOG or Epic game (reads the store's build manifest). */
  downloads_preview_store: { args: { gameId: GameId }; result: InstallPreview };
  /** Sizes for the install dialog (asks the server for the file size). */
  downloads_preview_url: { args: { input: DirectDownloadInput }; result: InstallPreview };
  downloads_add_url: { args: { input: DirectDownloadInput; target: InstallTarget }; result: DownloadItem };
  downloads_add_itch: {
    /** `extras`: upload ids of extras (soundtrack …) for the game's "Extras" folder. */
    args: { gameId: number; uploadId: number; downloadKeyId: number | null; title: string; target: InstallTarget; extras: number[] };
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
  /** Native folder picker; null when the user cancels. */
  system_pick_folder: { args: { defaultPath: string | null }; result: string | null };
  /** Free space, write access and the "Program Files" warning for a folder. */
  system_folder_info: { args: { path: string }; result: FolderInfo };
  /** Asks the release server for a newer version (signed `latest.json`). */
  updates_check: { args: NoArgs; result: UpdateCheck };
  /** Downloads, verifies and installs the update, then restarts Cove. */
  updates_install: { args: NoArgs; result: null };
  catalog_status: { args: NoArgs; result: CatalogStatus };
  catalog_set_credentials: { args: { credentials: CatalogCredentials }; result: CatalogStatus };
  catalog_clear_credentials: { args: { provider: CatalogProviderId }; result: CatalogStatus };
  catalog_search: { args: { query: string }; result: CatalogPage };
  catalog_browse: { args: { filter: CatalogFilter; sort: CatalogSort; page: number }; result: CatalogPage };
  catalog_game_details: { args: { gameId: string }; result: CatalogGameDetails };
  catalog_similar: { args: { gameId: string }; result: CatalogPage };
  /** How long games take, per catalogue id (`igdb-…`); ids without data are left out. */
  catalog_time_to_beat: { args: { catalogIds: string[] }; result: Record<string, TimeToBeat> };
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
  "update-progress": UpdateProgress;
  "game-move-progress": GameMoveProgress;
  "proton-progress": ProtonProgress;
}

export type CoveEvent = keyof CoveEvents;
