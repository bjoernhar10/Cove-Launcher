import type {
  Achievement,
  Game,
  GameId,
  GameMoveProgress,
  GameUpdateInfo,
  GameStateEvent,
  LaunchResult,
  LibraryItem,
  LibraryMatchEvent,
  LibraryUpdate,
  ManualGameInput,
  MatchCandidate,
  PlaytimeStats,
  RunningGame,
  ScanCandidate,
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

/** DLCs of a Steam game (installed ones marked). */
export function gameDlcs(gameId: GameId): Promise<StoreDlcList> {
  return call("game_dlcs", { gameId });
}

/** Opens the game in Steam to tick or untick its DLCs. */
export async function manageDlcs(gameId: GameId): Promise<void> {
  await call("game_manage_dlcs", { gameId });
}

/** Moves the game folder into another games folder; progress via `subscribeMoveProgress`. */
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
