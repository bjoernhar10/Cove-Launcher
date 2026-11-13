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

export type StoreSource = "steam" | "gog" | "epic" | "amazon" | "manual";

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

export type CompatibilityTool = "native" | "proton" | "wine";

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
  /**
   * With Proton: the build to use. `"latest"` = always the newest GE-Proton, a GE-Proton
   * release name (`"GE-Proton10-25"`), or `"steam:<folder>"` for a build Steam installed.
   * null = the default (`library.defaultProton`).
   */
  protonVersion: ProtonVersionId | null;
  /** Linux: MangoHud, GameMode, gamescope, FSR, DXVK/VKD3D (games Cove starts itself). */
  tweaks: GameTweaks;
}

/** Per-game options on Linux for games Cove starts itself. */
export interface GameTweaks {
  /** Performance overlay (`mangohud`). */
  mangohud: boolean;
  /** Feral GameMode (`gamemoderun`). */
  gamemode: boolean;
  /** Run inside Valve's gamescope compositor. */
  gamescope: boolean;
  /** gamescope: the resolution the game renders at; null = the game's own. */
  gamescopeWidth: number | null;
  gamescopeHeight: number | null;
  /** gamescope: fullscreen. */
  gamescopeFullscreen: boolean;
  /** AMD FSR 1 upscaling: gamescope's filter, else GE-Proton's `WINE_FULLSCREEN_FSR`. */
  fsr: boolean;
  /** Proton: Direct3D 9 to 11 through DXVK (off = WineD3D). */
  dxvk: boolean;
  /** Proton: Direct3D 12 through VKD3D-Proton (off = no D3D12). */
  vkd3d: boolean;
}

/** Which Linux game tools are installed. */
export interface GameTools {
  mangohud: boolean;
  gamemode: boolean;
  gamescope: boolean;
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
  /** How "Spielen" starts the game; null when it can't be started yet. */
  launchVia: LaunchVia | null;
  /** Installed by Cove's download manager: Cove may move and uninstall it. */
  installedByCove: boolean;
}

/** How a game is started: by Cove itself or through a store client. */
export type LaunchVia = "direct" | "steam" | "epicLauncher" | "gogGalaxy" | "amazonGames";

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

/** Statistics page: playtime over a year or all time (`stats_overview`). */
export interface PlayStats {
  /** The year shown; null = all time. */
  year: number | null;
  /** Years with tracked sessions, newest first. */
  years: number[];
  /**
   * Playtime in the range. All time includes playtime imported from the stores;
   * a year only counts sessions Cove tracked.
   */
  totalMinutes: number;
  sessionCount: number;
  /** Games with playtime in the range. */
  gamesPlayed: number;
  /** Days with at least one session. */
  daysPlayed: number;
  /** Most days in a row with a session. */
  longestStreakDays: number;
  longestSession: StatsSession | null;
  achievementsUnlocked: number;
  /** Twelve months, oldest first: the year's months, or the last 12 for all time. */
  months: StatsMonth[];
  /** Tracked minutes per weekday, Monday first (7 entries). */
  weekdays: number[];
  /** Most played games, at most 10. */
  topGames: StatsGame[];
  /** Minutes per genre, most first (a game with two genres counts for both). */
  genres: StatsGenre[];
  /** Minutes per store, most first. */
  stores: StatsStore[];
}

export interface StatsMonth {
  /** `YYYY-MM`. */
  month: string;
  minutes: number;
}

export interface StatsSession {
  gameId: GameId;
  title: string;
  minutes: number;
  startedAt: IsoDateTime;
}

export interface StatsGame {
  game: Game;
  minutes: number;
  sessionCount: number;
}

export interface StatsGenre {
  genre: Genre;
  minutes: number;
}

export interface StatsStore {
  store: StoreSource;
  minutes: number;
}

export interface Money {
  amount: number;
  /** ISO 4217, e.g. EUR. */
  currency: string;
}

/** One shop's current offer (from IsThereAnyDeal; links unchanged). */
export interface PriceDeal {
  shop: string;
  price: Money;
  regular: Money;
  /** Discount in percent (0 to 100). */
  cut: number;
  url: string;
  expiry: IsoDateTime | null;
}

