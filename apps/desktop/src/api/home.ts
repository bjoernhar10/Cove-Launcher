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

function isUnavailable(error: unknown, kinds: string[]): boolean {
  return error instanceof CoveError && kinds.includes(error.kind);
}

const CATALOG_DOWN = ["catalogNotConfigured", "offline"];
const RECOMMENDER_DOWN = ["recommenderUnavailable", "recommender"];

/** Catalogue rows stay empty (not an error) while the catalogue is unavailable. */
async function catalogGames(sort: "recommended" | "newest"): Promise<Game[]> {
  try {
    return (await browseCatalog(DEFAULT_CATALOG_FILTER, sort, 0)).items;
  } catch (error: unknown) {
    if (isUnavailable(error, CATALOG_DOWN)) return [];
    throw error;
  }
}

function ownedIds(library: LibraryItem[]): Set<string> {
  return new Set(library.flatMap((i) => [i.game.id, ...(i.entry.catalogId ? [i.entry.catalogId] : [])]));
}

/** Without the recommender: well-rated catalogue games, own genres first. */
async function simpleRecommendations(): Promise<Recommendation[]> {
  const library = await listLibrary();
  if (library.length === 0) return [];
  const owned = ownedIds(library);
  const ownedGenres = new Set(library.flatMap((i) => i.game.genres));
  return (await catalogGames("recommended"))
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

/** "Für dich empfohlen": the local recommender (hybrid, see services/recommender). */
export async function getRecommendations(): Promise<Recommendation[]> {
  try {
    return await call("recommendations_for_user", { limit: 12 });
  } catch (error: unknown) {
    if (isUnavailable(error, RECOMMENDER_DOWN)) return simpleRecommendations();
    if (isUnavailable(error, CATALOG_DOWN)) return [];
    throw error;
  }
}

/** "Neu entdecken": genres the user has not played yet, close to their taste. */
export async function getDiscover(): Promise<Recommendation[]> {
  try {
    return await call("recommendations_discover", { limit: 10 });
  } catch (error: unknown) {
    if (isUnavailable(error, CATALOG_DOWN)) return [];
    if (!isUnavailable(error, RECOMMENDER_DOWN)) throw error;
    const [library, games] = await Promise.all([listLibrary(), catalogGames("newest")]);
    const played = ownedIds(library.filter((i) => i.entry.playtimeMinutes > 0));
    return games
      .filter((g) => !played.has(g.id))
      .slice(0, 10)
      .map((game, index) => ({ game, score: 1 - index / 10, reason: "Neu im Katalog" }));
  }
}

/** Short reason why a catalogue game fits the user. */
export function explainRecommendation(gameId: string): Promise<string> {
  return call("recommendation_explain", { gameId });
}

export function getRecentAchievements(): Promise<AchievementUnlock[]> {
  return call("achievements_recent", { limit: 6 });
}
