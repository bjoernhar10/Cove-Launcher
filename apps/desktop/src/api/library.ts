import type {
  Achievement,
  BackupInfo,
  ExternalGame,
  FpsLog,
  GameNote,
  Screenshot,
  Game,
  GameBackups,
  GameId,
  GameMoveProgress,
  GameNews,
  GameShortcut,
  GameShortcuts,
  GameStateEvent,
  GameUpdateInfo,
  LaunchResult,
  LibraryItem,
  LibraryMatchEvent,
  LibraryUpdate,
  ManualGameInput,
  MatchCandidate,
  ModInfo,
  NewsItem,
  PlaytimeStats,
  PrefixAction,
  PrefixInfo,
  ProtonRating,
  RunningGame,
  ScanCandidate,
  ShortcutPlace,
  StorageOverview,
  StoreDlcList,
  UninstallMethod,
} from "@cove/shared";
import { call, subscribe } from "./transport.ts";

export function listLibrary(): Promise<LibraryItem[]> {
  return call("library_list", {});
}

/** Null if the game is not in the library. */
export function getLibraryItem(gameId: GameId): Promise<LibraryItem | null> {
  return call("library_get", { gameId });
}

export function addManualGame(input: ManualGameInput): Promise<LibraryItem> {
  return call("library_add_manual", { input });
}

export function addGameFromCatalog(game: Game): Promise<LibraryItem> {
  return call("library_add_from_catalog", { game });
}

export async function removeGame(gameId: GameId): Promise<void> {
  await call("library_remove", { gameId });
}

export function updateGame(gameId: GameId, patch: LibraryUpdate): Promise<LibraryItem> {
  return call("library_update", { gameId, patch });
}

export function setFavorite(gameId: GameId, favorite: boolean): Promise<LibraryItem> {
  return updateGame(gameId, { favorite });
}

export function scanFolder(path: string): Promise<ScanCandidate[]> {
  return call("library_scan_folder", { path });
}

/** Adds confirmed scan results one by one; returns the added items and per-game errors. */
export async function addScannedGames(
  candidates: ScanCandidate[],
): Promise<{ added: LibraryItem[]; failed: { title: string; message: string }[] }> {
  const added: LibraryItem[] = [];
  const failed: { title: string; message: string }[] = [];
  for (const c of candidates) {
    try {
      added.push(await addManualGame({ title: c.title, executablePath: c.executablePath, sizeBytes: c.sizeBytes }));
    } catch (error: unknown) {
      failed.push({ title: c.title, message: error instanceof Error ? error.message : String(error) });
    }
  }
  return { added, failed };
}

export function getAchievements(gameId: GameId): Promise<Achievement[]> {
  return call("library_achievements", { gameId });
}

export function getPlaytimeStats(gameId: GameId): Promise<PlaytimeStats> {
  return call("library_playtime", { gameId });
}

export function launchGame(gameId: GameId): Promise<LaunchResult> {
  return call("game_launch", { gameId });
}

/** Steam: opens Steam's uninstall dialog. Games Cove installed: deletes the game folder. */
export function uninstallGame(gameId: GameId): Promise<UninstallMethod> {
  return call("game_uninstall", { gameId });
}

/** "Speicherplatz": measures the installed games and their drives (takes a moment). */
export function storageOverview(): Promise<StorageOverview> {
  return call("storage_overview", {});
}

/** DLCs of a Steam game (installed ones marked). */
export function gameDlcs(gameId: GameId): Promise<StoreDlcList> {
  return call("game_dlcs", { gameId });
}

/** Opens the game in Steam to tick or untick its DLCs. */
export async function manageDlcs(gameId: GameId): Promise<void> {
  await call("game_manage_dlcs", { gameId });
}

/** ProtonDB's rating of the game (Linux); null when there is none. */
export function protonRating(gameId: GameId): Promise<ProtonRating | null> {
  return call("game_proton_rating", { gameId });
}

/** The game's announcements and patch notes from Steam (newest first). */
export function gameNews(gameId: GameId): Promise<NewsItem[]> {
  return call("game_news", { gameId });
}

/** Linux: the game's recorded sessions (frame rates), newest first. */
/** The user's note on a game; null = none. */
export function note(gameId: GameId): Promise<GameNote | null> {
  return call("game_note", { gameId });
}

/** Stores the note; an empty one is removed. */
export function setNote(gameId: GameId, text: string): Promise<GameNote | null> {
  return call("game_note_set", { gameId, text });
}

/** Every note by game id. */
export function notes(): Promise<Record<GameId, GameNote>> {
  return call("notes_list", {});
}

/** The game's screenshots, newest first. */
export function screenshots(gameId: GameId): Promise<Screenshot[]> {
  return call("game_screenshots", { gameId });
}

/** Opens a listed screenshot in the system's image viewer. */
export async function openScreenshot(url: string): Promise<void> {
  await call("screenshot_open", { url });
}

export function fpsLogs(gameId: GameId): Promise<FpsLog[]> {
  return call("game_fps_logs", { gameId });
}

export async function clearFpsLogs(gameId: GameId): Promise<void> {
  await call("game_fps_logs_clear", { gameId });
}

/** A game's mods in their order (later ones win for the same file). */
export function mods(gameId: GameId): Promise<ModInfo[]> {
  return call("mods_list", { gameId });
}