/** A game's prices in the user's country. */
export interface GamePrice {
  /** The game's page on IsThereAnyDeal. */
  url: string | null;
  /** Cheapest current offer. */
  best: PriceDeal | null;
  /** Current offers, cheapest first (at most 5). */
  deals: PriceDeal[];
  /** Lowest price ever recorded. */
  historyLow: Money | null;
  checkedAt: IsoDateTime;
}

/** A game on the wishlist (not owned). */
export interface WishlistItem {
  game: Game;
  addedAt: IsoDateTime;
  /** Notify when the best price is at or below this (in the price's currency); null = off. */
  targetPrice: number | null;
  /** Notify about every new discount. */
  notifyOnSale: boolean;
  /** Last known prices; null until the first check or when IsThereAnyDeal doesn't know the game. */
  price: GamePrice | null;
}

/** Fired when a wishlist game reaches its target price or goes on sale. */
export interface PriceAlert {
  gameId: GameId;
  title: string;
  deal: PriceDeal;
  /** "target" = at or below the target price, "sale" = a new discount. */
  reason: "target" | "sale";
}

/** Which button names and symbols to show (A/B/X/Y, ✕/○/□/△, Nintendo's B/A/Y/X). */
export type ControllerKind = "xbox" | "playstation" | "switch" | "steam" | "generic";

/**
 * What a controller button means for Cove. Face buttons by position:
 * `confirm` is the bottom one, `back` the right one, `secondary` the left one,
 * `tertiary` the top one (the label on them depends on the controller).
 */
export type PadAction =
  | "up"
  | "down"
  | "left"
  | "right"
  | "confirm"
  | "back"
  | "secondary"
  | "tertiary"
  | "prevTab"
  | "nextTab"
  | "menu"
  | "view"
  | "home";

export interface GamepadInfo {
  id: number;
  name: string;
  kind: ControllerKind;
}

/** A controller action (event `gamepad-input`); `kind` is the controller it came from. */
export interface GamepadInput {
  action: PadAction;
  kind: ControllerKind;
}

export type DownloadStatus =
  | "downloading"
  | "paused"
  | "queued"
  /** SHA-256 check after the transfer. */
  | "verifying"
  /** Unpacking the archive into the game folder. */
  | "extracting"
  | "completed"
  | "failed"
  | "cancelled"
  /** Store game: the installation was handed over to the store's own client. */
  | "handedOff";

/** Where a download comes from. Store games are never downloaded by Cove itself. */
export type DownloadOrigin = "direct" | "itch" | "steam" | "gog" | "epic" | "amazon";

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
  origin: DownloadOrigin;
  fileName: string | null;
  /** True once the SHA-256 matched the expected value. */
  checksumVerified: boolean;
  /** Game folder after a successful install. */
  installPath: string | null;
  executablePath: string | null;
  /** itch.io extra (soundtrack …) of the game: its file name; null for the game itself. */
  extra: string | null;
}

/** A folder the user may install into (install dialog). */
export interface FolderInfo {
  path: string;
  /** Free space on its drive, null if unknown. */
  freeBytes: number | null;
  /** Cove may create files there (or create the folder). */
  writable: boolean;
  /** Windows: below "Program Files", which needs admin rights. */
  needsAdmin: boolean;
}

/** Sizes shown in the install dialog before a download starts. */
export interface InstallPreview {
  title: string;
  downloadBytes: number | null;
  /** Size after the install; null when it is only known after unpacking. */
  installBytes: number | null;
  /** Why Cove can't install it and the store's launcher must (anti-cheat, third-party launcher). */
  launcherOnly: string | null;
  /** Owned DLCs that can be installed with the game (GOG, Epic); sizes above are the game alone. */
  dlcs: DlcOption[];
}

/** One DLC in the install dialog. */
export interface DlcOption {
  /** GOG product id or Epic app name. */
  id: string;
  title: string;
  downloadBytes: number | null;
  installBytes: number | null;
  /** Ticked: installed with the game (remembers the last choice for this game). */
  selected: boolean;
}

/** Where the install dialog puts a game. */
export interface InstallTarget {
  /** Games folder; Cove creates a subfolder named after the game. */
  folder: string;
  /** "Diesen Ordner als Standard verwenden". */
  makeDefault: boolean;
  /** DLCs left out (ids from `InstallPreview.dlcs`); kept for later updates. */
  excludedDlcs?: string[];
}

