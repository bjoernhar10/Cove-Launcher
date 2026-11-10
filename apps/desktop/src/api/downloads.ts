import type {
  DirectDownloadInput,
  DownloadItem,
  DownloadProgress,
  GameId,
  InstallPreview,
  InstallTarget,
  ItchGame,
  ItchStatus,
  ItchUpload,
} from "@cove/shared";
import { call, subscribe } from "./transport.ts";

export function listDownloads(): Promise<DownloadItem[]> {
  return call("downloads_list", {});
}

/** `downloads-changed` pushes the full list after every change. */
export function subscribeDownloads(listener: (items: DownloadItem[]) => void): () => void {
  return subscribe("downloads-changed", listener);
}

/** `download-progress`: bytes and speed, about 4 times per second. */
export function subscribeProgress(listener: (progress: DownloadProgress) => void): () => void {
  return subscribe("download-progress", listener);
}

/**
 * Store games: GOG games are installed by Cove into `target` (install dialog);
 * Steam and Epic installs are handed to their client.
 */
export async function enqueueDownload(gameId: GameId, target: InstallTarget | null = null): Promise<void> {
  await call("downloads_enqueue", { gameId, target });
}

/** Sizes for the install dialog of a GOG game. */
export function previewStoreInstall(gameId: GameId): Promise<InstallPreview> {
  return call("downloads_preview_store", { gameId });
}

/** Sizes for the install dialog (asks the server for the file size). */
export function previewUrlDownload(input: DirectDownloadInput): Promise<InstallPreview> {
  return call("downloads_preview_url", { input });
}

export function addUrlDownload(input: DirectDownloadInput, target: InstallTarget): Promise<DownloadItem> {
  return call("downloads_add_url", { input, target });
}

export function addItchDownload(
  game: ItchGame,
  upload: ItchUpload,
  target: InstallTarget,
  extras: readonly ItchUpload[] = [],
): Promise<DownloadItem> {
  return call("downloads_add_itch", {
    gameId: game.gameId,
    uploadId: upload.uploadId,
    downloadKeyId: game.downloadKeyId,
    title: game.title,
    target,
    extras: extras.map((e) => e.uploadId),
  });
}

export async function pauseDownload(id: string): Promise<void> {
  await call("downloads_set_status", { id, status: "paused" });
}

export async function resumeDownload(id: string): Promise<void> {
  await call("downloads_set_status", { id, status: "queued" });
}

export async function cancelDownload(id: string): Promise<void> {
  await call("downloads_set_status", { id, status: "cancelled" });
}

export async function retryDownload(id: string): Promise<void> {
  await call("downloads_set_status", { id, status: "queued" });
}

/** `orderedIds` = queued download ids in their new order. */
export async function reorderQueue(orderedIds: string[]): Promise<void> {
  await call("downloads_reorder", { orderedIds });
}

export async function clearDownloadHistory(): Promise<void> {
  await call("downloads_clear_history", {});
}

// ───────────── itch.io ─────────────

export function itchStatus(): Promise<ItchStatus> {
  return call("itch_status", {});
}

export function setItchApiKey(apiKey: string): Promise<ItchStatus> {
  return call("itch_set_api_key", { apiKey });
}

export function clearItchApiKey(): Promise<ItchStatus> {
  return call("itch_clear_api_key", {});
}

export function itchOwnedGames(): Promise<ItchGame[]> {
  return call("itch_owned_games", {});
}

/** A free game by its page address. */
export function itchLookup(url: string): Promise<ItchGame> {
  return call("itch_lookup", { url });
}

export function itchUploads(game: ItchGame): Promise<ItchUpload[]> {
  return call("itch_uploads", { gameId: game.gameId, downloadKeyId: game.downloadKeyId });
}
