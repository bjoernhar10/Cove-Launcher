import type { LibraryItem, PluginGameInput, PluginInfo, PluginManifestPreview } from "@cove/shared";
import { call } from "./transport.ts";

export function listPlugins(): Promise<PluginInfo[]> {
  return call("plugins_list", {});
}

export function setPluginEnabled(id: string, enabled: boolean): Promise<PluginInfo> {
  return call("plugins_set_enabled", { id, enabled });
}

/** Call only after the user confirmed the permissions. */
export function approvePlugin(id: string): Promise<PluginInfo> {
  return call("plugins_approve", { id });
}

export function inspectPlugin(path: string): Promise<PluginManifestPreview> {
  return call("plugins_inspect", { path });
}

/** Call only after the user confirmed the permissions. */
export function installPlugin(path: string): Promise<PluginInfo> {
  return call("plugins_install", { path });
}

export async function uninstallPlugin(id: string): Promise<void> {
  await call("plugins_uninstall", { id });
}

export async function openPluginFolder(): Promise<void> {
  await call("plugins_open_folder", {});
}

// ───────────── plugin API (used by the plugin host only) ─────────────

export function storageGet(pluginId: string, key: string): Promise<unknown> {
  return call("plugin_storage_get", { pluginId, key });
}

export async function storageSet(pluginId: string, key: string, value: unknown): Promise<void> {
  await call("plugin_storage_set", { pluginId, key, value });
}

export async function storageRemove(pluginId: string, key: string): Promise<void> {
  await call("plugin_storage_remove", { pluginId, key });
}

export function storageKeys(pluginId: string): Promise<string[]> {
  return call("plugin_storage_keys", { pluginId });
}

export function defineAchievements(pluginId: string, achievements: unknown[]): Promise<number> {
  return call("plugin_define_achievements", { pluginId, achievements });
}

export function libraryAdd(pluginId: string, game: PluginGameInput): Promise<LibraryItem> {
  return call("plugin_library_add", { pluginId, game });
}
