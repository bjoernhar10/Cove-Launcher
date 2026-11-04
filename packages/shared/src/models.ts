/**
 * Domain models shared by UI, core and services.
 *
 * Every type here is exported as JSON-Schema to `schemas/models.schema.json`
 * (`npm run schemas -w @cove/shared`). Backends must produce exactly these shapes.
 *
 * Conventions:
 * - Timestamps are ISO-8601 strings (UTC), dates are `YYYY-MM-DD`.
 * - Durations are whole minutes, sizes are bytes, speeds are bytes/second.
 * - Missing values are `null`, never omitted.
 */

/** Stable game identifier (store-independent, assigned by the core). */
export type GameId = string;

/** ISO-8601 timestamp, e.g. `2026-11-03T09:52:57Z`. */
export type IsoDateTime = string;

/** Calendar date, e.g. `2026-11-03`. */
export type IsoDate = string;

export type StoreSource = "steam" | "gog" | "epic" | "manual";

/** Stores that support account connections (everything except manual). */
export type ConnectableStore = Exclude<StoreSource, "manual">;

export type Platform = "windows" | "linux" | "macos";

export type Genre =
  | "action"
  | "adventure"
  | "rpg"
  | "strategy"
  | "simulation"
  | "puzzle"
  | "platformer"
  | "shooter"
  | "racing"
  | "sports"
  | "horror"
  | "indie";

/** Reference to an image. `url` is null when no artwork is available yet. */
export interface ImageRef {
  url: string | null;
  /** Accessible description of the image. */
  alt: string;
}

export interface GameArtwork {
  /** Portrait cover, 3:4. */
  cover: ImageRef;
  /** Wide header / hero image, 16:9 or wider. */
  header: ImageRef;
  screenshots: ImageRef[];
}

/** Catalogue metadata for a game (independent of whether the user owns it). */
export interface Game {
  id: GameId;
  title: string;
  /** One-sentence teaser. */
  summary: string;
  /** Full description, plain text with `\n\n` between paragraphs. */
  description: string;
  genres: Genre[];
  platforms: Platform[];
  /** Null when unknown (e.g. unreleased or manually added games). */
  releaseDate: IsoDate | null;
  developer: string;
  publisher: string;
  /** Aggregated rating 0–100, null if unrated. */
  rating: number | null;
  /** Relative popularity 0–100, used for "Beliebt" sorting. */
  popularity: number;
  artwork: GameArtwork;
  /** Extra descriptors for the recommender; null when the source does not provide them. */
  features: GameFeatures | null;
}

export interface GameFeatures {
  themes: string[];
  gameModes: string[];
  perspectives: string[];
  keywords: string[];
}

export type CompatibilityTool = "native" | "proton" | "proton-experimental" | "wine";

export interface EnvVar {
  key: string;
  value: string;
}

export interface LaunchOptions {
  /** Extra command-line arguments, as typed by the user. */
  arguments: string;
  /** Linux only: how Windows binaries are run. */
  compatibilityTool: CompatibilityTool;
  env: EnvVar[];
}

export interface AchievementProgress {
  unlocked: number;
  total: number;
}

/** A game in the user's library. */
export interface LibraryEntry {
  gameId: GameId;
  source: StoreSource;
  /** Store-specific id (e.g. Steam app id) used for URL-scheme launches; null for manual games. */
  storeGameId: string | null;
  installed: boolean;
  /** Absolute path of the install directory, null if not installed. */
  installPath: string | null;
  /** Absolute path of the file Cove starts (.exe, binary, AppImage, .desktop, script). */
  executablePath: string | null;
  /** Size on disk in bytes, null if not installed. */
  installSizeBytes: number | null;
  favorite: boolean;
  playtimeMinutes: number;
  lastPlayedAt: IsoDateTime | null;
  addedAt: IsoDateTime;
  achievements: AchievementProgress;
  launchOptions: LaunchOptions;
  /** Linked catalogue entry (e.g. `igdb-1942`), null if not matched. */
  catalogId: string | null;
  matchStatus: MatchStatus;
}

/**
 * Catalogue matching state of a library game:
 * none = not matched (yet) · pending = suggestions wait for the user ·
 * matched = metadata applied · rejected = user declined all suggestions.
 */
export type MatchStatus = "none" | "pending" | "matched" | "rejected";

/** Library entry joined with its game metadata — what list views render. */
export interface LibraryItem {
  entry: LibraryEntry;
  game: Game;
}

