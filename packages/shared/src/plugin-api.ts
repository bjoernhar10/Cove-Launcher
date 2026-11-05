/**
 * Cove plugin API, version 1.
 *
 * Plugins are ES modules that run in their own Web Worker. They export
 * `activate(cove)` and talk to Cove only through the `cove` object below.
 * Every method needs the matching permission in `plugin.json`.
 *
 * Use from plain JavaScript with JSDoc:
 *
 *   /** @param {import("@cove/shared/plugin-api").CoveApi} cove *\/
 *   export function activate(cove) { ... }
 */

import type { PluginPermission } from "./models.ts";

export const PLUGIN_API_VERSION = 1;

/** `plugin.json` */
export interface PluginManifest {
  /** Lowercase letters, digits and dashes, e.g. `random-pick`. */
  id: string;
  name: string;
  /** Semantic version, e.g. `1.2.0`. */
  version: string;
  author: string;
  description?: string;
  /** API version the plugin was written for (currently 1). */
  apiVersion?: number;
  permissions: PluginPermission[];
  /** Entry module relative to the plugin folder, e.g. `index.js`. */
  entry: string;
}

/** A library game as plugins see it (no file paths). */
export interface PluginGame {
  id: string;
  title: string;
  installed: boolean;
  favorite: boolean;
  playtimeMinutes: number;
  lastPlayedAt: string | null;
  genres: string[];
  source: "steam" | "gog" | "epic" | "manual";
}

export interface PluginCatalogGame {
  id: string;
  title: string;
  releaseDate: string | null;
  genres: string[];
  rating: number | null;
}

/** Rules Cove evaluates locally (same as Cove's own achievements). */
export type PluginAchievementRule =
  /** Hours played in sessions tracked by Cove. */
  | { type: "playtimeHours"; hours: number }
  | { type: "libraryGames"; count: number }
  /** A session overlapped this local time window. */
  | { type: "playedBetween"; fromHour: number; toHour: number }
  | { type: "recommendedPlayed"; count: number };

export interface PluginAchievement {
  /** Unique within the plugin; Cove prefixes it with the plugin id. */
  id: string;
  title: string;
  description: string;
  rule: PluginAchievementRule;
}

/** Icons a sidebar item may use (kept small on purpose, matching Cove's style). */
export type PluginIcon = "puzzle" | "dice" | "trophy" | "upload" | "sparkles" | "list" | "star" | "clock";

export interface PluginFile {
  name: string;
  /** File content as text (UTF-8). */
  text: string;
}

export interface SidebarItem {
  /** Unique within the plugin. */
  id: string;
  /** Short label, at most 40 characters. */
  label: string;
  icon?: PluginIcon;
  /**
   * Let the user pick a file when the item is clicked; the content is passed
   * to `onClick`. Only text files up to `maxBytes` (default and maximum 5 MB).
   */
  pickFile?: { accept: string; maxBytes?: number };
  onClick: (event: { file: PluginFile | null }) => void | Promise<void>;
}

export interface NotifyOptions {
  /** Adds an "Öffnen" link to this library game. */
  gameId?: string;
}

export interface CoveApi {
  readonly apiVersion: 1;
  readonly plugin: { id: string; name: string; version: string; permissions: PluginPermission[] };
  /** `library.read` / `library.add` */
  library: {
    list(): Promise<PluginGame[]>;
    /** Without `executablePath` the game is added as not installed. */
    add(game: { title: string; executablePath?: string | null }): Promise<PluginGame>;
  };
  /** `catalog.search` */
  catalog: {
    search(query: string): Promise<PluginCatalogGame[]>;
  };
  /** `achievements.define`: replaces all achievements of this plugin. */
  achievements: {
    define(achievements: PluginAchievement[]): Promise<number>;
  };
  /** `ui.addSidebarItem` / `ui.notify` */
  ui: {
    addSidebarItem(item: SidebarItem): Promise<void>;
    notify(message: string, options?: NotifyOptions): Promise<void>;
  };
  /** `storage`: small JSON values, private to this plugin (1 MB in total). */
  storage: {
    get<T = unknown>(key: string): Promise<T | null>;
    set(key: string, value: unknown): Promise<void>;
    remove(key: string): Promise<void>;
    keys(): Promise<string[]>;
  };
}

/** What the entry module exports. */
export interface PluginModule {
  activate(cove: CoveApi): void | Promise<void>;
  deactivate?(): void | Promise<void>;
}

// ───────────── worker protocol (host ↔ plugin worker) ─────────────

export type PluginMethod =
  | "library.list"
  | "library.add"
  | "catalog.search"
  | "achievements.define"
  | "ui.addSidebarItem"
  | "ui.notify"
  | "storage.get"
  | "storage.set"
  | "storage.remove"
  | "storage.keys";

/** Permission each method needs. */
export const METHOD_PERMISSIONS: Record<PluginMethod, PluginPermission> = {
  "library.list": "library.read",
  "library.add": "library.add",
  "catalog.search": "catalog.search",
  "achievements.define": "achievements.define",
  "ui.addSidebarItem": "ui.addSidebarItem",
  "ui.notify": "ui.notify",
  "storage.get": "storage",
  "storage.set": "storage",
  "storage.remove": "storage",
  "storage.keys": "storage",
};

export type HostToPlugin =
  | {
      type: "init";
      apiVersion: number;
      plugin: { id: string; name: string; version: string; permissions: PluginPermission[] };
      entryUrl: string;
      /** UI language, for the runtime's own error messages. */
      language: "de" | "en";
    }
  | { type: "result"; id: number; result: unknown }
  | { type: "error"; id: number; error: { kind: string; message: string } }
  | { type: "sidebarClick"; itemId: string; file: PluginFile | null };

export type PluginToHost =
  | { type: "call"; id: number; method: PluginMethod; params: unknown }
  | { type: "ready" }
  | { type: "failed"; message: string };
