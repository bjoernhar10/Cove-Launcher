import type { AboutInfo, FolderInfo, UpdateCheck, UpdateProgress, UserProfile } from "@cove/shared";
import { call, subscribe } from "./transport.ts";

export function getProfile(): Promise<UserProfile> {
  return call("system_profile", {});
}

export function getAbout(): Promise<AboutInfo> {
  return call("system_about", {});
}

/** Native folder picker, starting at `defaultPath`; null when cancelled. */
export function pickFolder(defaultPath: string | null): Promise<string | null> {
  return call("system_pick_folder", { defaultPath });
}

/** Free space, write access and the "Program Files" warning for a folder. */
export function folderInfo(path: string): Promise<FolderInfo> {
  return call("system_folder_info", { path });
}

/** Asks the release server whether a newer Cove exists. */
export function checkForUpdates(): Promise<UpdateCheck> {
  return call("updates_check", {});
}

/** Downloads, verifies and installs the update; Cove restarts when done. */
export async function installUpdate(): Promise<void> {
  await call("updates_install", {});
}

export function subscribeUpdateProgress(handler: (progress: UpdateProgress) => void): () => void {
  return subscribe("update-progress", handler);
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
