import type { DownloadItem, GameId } from "@cove/shared";
import { call, subscribe } from "./transport.ts";

export function listDownloads(): Promise<DownloadItem[]> {
  return call("downloads_list", {});
}

/** `downloads-changed` pushes the full list after every change. */
export function subscribeDownloads(listener: (items: DownloadItem[]) => void): () => void {
  return subscribe("downloads-changed", listener);
}

export async function enqueueDownload(gameId: GameId): Promise<void> {
  await call("downloads_enqueue", { gameId });
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