export interface Achievement {
  id: string;
  gameId: GameId;
  title: string;
  description: string;
  /** Null while locked. */
  unlockedAt: IsoDateTime | null;
  /** Share of players who unlocked it, 0–100; null if unknown. */
  rarityPercent: number | null;
  /** Hidden achievements show no description until unlocked. */
  hidden: boolean;
  /** Store icon (the coloured one when unlocked, the grey one when locked). */
  icon: ImageRef;
}

export interface AchievementUnlock {
  achievement: Achievement;
  game: Game;
}

export interface PlaytimeDay {
  date: IsoDate;
  minutes: number;
}

export interface PlaytimeStats {
  gameId: GameId;
  totalMinutes: number;
  last14DaysMinutes: number;
  sessionCount: number;
  /** One entry per day, oldest first (no gaps). */
  days: PlaytimeDay[];
}

export type DownloadStatus = "downloading" | "paused" | "queued" | "completed" | "failed" | "cancelled";

export interface Download {
  id: string;
  gameId: GameId;
  source: StoreSource;
  status: DownloadStatus;
  totalBytes: number;
  downloadedBytes: number;
  /** Current speed, 0 when not downloading. */
  bytesPerSecond: number;
  /** Position in the queue (0-based) for active/queued items, null otherwise. */
  queuePosition: number | null;
  queuedAt: IsoDateTime;
  finishedAt: IsoDateTime | null;
  /** Human-readable error for failed downloads. */
  error: string | null;
}

/** Download joined with its game metadata. */
export interface DownloadItem {
  download: Download;
  game: Game;
}

export type CatalogSort = "recommended" | "popular" | "newest" | "title";

export interface YearRange {
  from: number;
  to: number;
}

export interface CatalogFilter {
  query: string;
  genres: Genre[];
  platforms: Platform[];
  releaseYears: YearRange | null;
  /** Minimum rating 0–100, null = any. */
  minRating: number | null;
}

export type CatalogProviderId = "igdb" | "rawg";

/** One page of catalogue results. */
export interface CatalogPage {
  items: Game[];
  /** 0-based page index. */
  page: number;
  hasMore: boolean;
  /** Total number of matches, null if the provider does not report it. */
  total: number | null;
  /** True when the provider was unreachable and cached data is shown. */
  offline: boolean;
  /** Recommendation reasons by game id (sort "recommended" with the recommender running; else empty). */
  reasons: Record<string, string>;
}

export interface CatalogGameDetails {
  game: Game;
  /** Library game linked to this catalogue entry, if any. */
  libraryGameId: GameId | null;
  offline: boolean;
}

export interface ProvidersConfigured {
  igdb: boolean;
  rawg: boolean;
}

/** Where catalogue data comes from, in fallback order. */
export type CatalogSource = "cove" | "igdb" | "rawg";

export interface CoveServiceStatus {
  /** This build knows a proxy URL. */
  available: boolean;
  /** The user has not switched it off. */
  enabled: boolean;
}

export interface CatalogStatus {
  /** Sources in the order they are tried; empty = catalogue not available. */
  sources: CatalogSource[];
  coveService: CoveServiceStatus;
  /** Own credentials stored in the keyring. */
  configured: ProvidersConfigured;
  /** False after the last request failed for network reasons. */
  online: boolean;
}

/** Credentials entered in Settings; stored in the OS keyring, never returned to the UI. */
export type CatalogCredentials =
  | { provider: "igdb"; clientId: string; clientSecret: string }
  | { provider: "rawg"; apiKey: string };

export interface MatchCandidate {
  game: Game;
  /** Title similarity 0–1. */
  score: number;
}

/** Payload of the `library-match-changed` event. */
export interface LibraryMatchEvent {
  gameId: GameId;
  status: MatchStatus;
}

/** Cursor-based page. `nextCursor` is null on the last page. */
export interface Page<T> {
  items: T[];
  nextCursor: string | null;
  total: number;
}

export interface Recommendation {
  game: Game;
  /** 0–1 relevance score from the recommender. */
  score: number;
  /** Short human-readable reason, e.g. "Weil du Tidewater gespielt hast". */
  reason: string;
}

export interface UserProfile {
  id: string;
  displayName: string;
  avatarUrl: string | null;
}

export type StoreAccountStatus = "disconnected" | "connected" | "needsReauth";

