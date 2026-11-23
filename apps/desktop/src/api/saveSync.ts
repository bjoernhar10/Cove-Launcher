import type { CloudProvider, GameId, GameSaveSync, LibrarySyncStatus, SaveSyncKeep, SaveSyncStatus } from "@cove/shared";
import { call, subscribe } from "./transport.ts";

/** Save sync: the connected cloud, if any. */
export function status(): Promise<SaveSyncStatus> {
  return call("save_sync_status", {});
}

/** Sign-in in the system browser; resolves once it is done. */
export function connect(provider: CloudProvider): Promise<SaveSyncStatus> {
  return call("save_sync_connect", { provider });
}

/** Forgets the cloud on this computer; the saves in it stay. */
export function disconnect(): Promise<SaveSyncStatus> {
  return call("save_sync_disconnect", {});
}

/** A game's save sync, without asking the cloud. */
export function game(gameId: GameId): Promise<GameSaveSync> {
  return call("game_save_sync", { gameId });
}

/** Syncs the game's saves now. */
export function syncNow(gameId: GameId): Promise<GameSaveSync> {
  return call("game_save_sync_now", { gameId });
}

/** A conflict: which side wins; the other is kept as a save backup. */
export function resolve(gameId: GameId, keep: SaveSyncKeep): Promise<GameSaveSync> {
  return call("game_save_sync_resolve", { gameId, keep });
}

/** A game's saves were synced in the background, or a conflict came up. */
export function subscribeChanged(handler: (event: { gameId: GameId }) => void): () => void {
  return subscribe("save-sync-changed", handler);
}

/** Library sync: on, connected, last sync and its problem. */
export function libraryStatus(): Promise<LibrarySyncStatus> {
  return call("library_sync_status", {});
}

/** Syncs favourites, notes, collections, wishlist and playtime now. */
export function syncLibraryNow(): Promise<LibrarySyncStatus> {
  return call("library_sync_now", {});
}

/** A library sync ran; `changed` = this computer's library changed. */
export function subscribeLibrary(handler: (event: { changed: boolean }) => void): () => void {
  return subscribe("library-sync-changed", handler);
}