export type MoveStatus = "copying" | "verifying" | "completed" | "failed";

/** Event `game-move-progress` while a game folder is moved. */
export interface GameMoveProgress {
  gameId: GameId;
  status: MoveStatus;
  copiedBytes: number;
  totalBytes: number;
  error: string | null;
}

/** Update state of a store game Cove installed (GOG, Epic). */
export interface GameUpdateInfo {
  available: boolean;
  installedVersion: string;
  latestVersion: string;
}

/** What `game_uninstall` did: Cove deleted the folder, or Steam asks and removes the game. */
export type UninstallMethod = "removed" | "steam";

/** A DLC of a store game as the store sees it (Steam: managed in Steam itself). */
export interface StoreDlc {
  id: string;
  title: string;
  /** Installed on this computer. */
  installed: boolean;
  /** GOG/Epic: no files of its own, the game unlocks it through the login. */
  unlockOnly?: boolean;
}

export type CollectionId = string;

/** One of the user's own collections ("Mit Freunden", "Angefangen" ...). A game can be in several. */
export interface Collection {
  id: CollectionId;
  name: string;
  /** The library games in it, oldest member first. */
  gameIds: GameId[];
}

/** `"latest"`, a GE-Proton release name or `"steam:<folder>"`. */
export type ProtonVersionId = string;

/** Where a Proton build comes from: GE-Proton (Cove downloads and updates it) or Steam. */
export type ProtonKind = "ge" | "steam";

/** A Proton build a game can use. */
export interface ProtonVersion {
  id: ProtonVersionId;
  name: string;
  kind: ProtonKind;
  installed: boolean;
  /** Size of the download (GE-Proton releases). */
  downloadBytes: number | null;
  releasedAt: string | null;
  /** Library games that start with this build (directly, as "latest" or as the default). */
  games: number;
  /** Cove may delete it: a GE-Proton build no game uses. */
  removable: boolean;
}

export type ProtonPhase = "downloading" | "verifying" | "unpacking" | "done" | "failed" | "cancelled";

/** Event `proton-progress`: one GE-Proton install, at most about 4 per second. */
export interface ProtonProgress {
  id: ProtonVersionId;
  phase: ProtonPhase;
  downloadedBytes: number;
  totalBytes: number;
  /** User-facing reason when `phase` is `failed`. */
  error: string | null;
}

/** Everything the Proton settings show. */
export interface ProtonOverview {
  /** GE-Proton (newest first), then Steam's builds. */
  versions: ProtonVersion[];
  /** The build "latest" stands for right now. */
  latest: ProtonVersionId | null;
  /** When the list of GE-Proton releases was last loaded. */
  checkedAt: string | null;
  /** The release list could not be loaded; only installed builds are listed. */
  offline: boolean;
  installing: ProtonProgress[];
  umu: UmuStatus;
}

/** Where the umu-launcher Cove uses comes from: the system's package or Cove's own download. */
export type UmuSource = "system" | "cove";

/** umu-launcher: runs Proton inside the Steam Linux Runtime, like Steam does. */
export interface UmuStatus {
  /** `library.useUmu`. */
  enabled: boolean;
  /** null = not on this machine yet. */
  source: UmuSource | null;
  /** Version of Cove's copy (null for the system's). */
  version: string | null;
  /** Newest release, when the list could be loaded. */
  latest: string | null;
  /** `python3` is there (Cove's copy needs it). */
  python: boolean;
}

/** A game's Proton prefix (Linux). */
export interface PrefixInfo {
  /** The prefix has been created (the game started at least once). */
  exists: boolean;
  sizeBytes: number | null;
  /** `winetricks` is installed. */
  winetricks: boolean;
}

/** open = show the folder · reset = delete it (the next start creates a fresh one) · winetricks = start winetricks for it. */
export type PrefixAction = "open" | "reset" | "winetricks";

/** How long a game takes, in seconds (IGDB "time to beat", from player reports). */
export interface TimeToBeat {
  /** Main story. */
  hastily: number | null;
  /** Main story and extras. */
  normally: number | null;
  /** Everything (100 %). */
  completely: number | null;
  /** Number of player reports. */
  count: number;
}

