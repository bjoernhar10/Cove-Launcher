/**
 * Plugin host (UI side). Starts one worker per active plugin, answers its
 * calls and enforces permissions. The core checks data-changing calls a
 * second time (storage, achievements, library.add).
 */
import type {
  HostToPlugin,
  LibraryItem,
  PluginCatalogGame,
  PluginFile,
  PluginGame,
  PluginIcon,
  PluginInfo,
  PluginMethod,
  PluginToHost,
} from "@cove/shared";
import { METHOD_PERMISSIONS } from "@cove/shared/plugin-api";
import { api, CoveError } from "../api/index.ts";

export interface PluginPort {
  postMessage(message: HostToPlugin): void;
  onmessage: ((event: { data: PluginToHost }) => void) | null;
  onerror: ((message: string) => void) | null;
  terminate(): void;
}

export type Spawn = (plugin: PluginInfo) => PluginPort;

export interface SidebarEntry {
  pluginId: string;
  pluginName: string;
  id: string;
  label: string;
  icon: PluginIcon;
  pickFile: { accept: string; maxBytes: number } | null;
}

/** How the host shows things (sidebar items, toasts). */
export interface PluginUi {
  setSidebarItems(pluginId: string, items: SidebarEntry[]): void;
  notify(pluginName: string, message: string, gameId: string | null): void;
  failed(pluginName: string, message: string): void;
}

const ICONS: readonly PluginIcon[] = ["puzzle", "dice", "trophy", "upload", "sparkles", "list", "star", "clock"];
export const MAX_FILE_BYTES = 5 * 1024 * 1024;
const MAX_SIDEBAR_ITEMS = 3;
const NOTIFY_LIMIT = 3;
const NOTIFY_WINDOW_MS = 10_000;

class PluginCallError extends Error {
  constructor(
    readonly kind: string,
    message: string,
  ) {
    super(message);
  }
}

type Params = Record<string, unknown>;

function isParams(value: unknown): value is Params {
  return typeof value === "object" && value !== null && !Array.isArray(value);
}

function text(params: Params, key: string, max: number): string {
  const value = params[key];
  if (typeof value !== "string" || value.trim() === "" || value.length > max) {
    throw new PluginCallError("invalidInput", `„${key}“ muss ein Text mit 1 bis ${String(max)} Zeichen sein.`);
  }
  return value;
}

function optionalText(params: Params, key: string): string | null {
  const value = params[key];
  if (value === null || value === undefined) return null;
  if (typeof value !== "string") throw new PluginCallError("invalidInput", `„${key}“ muss ein Text sein.`);
  return value;
}

export function toPluginGame(item: LibraryItem): PluginGame {
  return {
    id: item.game.id,
    title: item.game.title,
    installed: item.entry.installed,
    favorite: item.entry.favorite,
    playtimeMinutes: item.entry.playtimeMinutes,
    lastPlayedAt: item.entry.lastPlayedAt,
    genres: [...item.game.genres],
    source: item.entry.source,
  };
}

interface Running {
  plugin: PluginInfo;
  port: PluginPort;
  items: SidebarEntry[];
  notifications: number[];
}

export class PluginHost {
  private readonly running = new Map<string, Running>();

  constructor(
    private readonly spawn: Spawn,
    private readonly ui: PluginUi,
  ) {}

  /** Starts active plugins, stops the rest; restarts a plugin when its version or permissions changed. */
  sync(plugins: PluginInfo[]): void {
    const active = plugins.filter((p) => p.status === "active" && p.entryUrl !== null);
    for (const [id, run] of this.running) {
      const next = active.find((p) => p.id === id);
      const unchanged =
        next?.version === run.plugin.version && next.granted.join() === run.plugin.granted.join() && next.entryUrl === run.plugin.entryUrl;
      if (!unchanged) {
        this.stop(id);
      }
    }
    for (const plugin of active) {
      if (!this.running.has(plugin.id)) this.start(plugin);
    }
  }

  stopAll(): void {
    for (const id of [...this.running.keys()]) this.stop(id);
  }

  isRunning(id: string): boolean {
    return this.running.has(id);
  }

  private stop(id: string): void {
    const run = this.running.get(id);
    if (!run) return;
    run.port.terminate();
    this.running.delete(id);
    this.ui.setSidebarItems(id, []);
  }

  private start(plugin: PluginInfo): void {
    if (!plugin.entryUrl) return;
    const port = this.spawn(plugin);
    const run: Running = { plugin, port, items: [], notifications: [] };
    this.running.set(plugin.id, run);
    port.onmessage = (event) => {
      void this.receive(run, event.data);
    };
    port.onerror = (message) => {
      this.ui.failed(plugin.name, message);
    };
    port.postMessage({
      type: "init",
      apiVersion: 1,
      plugin: { id: plugin.id, name: plugin.name, version: plugin.version, permissions: [...plugin.granted] },
      entryUrl: plugin.entryUrl,
    });
  }

