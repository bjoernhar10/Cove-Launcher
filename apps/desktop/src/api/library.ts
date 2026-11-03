import type {
  Achievement,
  Game,
  GameId,
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