/** ProtonDB tier: how well a Windows game runs on Linux through Proton. */
export type ProtonTier = "platinum" | "gold" | "silver" | "bronze" | "borked";

/** ProtonDB's community rating of a game. */
export interface ProtonRating {
  tier: ProtonTier;
  /** Number of user reports behind the tier. */
  reports: number;
  /** The Steam app the rating belongs to (ProtonDB rates Steam versions). */
  steamAppId: string;
}

export interface StoreDlcList {
  dlcs: StoreDlc[];
  /** All DLCs of the game; the list is cut off for games with hundreds. */
  total: number;
}

/** Event `download-progress`, at most about 4 per second and download. */
export interface DownloadProgress {
  id: string;
  status: DownloadStatus;
  downloadedBytes: number;
  totalBytes: number;
  bytesPerSecond: number;
}

/** A direct link (DRM-free game, mod, patch). */
export interface DirectDownloadInput {
  url: string;
  /** Library title; null = derived from the file name. */
  title: string | null;
  /** Expected SHA-256 (hex); null = not checked. */
  sha256: string | null;
}

export interface ItchStatus {
  connected: boolean;
  userName: string | null;
}

/** An itch.io game the user owns (or a free one). */
export interface ItchGame {
  gameId: number;
  title: string;
  coverUrl: string | null;
  /** Purchase key; null for free games. */
  downloadKeyId: number | null;
  url: string | null;
}

