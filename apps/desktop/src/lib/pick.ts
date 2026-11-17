import type { Collection, CollectionId, LibraryItem, TimeToBeat } from "@cove/shared";
import type { LengthFilter } from "../stores/preferences.ts";
import { inBand } from "./lengths.ts";

/** "Was soll ich spielen?": which games may come up. */
export interface PickFilters {
  /** Only installed games, or the whole library. */
  installedOnly: boolean;
  /** `new`: hardly played (under an hour); `resume`: played, but not for a month. */
  kind: "any" | "new" | "resume";
  length: LengthFilter;
  collection: CollectionId | null;
}

export const DEFAULT_PICK: PickFilters = {
  installedOnly: true,
  kind: "any",
  length: "any",
  collection: null,
};

const HOUR = 60;
const MONTH_MS = 30 * 24 * 3600 * 1000;

function matchesKind(item: LibraryItem, kind: PickFilters["kind"], now: Date): boolean {
  const { playtimeMinutes, lastPlayedAt } = item.entry;
  if (kind === "new") return playtimeMinutes < HOUR;
  if (kind === "resume") {
    if (playtimeMinutes < HOUR) return false;
    return lastPlayedAt === null || now.getTime() - Date.parse(lastPlayedAt) > MONTH_MS;
  }
  return true;
}

/** The games the filters allow. `times` is keyed by catalogue id (only needed for a length). */
export function pickCandidates(
  items: readonly LibraryItem[],
  filters: PickFilters,
  times: Readonly<Record<string, TimeToBeat>>,
  collections: readonly Collection[],
  now: Date = new Date(),
): LibraryItem[] {
  const members = filters.collection ? new Set(collections.find((c) => c.id === filters.collection)?.gameIds ?? []) : null;
  return items.filter((item) => {
    if (filters.installedOnly && !item.entry.installed) return false;
    if (members && !members.has(item.game.id)) return false;
    if (!matchesKind(item, filters.kind, now)) return false;
    if (filters.length === "any") return true;
    const id = item.entry.catalogId;
    return inBand(id ? times[id] : undefined, filters.length);
  });
}

/**
 * One game at random. Games shown recently (`recent`, newest last) are skipped
 * while others are left, so "Nochmal" doesn't bring the same game back.
 */
export function pickGame(
  candidates: readonly LibraryItem[],
  recent: readonly string[],
  random: () => number = Math.random,
): LibraryItem | null {
  if (candidates.length === 0) return null;
  const fresh = candidates.filter((item) => !recent.includes(item.game.id));
  let pool: readonly LibraryItem[] = fresh;
  if (fresh.length === 0) {
    // Everything was shown: start over, but not with the game on screen.
    const last = recent.at(-1);
    const others = candidates.filter((item) => item.game.id !== last);
    pool = others.length > 0 ? others : candidates;
  }
  return pool[Math.min(pool.length - 1, Math.floor(random() * pool.length))] ?? null;
}
