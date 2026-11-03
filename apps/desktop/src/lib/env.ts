import type { Platform } from "@cove/shared";

/** True inside the Tauri webview (the core is reachable). */
export const inTauri: boolean = typeof window !== "undefined" && "__TAURI_INTERNALS__" in window;

/** Best-effort OS detection for UI decisions (e.g. showing Proton options). */
export function detectPlatform(): Platform {
  const ua = typeof navigator === "undefined" ? "" : navigator.userAgent;
  if (/Windows/i.test(ua)) return "windows";
  if (/Mac OS X|Macintosh/i.test(ua)) return "macos";
  return "linux";
}

export const isMac: boolean = detectPlatform() === "macos";

/** "Ctrl" or "⌘" for keyboard hints. */
export const modKeyLabel: string = isMac ? "⌘" : "Ctrl";
