import type { AppSettings, ConnectableStore, PluginInfo, StoreAccount } from "@cove/shared";
import { call } from "./transport.ts";

export function getSettings(): Promise<AppSettings> {
  return call("settings_get", {});
}

export function saveSettings(settings: AppSettings): Promise<AppSettings> {
  return call("settings_update", { settings });
}

export function listAccounts(): Promise<StoreAccount[]> {
  return call("sources_list", {});
}

/** Not available yet — the core answers with a `notSupported` error until the connectors exist. */
export function connectStore(store: ConnectableStore): Promise<StoreAccount> {
  return call("sources_connect", { store });
}

export function listPlugins(): Promise<PluginInfo[]> {
  return call("plugins_list", {});
}

export function setPluginEnabled(id: string, enabled: boolean): Promise<PluginInfo> {
  return call("plugins_set_enabled", { id, enabled });
}