  /** A sidebar item was clicked (with the picked file, if it asks for one). */
  click(pluginId: string, itemId: string, file: PluginFile | null): void {
    this.running.get(pluginId)?.port.postMessage({ type: "sidebarClick", itemId, file });
  }

  private async receive(run: Running, message: PluginToHost): Promise<void> {
    if (message.type === "failed") {
      this.ui.failed(run.plugin.name, message.message);
      this.stop(run.plugin.id);
      return;
    }
    if (message.type !== "call") return;
    try {
      const result = await this.dispatch(run, message.method, message.params);
      run.port.postMessage({ type: "result", id: message.id, result: result ?? null });
    } catch (error: unknown) {
      const kind = error instanceof PluginCallError || error instanceof CoveError ? error.kind : "plugin";
      const text = error instanceof Error ? error.message : String(error);
      run.port.postMessage({ type: "error", id: message.id, error: { kind, message: text } });
    }
  }

  /** Permission check first, then the call. Exported for tests through `PluginHost.prototype`. */
  async dispatch(run: Running, method: PluginMethod, raw: unknown): Promise<unknown> {
    const permission = (METHOD_PERMISSIONS as Partial<Record<string, string>>)[method];
    if (!permission) throw new PluginCallError("invalidInput", `Unbekannte Methode: ${method}`);
    if (!run.plugin.granted.includes(METHOD_PERMISSIONS[method])) {
      throw new PluginCallError("plugin", `Das Plugin „${run.plugin.name}“ hat keine Berechtigung „${permission}“.`);
    }
    const params = isParams(raw) ? raw : {};
    const id = run.plugin.id;
    switch (method) {
      case "library.list":
        return (await api.library.listLibrary()).map(toPluginGame);
      case "library.add":
        return toPluginGame(
          await api.plugins.libraryAdd(id, { title: text(params, "title", 200), executablePath: optionalText(params, "executablePath") }),
        );
      case "catalog.search": {
        const page = await api.catalog.searchCatalog(text(params, "query", 100));
        return page.items.slice(0, 20).map(
          (g): PluginCatalogGame => ({ id: g.id, title: g.title, releaseDate: g.releaseDate, genres: [...g.genres], rating: g.rating }),
        );
      }
      case "achievements.define": {
        const list = params.achievements;
        if (!Array.isArray(list)) throw new PluginCallError("invalidInput", "„achievements“ muss eine Liste sein.");
        return api.plugins.defineAchievements(id, list);
      }
      case "ui.addSidebarItem":
        this.addSidebarItem(run, params);
        return null;
      case "ui.notify": {
        const now = Date.now();
        run.notifications = run.notifications.filter((t) => now - t < NOTIFY_WINDOW_MS);
        if (run.notifications.length >= NOTIFY_LIMIT) {
          throw new PluginCallError("plugin", "Zu viele Hinweise in kurzer Zeit.");
        }
        run.notifications.push(now);
        this.ui.notify(run.plugin.name, text(params, "message", 200), optionalText(params, "gameId"));
        return null;
      }
      case "storage.get":
        return api.plugins.storageGet(id, text(params, "key", 200));
      case "storage.set":
        await api.plugins.storageSet(id, text(params, "key", 200), params.value ?? null);
        return null;
      case "storage.remove":
        await api.plugins.storageRemove(id, text(params, "key", 200));
        return null;
      case "storage.keys":
        return api.plugins.storageKeys(id);
    }
  }

  private addSidebarItem(run: Running, params: Params): void {
    const itemId = text(params, "id", 64);
    const label = text(params, "label", 40);
    const icon = ICONS.find((i) => i === params.icon) ?? "puzzle";
    let pickFile: SidebarEntry["pickFile"] = null;
    if (isParams(params.pickFile)) {
      const accept = text(params.pickFile, "accept", 100);
      const max = typeof params.pickFile.maxBytes === "number" ? params.pickFile.maxBytes : MAX_FILE_BYTES;
      pickFile = { accept, maxBytes: Math.min(Math.max(1, max), MAX_FILE_BYTES) };
    }
    const others = run.items.filter((i) => i.id !== itemId);
    if (others.length >= MAX_SIDEBAR_ITEMS) {
      throw new PluginCallError("plugin", `Ein Plugin kann höchstens ${String(MAX_SIDEBAR_ITEMS)} Einträge anlegen.`);
    }
    run.items = [...others, { pluginId: run.plugin.id, pluginName: run.plugin.name, id: itemId, label, icon, pickFile }];
    this.ui.setSidebarItems(run.plugin.id, run.items);
  }
}
