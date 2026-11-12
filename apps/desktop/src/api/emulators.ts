import type { RomScanResult, SystemInfo } from "@cove/shared";
import { call } from "./transport.ts";

/** The systems Cove plays ROMs of, with their emulators (installed or not). */
export function systems(): Promise<SystemInfo[]> {
  return call("emulators_systems", {});
}

/** Adds the ROMs of every ROM folder to the library (a collection per system). */
export function scan(): Promise<RomScanResult> {
  return call("emulators_scan", {});
}
