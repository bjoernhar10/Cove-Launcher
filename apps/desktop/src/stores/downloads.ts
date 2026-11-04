import type { DownloadItem, DownloadProgress } from "@cove/shared";
import { create } from "zustand";
import { api } from "../api/index.ts";

interface DownloadsState {
  items: DownloadItem[];
  loaded: boolean;
  error: string | null;
  /** Reloads the list from the core (after local mutations). */
  refresh: () => Promise<void>;
  /** Loads the list and listens for list and progress events. Returns the unsubscribe function. */
  connect: () => () => void;
}

/** Live bytes and speed from `download-progress` (the list itself only changes on status changes). */
export function applyProgress(items: DownloadItem[], progress: DownloadProgress): DownloadItem[] {
  return items.map((item) =>
    item.download.id === progress.id && (item.download.status === "downloading" || item.download.status === progress.status)
      ? {
          ...item,
          download: {
            ...item.download,
            downloadedBytes: progress.downloadedBytes,
            totalBytes: progress.totalBytes,
            bytesPerSecond: progress.bytesPerSecond,
          },
        }
      : item,
  );
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
    const offList = api.downloads.subscribeDownloads((items) => {
      set({ items, loaded: true, error: null });
    });
    const offProgress = api.downloads.subscribeProgress((progress) => {
      set((s) => ({ items: applyProgress(s.items, progress) }));
    });
    return () => {
      offList();
      offProgress();
    };
  },
}));

/** Statuses shown under "Aktiv". */
const ACTIVE = new Set(["downloading", "paused", "verifying", "extracting", "handedOff"]);

export function selectActive(items: DownloadItem[]): DownloadItem[] {
  return items
    .filter((i) => ACTIVE.has(i.download.status))
    .sort((a, b) => (a.download.queuePosition ?? 0) - (b.download.queuePosition ?? 0));
}

/** The transfer shown in the sidebar (Cove's own downloads, not store installs). */
export function selectRunning(items: DownloadItem[]): DownloadItem | undefined {
  return selectActive(items).find((i) => ["downloading", "verifying", "extracting"].includes(i.download.status));
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
