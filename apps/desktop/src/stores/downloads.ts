import type { DownloadItem } from "@cove/shared";
import { create } from "zustand";
import { api } from "../api/index.ts";

interface DownloadsState {
  items: DownloadItem[];
  loaded: boolean;
  error: string | null;
  /** Reloads the list from the core (after local mutations). */
  refresh: () => Promise<void>;
  /** Loads the list and listens for `downloads-changed`. Returns the unsubscribe function. */
  connect: () => () => void;
}

export const useDownloads = create<DownloadsState>()((set, get) => ({
  items: [],
  loaded: false,
  error: null,
  refresh: async () => {
    try {
      set({ items: await api.downloads.listDownloads(), loaded: true, error: null });
    } catch (error: unknown) {
      set({ loaded: true, error: error instanceof Error ? error.message : String(error) });
    }
  },
  connect: () => {
    void get().refresh();
    return api.downloads.subscribeDownloads((items) => {
      set({ items, loaded: true, error: null });
    });
  },
}));

const ACTIVE = new Set(["downloading", "paused"]);

export function selectActive(items: DownloadItem[]): DownloadItem[] {
  return items
    .filter((i) => ACTIVE.has(i.download.status))
    .sort((a, b) => (a.download.queuePosition ?? 0) - (b.download.queuePosition ?? 0));
}

export function selectQueued(items: DownloadItem[]): DownloadItem[] {
  return items
    .filter((i) => i.download.status === "queued")
    .sort((a, b) => (a.download.queuePosition ?? 0) - (b.download.queuePosition ?? 0));
}

export function selectHistory(items: DownloadItem[]): DownloadItem[] {
  return items
    .filter((i) => i.download.queuePosition === null)
    .sort((a, b) => (b.download.finishedAt ?? "").localeCompare(a.download.finishedAt ?? ""));
}
