import type { PlayStats } from "@cove/shared";
import { call } from "./transport.ts";

/** The statistics page: a year, or all time (null). */
export function overview(year: number | null): Promise<PlayStats> {
  return call("stats_overview", { year });
}
