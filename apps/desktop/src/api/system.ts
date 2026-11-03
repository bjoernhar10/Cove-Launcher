import type { AboutInfo, UserProfile } from "@cove/shared";
import { call } from "./transport.ts";

export function getProfile(): Promise<UserProfile> {
  return call("system_profile", {});
}

export function getAbout(): Promise<AboutInfo> {
  return call("system_about", {});
}

/** Opens a web link in the user's default browser. */
export async function openExternal(url: string): Promise<void> {
  if (!/^https?:\/\//.test(url)) throw new Error(`Nur Web-Links erlaubt: ${url}`);
  if (typeof window !== "undefined" && "__TAURI_INTERNALS__" in window) {
    const { openUrl } = await import("@tauri-apps/plugin-opener");
    await openUrl(url);
  } else {
    window.open(url, "_blank", "noopener");
  }
}
