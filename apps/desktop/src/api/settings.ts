import type { AppSettings, PluginInfo } from "@cove/shared";
import { call } from "./transport.ts";

export function getSettings(): Promise<AppSettings> {
  return call("settings_get", {});
}

export function saveSettings(settings: AppSettings): Promise<AppSettings> {
  return call("settings_update", { settings });
}

export function listPlugins(): Promise<PluginInfo[]> {
  return call("plugins_list", {});
}

export function setPluginEnabled(id: string, enabled: boolean): Promise<PluginInfo> {
  return call("plugins_set_enabled", { id, enabled });
}
