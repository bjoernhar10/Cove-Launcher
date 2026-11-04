import type { PluginFile } from "@cove/shared";
import { create } from "zustand";
import { api } from "../api/index.ts";
import { MAX_FILE_BYTES, PluginHost, type SidebarEntry, type Spawn } from "../plugins/host.ts";
import { toast, UserFacingError } from "./toasts.ts";

interface PluginsState {
  items: SidebarEntry[];
  host: PluginHost | null;
  /** Starts the plugin host (once). `spawn` is injectable for tests. */
  start: (spawn?: Spawn) => Promise<void>;
  /** Re-reads the plugin list (after enable, approve, install, …). */
  reload: () => Promise<void>;
  stop: () => void;
  click: (entry: SidebarEntry, file: PluginFile | null) => void;
}

async function defaultSpawn(): Promise<Spawn | null> {
  // No workers in tests (jsdom) or very old engines: plugins simply do not run.
  if (typeof Worker === "undefined") return null;
  const module = await import("../plugins/spawnWorker.ts");
  return module.spawnWorker;
}

export const usePlugins = create<PluginsState>()((set, get) => ({
  items: [],
  host: null,
  start: async (spawn) => {
    if (get().host) return;
    const spawner = spawn ?? (await defaultSpawn());
    if (!spawner) return;
    const host = new PluginHost(spawner, {
      setSidebarItems: (pluginId, items) => {
        set((s) => ({ items: [...s.items.filter((i) => i.pluginId !== pluginId), ...items] }));
      },
      notify: (pluginName, message, gameId) => {
        toast(message, "neutral", {
          title: pluginName,
          ...(gameId ? { action: { label: "Öffnen", to: `/library/${encodeURIComponent(gameId)}` } } : {}),
          durationMs: 6000,
        });
      },
      failed: (pluginName, message) => {
        toast(`${pluginName}: ${message}`, "danger", { title: "Plugin-Fehler" });
      },
    });
    set({ host });
    await get().reload();
  },
  reload: async () => {
    const host = get().host;
    if (!host) return;
    try {
      host.sync(await api.plugins.listPlugins());
    } catch {
      // Outside the desktop app there is no core and therefore no plugins.
    }
  },
  stop: () => {
    get().host?.stopAll();
    set({ host: null, items: [] });
  },
  click: (entry, file) => {
    get().host?.click(entry.pluginId, entry.id, file);
  },
}));

/** Reads a picked file for a plugin (text, size-limited). */
export async function readPickedFile(file: File, maxBytes: number): Promise<PluginFile> {
  if (file.size > Math.min(maxBytes, MAX_FILE_BYTES)) {
    throw new UserFacingError(`Die Datei ist zu groß (höchstens ${String(Math.round(maxBytes / 1024))} KB).`);
  }
  return { name: file.name, text: await file.text() };
}
