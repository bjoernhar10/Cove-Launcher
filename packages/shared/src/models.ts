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
  releaseDate: IsoDate;
  developer: string;
  publisher: string;
  /** Aggregated rating 0–100, null if unrated. */
  rating: number | null;
  /** Relative popularity 0–100, used for "Beliebt" sorting. */
  popularity: number;
  artwork: GameArtwork;
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
}

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
  sort: CatalogSort;
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

export interface StoreAccount {
  store: ConnectableStore;
  connected: boolean;
  /** Account display name when connected. */
  accountName: string | null;
  /** Number of games imported from this store. */
  gameCount: number;
  lastSyncedAt: IsoDateTime | null;
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

export interface AppSettings {
  general: GeneralSettings;
  appearance: AppearanceSettings;
  library: LibrarySettings;
  downloads: DownloadSettings;
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
  | "notSupported";

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
