import type { AchievementUnlock, Game, LibraryItem, Recommendation } from "@cove/shared";
import { browseCatalog, DEFAULT_CATALOG_FILTER } from "./catalog.ts";
import { listLibrary } from "./library.ts";
import { call, CoveError } from "./transport.ts";

/** Most recently played library games. */
export async function getContinuePlaying(): Promise<LibraryItem[]> {
  const items = await listLibrary();
  return items
    .filter((i) => i.entry.lastPlayedAt !== null)
    .sort((a, b) => (b.entry.lastPlayedAt ?? "").localeCompare(a.entry.lastPlayedAt ?? ""))
    .slice(0, 3);
}

/** Catalogue rows on Home stay empty (not an error) while the catalogue is not set up. */
async function catalogGames(sort: "recommended" | "newest"): Promise<Game[]> {
  try {
    return (await browseCatalog(DEFAULT_CATALOG_FILTER, sort, 0)).items;
  } catch (error: unknown) {
    if (error instanceof CoveError && (error.kind === "catalogNotConfigured" || error.kind === "offline")) return [];
    throw error;
  }
}

function ownedIds(library: LibraryItem[]): Set<string> {
  return new Set(library.flatMap((i) => [i.game.id, ...(i.entry.catalogId ? [i.entry.catalogId] : [])]));
}

/**
 * Well-rated catalogue games the user does not own, favouring their genres.
 * A placeholder ranking until the Python recommender is connected.
 */
export async function getRecommendations(): Promise<Recommendation[]> {
  const library = await listLibrary();
  if (library.length === 0) return [];
  const games = await catalogGames("recommended");
  const owned = ownedIds(library);
  const ownedGenres = new Set(library.flatMap((i) => i.game.genres));
  return games
    .filter((g) => !owned.has(g.id))
    .map((game) => ({ game, shared: game.genres.some((g) => ownedGenres.has(g)) }))
    .sort((a, b) => Number(b.shared) - Number(a.shared))
    .slice(0, 10)
    .map(({ game, shared }, index) => ({
      game,
      score: 1 - index / 10,
      reason: shared ? "Passt zu deinen Genres" : "Hoch bewertet",
    }));
}

/** New catalogue releases the user has not played. */
export async function getDiscover(): Promise<Game[]> {
  const [library, games] = await Promise.all([listLibrary(), catalogGames("newest")]);
  const played = ownedIds(library.filter((i) => i.entry.playtimeMinutes > 0));
  return games.filter((g) => !played.has(g.id)).slice(0, 10);
}

export function getRecentAchievements(): Promise<AchievementUnlock[]> {
  return call("achievements_recent", { limit: 6 });
}
