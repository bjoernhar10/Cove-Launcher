/**
 * Runs inside the plugin's Web Worker: builds the `cove` API object, loads
 * the plugin module and forwards every call to the host as a message. No
 * imports from the app here, the worker has no access to Tauri or the DOM.
 */
import type {
  CoveApi,
  HostToPlugin,
  PluginFile,
  PluginMethod,
  PluginToHost,
  SidebarItem,
} from "@cove/shared";
import { PLUGIN_API_VERSION } from "@cove/shared/plugin-api";

export interface RuntimePort {
  postMessage(message: PluginToHost): void;
  onmessage: ((event: { data: HostToPlugin }) => void) | null;
}

export type ModuleLoader = (url: string) => Promise<unknown>;

/** Error a plugin sees when Cove rejects a call (e.g. missing permission). */
export class CovePluginError extends Error {
  readonly kind: string;
  constructor(kind: string, message: string) {
    super(message);
    this.name = "CovePluginError";
    this.kind = kind;
  }
}

function activateOf(module: unknown): ((cove: CoveApi) => unknown) | null {
  if (typeof module !== "object" || module === null) return null;
  const candidate: unknown = (module as { activate?: unknown }).activate ?? (module as { default?: { activate?: unknown } }).default?.activate;
  return typeof candidate === "function" ? (candidate as (cove: CoveApi) => unknown) : null;
}

export function runPlugin(port: RuntimePort, load: ModuleLoader): void {
  let nextId = 1;
  const pending = new Map<number, { resolve: (value: unknown) => void; reject: (error: Error) => void }>();
  const clickHandlers = new Map<string, SidebarItem["onClick"]>();

  function call<T>(method: PluginMethod, params: unknown = null): Promise<T> {
    return new Promise<T>((resolve, reject) => {
      const id = nextId++;
      pending.set(id, { resolve: (value) => { resolve(value as T); }, reject });
      port.postMessage({ type: "call", id, method, params });
    });
  }

  async function start(init: Extract<HostToPlugin, { type: "init" }>): Promise<void> {
    if (init.apiVersion !== PLUGIN_API_VERSION) {
      port.postMessage({ type: "failed", message: `Plugin-API ${String(init.apiVersion)} wird nicht unterstützt.` });
      return;
    }
    const cove: CoveApi = Object.freeze({
      apiVersion: 1 as const,
      plugin: Object.freeze({ ...init.plugin, permissions: [...init.plugin.permissions] }),
      library: Object.freeze({
        list: () => call("library.list"),
        add: (game: { title: string; executablePath?: string | null }) =>
          call("library.add", { title: game.title, executablePath: game.executablePath ?? null }),
      }),
      catalog: Object.freeze({ search: (query: string) => call("catalog.search", { query }) }),
      achievements: Object.freeze({ define: (achievements: unknown[]) => call("achievements.define", { achievements }) }),
      ui: Object.freeze({
        addSidebarItem: async (item: SidebarItem) => {
          if (typeof item.onClick !== "function") throw new CovePluginError("invalidInput", "Ein Seitenleisten-Eintrag braucht onClick.");
          await call("ui.addSidebarItem", { id: item.id, label: item.label, icon: item.icon ?? "puzzle", pickFile: item.pickFile ?? null });
          clickHandlers.set(item.id, item.onClick);
        },
        notify: (message: string, options?: { gameId?: string }) => call("ui.notify", { message, gameId: options?.gameId ?? null }),
      }),
      storage: Object.freeze({
        get: (key: string) => call("storage.get", { key }),
        set: (key: string, value: unknown) => call("storage.set", { key, value }),
        remove: (key: string) => call("storage.remove", { key }),
        keys: () => call("storage.keys"),
      }),
    }) as CoveApi;
    try {
      const activate = activateOf(await load(init.entryUrl));
      if (!activate) throw new Error("Das Plugin exportiert keine Funktion „activate“.");
      await activate(cove);
      port.postMessage({ type: "ready" });
    } catch (error: unknown) {
      port.postMessage({ type: "failed", message: error instanceof Error ? error.message : String(error) });
    }
  }

  async function click(itemId: string, file: PluginFile | null): Promise<void> {
    const handler = clickHandlers.get(itemId);
    try {
      await handler?.({ file });
    } catch (error: unknown) {
      // Shown to the user by the host as a plugin error.
      console.error(error);
    }
  }

  port.onmessage = (event) => {
    const message = event.data;
    switch (message.type) {
      case "init":
        void start(message);
        break;
      case "result":
        pending.get(message.id)?.resolve(message.result);
        pending.delete(message.id);
        break;
      case "error":
        pending.get(message.id)?.reject(new CovePluginError(message.error.kind, message.error.message));
        pending.delete(message.id);
        break;
      case "sidebarClick":
        void click(message.itemId, message.file);
        break;
    }
  };
}