/** Lets the user pick a mod file (archive or single file) or folder; null when cancelled. */
export function pickMod(folder: boolean): Promise<string | null> {
  return call("mods_pick", { folder });
}

export function addMod(gameId: GameId, source: string, name: string, target: string): Promise<ModInfo[]> {
  return call("mods_add", { gameId, source, name, target });
}

export function setModEnabled(gameId: GameId, modId: string, enabled: boolean): Promise<ModInfo[]> {
  return call("mods_set_enabled", { gameId, modId, enabled });
}

export function moveMod(gameId: GameId, modId: string, position: number): Promise<ModInfo[]> {
  return call("mods_move", { gameId, modId, position });
}

export function removeMod(gameId: GameId, modId: string): Promise<ModInfo[]> {
  return call("mods_remove", { gameId, modId });
}

/** Games added to Heroic by hand and Lutris' Wine and Linux games. */
export function launcherGames(): Promise<ExternalGame[]> {
  return call("launchers_scan", {});
}

/** Adds them with their Wine prefix, Wine build and playtime; returns the new game ids. */
export function importLauncherGames(ids: string[]): Promise<GameId[]> {
  return call("launchers_import", { ids });
}

/** The game's desktop and menu shortcuts, and the places this system has. */
export function shortcuts(gameId: GameId): Promise<GameShortcuts> {
  return call("game_shortcuts", { gameId });
}

/** A shortcut that starts the game through Cove (the cover is its icon). */
export function createShortcut(gameId: GameId, place: ShortcutPlace): Promise<GameShortcut> {
  return call("game_create_shortcut", { gameId, place });
}

export async function removeShortcut(gameId: GameId, place: ShortcutPlace): Promise<void> {
  await call("game_remove_shortcut", { gameId, place });
}

/** Recent posts of the games played last (Home). */
export function recentNews(): Promise<GameNews[]> {
  return call("news_recent", {});
}

/** Moves the game folder into another games folder; progress via `subscribeMoveProgress`. */
/** Save backups of a game: known to the manifest, files found now, backups (newest first). */
export function backups(gameId: GameId): Promise<GameBackups> {
  return call("game_backups", { gameId });
}

/** Backs the game's saves up now; null when nothing changed since the last backup. */
export function backUpNow(gameId: GameId): Promise<BackupInfo | null> {
  return call("game_backup_now", { gameId });
}

/** Puts a backup back (the current saves are backed up first); returns the files restored. */
export function restoreBackup(gameId: GameId, backupId: string): Promise<number> {
  return call("game_backup_restore", { gameId, backupId });
}

/** Linux: a game's Proton prefix (exists, size, winetricks installed). */
export function prefixInfo(gameId: GameId): Promise<PrefixInfo> {
  return call("game_prefix_info", { gameId });
}

/** Linux: opens the prefix folder, deletes it or starts winetricks for it. */
export async function prefixAction(gameId: GameId, action: PrefixAction): Promise<void> {
  await call("game_prefix_action", { gameId, action });
}

export async function moveGame(gameId: GameId, folder: string): Promise<void> {
  await call("game_move", { gameId, folder });
}

/** Installed and newest version of a GOG game Cove installed. */
export function checkGameUpdate(gameId: GameId): Promise<GameUpdateInfo> {
  return call("game_update_check", { gameId });
}

/** Queues the update; only changed parts are downloaded. */
export async function installGameUpdate(gameId: GameId): Promise<void> {
  await call("game_update", { gameId });
}

/** "DLCs ändern": unticked DLCs are removed, ticked ones downloaded (as an update). */
export async function setGameDlcs(gameId: GameId, excludedDlcs: string[]): Promise<void> {
  await call("game_set_dlcs", { gameId, excludedDlcs });
}

export function subscribeMoveProgress(listener: (progress: GameMoveProgress) => void): () => void {
  return subscribe("game-move-progress", listener);
}

export async function stopGame(gameId: GameId): Promise<void> {
  await call("game_stop", { gameId });
}

export function listRunningGames(): Promise<RunningGame[]> {
  return call("game_running_list", {});
}

/** `game-state-changed`: running / stopped. */
export function subscribeGameState(handler: (event: GameStateEvent) => void): () => void {
  return subscribe("game-state-changed", handler);
}

/** Catalogue suggestions waiting for the user's decision (`matchStatus: "pending"`). */
export function getMatchCandidates(gameId: GameId): Promise<MatchCandidate[]> {
  return call("library_match_candidates", { gameId });
}

/** Links a library game to a catalogue entry and takes over its metadata and artwork. */
export function linkCatalog(gameId: GameId, catalogId: string): Promise<LibraryItem> {
  return call("library_link_catalog", { gameId, catalogId });
}

export function rejectMatch(gameId: GameId): Promise<LibraryItem> {
  return call("library_reject_match", { gameId });
}

/** Runs the automatic catalogue matching again (result arrives as `library-match-changed`). */
export async function rematch(gameId: GameId): Promise<void> {
  await call("library_rematch", { gameId });
}

export function subscribeMatchChanges(handler: (event: LibraryMatchEvent) => void): () => void {
  return subscribe("library-match-changed", handler);
}