/** How the login page is shown: system browser (loopback redirect) or an own window. */
export type ConnectorLoginMode = "browser" | "webview";

export interface StoreProblem {
  kind: "privateProfile" | "unavailable";
  message: string;
  /** Where the user can fix it (e.g. Steam privacy settings). */
  helpUrl: string | null;
}

export interface StoreAccount {
  store: ConnectableStore;
  status: StoreAccountStatus;
  /** True while a login is stored (also when it needs to be renewed). */
  connected: boolean;
  /** Account display name when connected. */
  accountName: string | null;
  /** Number of games imported from this store. */
  gameCount: number;
  lastSyncedAt: IsoDateTime | null;
  loginMode: ConnectorLoginMode;
  problem: StoreProblem | null;
}

/** Answer of `connector_login_start`; the result arrives as `connector-login-finished`. */
export interface ConnectorLoginStart {
  store: ConnectableStore;
  mode: ConnectorLoginMode;
  /**
   * Alternative login (Epic): open this page in any browser and paste the
   * shown code into Cove (`connector_login_complete`).
   */
  fallbackUrl: string | null;
}

/** Event `connector-login-finished` */
export interface ConnectorLoginFinished {
  store: ConnectableStore;
  account: StoreAccount | null;
  /** Null on success and when the user cancelled. */
  error: AppErrorPayload | null;
}

/** A game installed on this computer, found without login. */
export interface LocalStoreGame {
  store: ConnectableStore;
  storeGameId: string;
  title: string;
  installPath: string;
  executablePath: string | null;
  sizeBytes: number | null;
  inLibrary: boolean;
}

export interface ImportSummary {
  store: ConnectableStore;
  added: number;
  updated: number;
  total: number;
}

export type SyncPhase = "library" | "matching" | "achievements";

/** Event `store-sync-progress` while a background sync runs. */
export interface SyncProgress {
  store: ConnectableStore;
  phase: SyncPhase;
  done: number;
  total: number;
  finished: boolean;
}

export interface AchievementsSyncSummary {
  /** Games whose achievements were read. */
  games: number;
  achievements: number;
  newlyUnlocked: number;
}

/** Cove's own achievements, evaluated locally (definitions: JSON, plugins can add more). */
export interface CoveAchievement {
  id: string;
  title: string;
  description: string;
  unlockedAt: IsoDateTime | null;
  current: number;
  target: number;
}

export interface PluginInfo {
  id: string;
  name: string;
  version: string;
  author: string;
  description: string;
  enabled: boolean;
}

export type ThemePreference = "system" | "light" | "dark";

export type AccentColor = "blue" | "teal" | "sage" | "plum" | "clay";

export type Language = "de" | "en";

export type StartPage = "home" | "library";

export interface GeneralSettings {
  language: Language;
  launchOnStartup: boolean;
  minimizeToTray: boolean;
  startPage: StartPage;
}

export interface AppearanceSettings {
  theme: ThemePreference;
  accent: AccentColor;
}

export interface LibrarySettings {
  scanFolders: string[];
  scanOnStartup: boolean;
}

export interface DownloadSettings {
  directory: string;
  /** Bandwidth limit in bytes/second, null = unlimited. */
  bandwidthLimitBytesPerSecond: number | null;
  maxConcurrent: number;
}

export interface CatalogSettings {
  /** Use the Cove catalogue service (no own API keys needed). */
  useCoveService: boolean;
}

export interface AppSettings {
  general: GeneralSettings;
  appearance: AppearanceSettings;
  library: LibrarySettings;
  downloads: DownloadSettings;
  catalog: CatalogSettings;
}

/** Input for adding a game manually. */
export interface ManualGameInput {
  title: string;
  executablePath: string;
  /** Known size on disk (from a folder scan), null if unknown. */
  sizeBytes: number | null;
}

/** Partial update of a library entry; omitted fields stay unchanged. */
export interface LibraryUpdate {
  title?: string;
  favorite?: boolean;
  executablePath?: string;
  launchOptions?: LaunchOptions;
}

export type ExecutableKind = "windows-exe" | "linux-binary" | "appimage" | "desktop-entry" | "script";

/** A game found while scanning a folder (suggestion — the user confirms). */
export interface ScanCandidate {
  title: string;
  executablePath: string;
  /** Size of the game folder in bytes. */
  sizeBytes: number;
  kind: ExecutableKind;
  alreadyInLibrary: boolean;
}

