import type { AppSettings } from "@cove/shared";
import { call } from "./transport.ts";

export function getSettings(): Promise<AppSettings> {
  return call("settings_get", {});
}

export function saveSettings(settings: AppSettings): Promise<AppSettings> {
  return call("settings_update", { settings });
}
