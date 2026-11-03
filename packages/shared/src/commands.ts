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
  sources_list: { args: NoArgs; result: StoreAccount[] };
  sources_connect: { args: { store: ConnectableStore }; result: StoreAccount };
  plugins_list: { args: NoArgs; result: PluginInfo[] };
  plugins_set_enabled: { args: { id: string; enabled: boolean }; result: PluginInfo };
  downloads_list: { args: NoArgs; result: DownloadItem[] };
  downloads_enqueue: { args: { gameId: GameId }; result: null };
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
}

export type CoveCommand = keyof CoveCommands;

/** Events emitted by the core (name → payload). */
export interface CoveEvents {
  "game-state-changed": GameStateEvent;
  "downloads-changed": DownloadItem[];
  "library-match-changed": LibraryMatchEvent;
}

export type CoveEvent = keyof CoveEvents;