export interface ItchUpload {
  uploadId: number;
  fileName: string;
  sizeBytes: number | null;
  platforms: Platform[];
  /** Not the game itself (soundtrack, artbook …): offered as an extra. */
  extra: boolean;
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

/** What a plugin may do. The user confirms them when installing. */
export type PluginPermission =
  | "library.read"
  | "library.add"
  | "catalog.search"
  | "achievements.define"
  | "ui.addSidebarItem"
  | "ui.notify"
  | "storage";

/**
 * - `active`: approved and enabled, runs in its own worker
 * - `disabled`: approved, switched off by the user
 * - `needsApproval`: new, or an update asks for more permissions
 * - `invalid`: `plugin.json` is broken (see `error`)
 */
export type PluginStatus = "active" | "disabled" | "needsApproval" | "invalid";

export interface PluginInfo {
  id: string;
  name: string;
  version: string;
  author: string;
  description: string;
  /** Requested in `plugin.json`. */
  permissions: PluginPermission[];
  /** Confirmed by the user. */
  granted: PluginPermission[];
  enabled: boolean;
  status: PluginStatus;
  error: string | null;
  /** Installed into Cove's plugin folder (development plugins cannot be removed here). */
  removable: boolean;
  /** Module URL for the worker; only for active plugins. */
  entryUrl: string | null;
}

/** `plugin.json` of a folder the user wants to install (shown in the permission dialog). */
export interface PluginManifestPreview {
  id: string;
  name: string;
  version: string;
  author: string;
  description: string;
  permissions: PluginPermission[];
  /** An older version is installed already (it will be replaced). */
  alreadyInstalled: boolean;
}

/** `library.add` from a plugin. */
export interface PluginGameInput {
  title: string;
  /** Null adds the game without install (Cove looks up catalogue data). */
  executablePath: string | null;
}

export type ThemePreference = "system" | "light" | "dark";

export type AccentColor = "blue" | "teal" | "sage" | "plum" | "clay";

export type Language = "de" | "en";

export type StartPage = "home" | "library";

export interface GeneralSettings {
  /** Open in the TV mode (also automatic on the Steam Deck and in gamescope sessions). */
  startInTvMode: boolean;
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
  /** Folders games are installed into (e.g. one per drive). The default is `downloads.directory`. */
  gameFolders: string[];
  /** Start Steam with `-silent` (tray only) before a Steam game starts. */
  steamSilentStart: boolean;
  /** Sync Epic cloud saves before and after playing (games Cove installed). */
  epicCloudSaves: boolean;
  /** Sync GOG cloud saves before and after playing (games Cove installed). */
  gogCloudSaves: boolean;
  /** Linux: Proton build for games without a choice of their own (`"latest"` by default). */
  defaultProton: ProtonVersionId;
  /** Linux: start Proton through umu-launcher (Steam Linux Runtime, per-game fixes). */
  useUmu: boolean;
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
  backups: BackupSettings;
  emulators: EmulatorSettings;
  wishlist: WishlistSettings;
}

/** Wishlist prices (IsThereAnyDeal through the Cove proxy). */
export interface WishlistSettings {
  /** Two-letter country code for prices and currency, e.g. DE. */
  country: string;
  /** Check prices every few hours and notify about alerts. */
  priceAlerts: boolean;
}

/** Save backups of every game (save locations from the Ludusavi manifest). */
export interface BackupSettings {
  /** Back up after every session. */
  enabled: boolean;
  /** Where backups go; empty = Cove's app data folder. */
  directory: string;
  /** Backups kept per game (1 to 100). */
  keep: number;
}

/** Folders with ROMs and the emulator that plays them. */
export interface EmulatorSettings {
  romFolders: RomFolder[];
}

export interface RomFolder {
  /** Stable id (chosen by the UI). */
  id: string;
  /** `snes`, `ps2` … (`SystemInfo.id`). */
  system: string;
  path: string;
  /** Emulator id of the system (`retroarch`, `dolphin` …) or `custom`. */
  emulator: string;
  /** Program to run instead of the one found on the system (needed for `custom`). */
  program: string | null;
  /** `custom`: the arguments, `{rom}` stands for the ROM file. */
  arguments: string;
}

/** An emulator for a system and whether it is installed. */
export interface EmulatorInfo {
  id: string;
  name: string;
  installed: boolean;
}

/** A console Cove can play ROMs of. */
export interface SystemInfo {
  id: string;
  name: string;
  /** ROM file extensions (lower case, without the dot). */
  extensions: string[];
  /** RetroArch first. */
  emulators: EmulatorInfo[];
}

/** Result of scanning the ROM folders. */
export interface RomScanResult {
  /** ROMs found in all folders. */
  found: number;
  /** New library games. */
  added: number;
}

/** One save backup of a game. */
export interface BackupInfo {
  /** Folder name (the time), used to restore it. */
  id: string;
  createdAt: IsoDateTime;
  files: number;
  bytes: number;
}

/** Save backups of a game, for its page. */
export interface GameBackups {
  /** The manifest knows where this game keeps its saves. */
  known: boolean;
  /** Save files found right now. */
  files: number;
  bytes: number;
  /** Newest first. */
  backups: BackupInfo[];
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
  | "connectorsUnavailable"
  | "download"
  | "itch"
  | "plugin"
  | "update";

/** Error shape of every rejected Tauri command. */
export interface AppErrorPayload {
  kind: AppErrorKind;
  message: string;
}

/** Health of a component. Sidecars start on first use, so `idle` is normal. */
export type ServiceStatus = "running" | "idle" | "unavailable" | "notInstalled";

export interface ComponentVersion {
  name: "core" | "connectors" | "recommender";
  version: string;
  status: ServiceStatus;
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

/** `POST /v1/time-to-beat`: IGDB game ids (at most 500). */
export interface ProxyTimeToBeatRequest {
  ids: number[];
}

/** Times per IGDB game id; ids without reports are left out. */
export interface ProxyTimeToBeatResponse {
  items: Record<string, TimeToBeat>;
}

/** `POST /v1/prices`: current prices of games, looked up by title on IsThereAnyDeal. */
export interface ProxyPricesRequest {
  /** Two-letter country code (ISO 3166-1), e.g. DE. */
  country: string;
  /** At most 50; `key` comes back as the key of the answer. */
  games: { key: string; title: string }[];
}

export interface ProxyPricesResponse {
  /** Per requested key; null = not found on IsThereAnyDeal. */
  items: Record<string, GamePrice | null>;
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

// ───────────── updates (Tauri updater) ─────────────

export interface UpdateInfo {
  version: string;
  notes: string | null;
  date: string | null;
}

export interface UpdateCheck {
  currentVersion: string;
  /** `null` when Cove is up to date. */
  available: UpdateInfo | null;
  /** `false` for `.deb`/`.rpm` installs: the package manager updates those. */
  canInstall: boolean;
  installHint: string | null;
}

/** Event `update-progress` while an update downloads. */
export interface UpdateProgress {
  downloadedBytes: number;
  totalBytes: number | null;
}