export type GameState = "running" | "stopped";

/** Payload of the `game-state-changed` event. */
export interface GameStateEvent {
  gameId: GameId;
  state: GameState;
  startedAt: IsoDateTime;
  /** Length of the finished session in seconds; null while running. */
  sessionSeconds: number | null;
}

export interface RunningGame {
  gameId: GameId;
  startedAt: IsoDateTime;
}

export type LaunchMethod = "process" | "url";

export interface LaunchResult {
  gameId: GameId;
  method: LaunchMethod;
  /** False when Cove cannot observe the game (started via a store client without known install folder). */
  tracked: boolean;
}

export type AppErrorKind =
  | "database"
  | "migration"
  | "io"
  | "serialization"
  | "notInLibrary"
  | "alreadyInLibrary"
  | "invalidInput"
  | "alreadyRunning"
  | "notRunning"
  | "launch"
  | "compatToolNotFound"
  | "offline"
  | "catalogNotConfigured"
  | "catalogAuth"
  | "catalogProvider"
  | "keyring"
  | "recommenderUnavailable"
  | "recommender"
  | "loginFailed"
  | "reauthRequired"
  | "privateProfile"
  | "storeUnavailable"
  | "connectorsUnavailable";

/** Error shape of every rejected Tauri command. */
export interface AppErrorPayload {
  kind: AppErrorKind;
  message: string;
}

export interface ComponentVersion {
  name: "core" | "connectors" | "recommender";
  version: string;
}

export interface AboutInfo {
  appVersion: string;
  components: ComponentVersion[];
  platform: Platform;
}

// ───────────── Cove catalogue proxy (services/catalog-proxy) ─────────────

/** `GET /v1/search`, `GET /v1/browse`, `GET /v1/similar/:id` */
export interface ProxyPage {
  items: Game[];
  /** 0-based page index. */
  page: number;
  hasMore: boolean;
  total: number | null;
}

/** `GET /v1/game/:id` */
export interface ProxyGame {
  game: Game;
}

/** `POST /v1/match` request body. */
export interface ProxyMatchRequest {
  title: string;
}

/** `POST /v1/match` — best catalogue candidates for a title, best first. */
export interface ProxyMatchResponse {
  candidates: MatchCandidate[];
}

export type ProxyErrorCode =
  | "invalid_request"
  | "not_found"
  | "rate_limited"
  | "upstream_error"
  | "upstream_unavailable"
  | "not_configured";

/** Body of every non-2xx proxy response. */
export interface ProxyError {
  error: { code: ProxyErrorCode; message: string };
}

// ───────────── recommender sidecar (services/recommender) ─────────────

/** A catalogue game as the recommender sees it (content features + prior). */
export interface RecommenderGame {
  id: string;
  title: string;
  genres: string[];
  themes: string[];
  gameModes: string[];
  perspectives: string[];
  keywords: string[];
  developer: string;
  releaseYear: number | null;
  rating: number | null;
  popularity: number;
}

/** A library game as the recommender sees it (interaction signals). */
export interface RecommenderLibraryGame {
  /** Catalogue id when linked, otherwise the library game id. */
  gameId: string;
  title: string;
  playtimeMinutes: number;
  lastPlayedAt: IsoDateTime | null;
  favorite: boolean;
  achievements: AchievementProgress;
  sessionCount: number;
}

/** `GET /v1/steam/profile/:steamid` */
export interface ProxySteamProfile {
  steamId: string;
  personaName: string;
  avatarUrl: string | null;
  /** False when the profile is private (library and achievements are hidden). */
  public: boolean;
}

export interface ProxySteamGame {
  appId: string;
  name: string;
  playtimeMinutes: number;
  lastPlayedAt: IsoDateTime | null;
  iconUrl: string | null;
}

/** `GET /v1/steam/owned/:steamid` */
export interface ProxySteamOwned {
  /** True when Steam hides the games (profile or "game details" private). */
  private: boolean;
  games: ProxySteamGame[];
}

export interface ProxySteamAchievement {
  id: string;
  title: string;
  description: string;
  iconUrl: string | null;
  iconLockedUrl: string | null;
  hidden: boolean;
  unlockedAt: IsoDateTime | null;
  rarityPercent: number | null;
}

/** `GET /v1/steam/achievements/:steamid/:appid` */
export interface ProxySteamAchievements {
  private: boolean;
  achievements: ProxySteamAchievement[];
}
