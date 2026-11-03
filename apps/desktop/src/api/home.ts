import type { AchievementUnlock, Game, LibraryItem, Recommendation } from "@cove/shared";
import { newestCatalogGames, recommendedCatalogGames } from "./catalog.ts";
import { listLibrary } from "./library.ts";
import { call } from "./transport.ts";

/** Most recently played library games. */
export async function getContinuePlaying(): Promise<LibraryItem[]> {
  const items = await listLibrary();
  return items
    .filter((i) => i.entry.lastPlayedAt !== null)
    .sort((a, b) => (b.entry.lastPlayedAt ?? "").localeCompare(a.entry.lastPlayedAt ?? ""))
    .slice(0, 3);
}

/**
 * Catalogue games not in the library. Ranking is a placeholder until the
 * Python recommender is connected.
 */
export async function getRecommendations(): Promise<Recommendation[]> {
  const [library, games] = await Promise.all([listLibrary(), recommendedCatalogGames()]);
  if (library.length === 0) return [];
  const owned = new Set(library.map((i) => i.game.id));
  const ownedGenres = new Set(library.flatMap((i) => i.game.genres));
  return games
    .filter((g) => !owned.has(g.id))
    .slice(0, 10)
    .map((game, index) => {
      const genre = game.genres.find((g) => ownedGenres.has(g));
      return { game, score: 1 - index / 10, reason: genre ? "Passt zu deinen Genres" : "Beliebt im Katalog" };
    });
}

/** Catalogue games the user has never played (newest first). */
export async function getDiscover(): Promise<Game[]> {
  const [library, games] = await Promise.all([listLibrary(), newestCatalogGames()]);
  const played = new Set(library.filter((i) => i.entry.playtimeMinutes > 0).map((i) => i.game.id));
  return games.filter((g) => !played.has(g.id)).slice(0, 8);
}

export function getRecentAchievements(): Promise<AchievementUnlock[]> {
  return call("achievements_recent", { limit: 6 });
}
