/**
 * Catalogue mock — the only remaining mock. Library, settings, downloads and
 * play sessions come from the Rust core. ONLY `src/api/*` may import this.
 */
import type { CatalogFilter, Game, GameId, Page } from "@cove/shared";
import { createGames } from "./games.ts";
import { hashString } from "./random.ts";

const games = createGames();

function matchesFilter(game: Game, filter: CatalogFilter): boolean {
  const query = filter.query.trim().toLowerCase();
  if (query && !`${game.title} ${game.developer}`.toLowerCase().includes(query)) return false;
  if (filter.genres.length > 0 && !filter.genres.some((g) => game.genres.includes(g))) return false;
  if (filter.platforms.length > 0 && !filter.platforms.some((p) => game.platforms.includes(p))) return false;
  if (filter.releaseYears) {
    const year = Number(game.releaseDate.slice(0, 4));
    if (year < filter.releaseYears.from || year > filter.releaseYears.to) return false;
  }
  if (filter.minRating !== null && (game.rating ?? 0) < filter.minRating) return false;
  return true;
}

export function recommendationScore(game: Game): number {
  return ((hashString(game.id) % 1000) / 1000) * 0.5 + game.popularity / 200;
}

function sortGames(list: Game[], sort: CatalogFilter["sort"]): Game[] {
  const sorted = [...list];
  switch (sort) {
    case "popular":
      return sorted.sort((a, b) => b.popularity - a.popularity);
    case "newest":
      return sorted.sort((a, b) => b.releaseDate.localeCompare(a.releaseDate));
    case "title":
      return sorted.sort((a, b) => a.title.localeCompare(b.title, "de"));
    case "recommended":
      return sorted.sort((a, b) => recommendationScore(b) - recommendationScore(a));
  }
}

function gameById(gameId: GameId): Game {
  const game = games.find((g) => g.id === gameId);
  if (!game) throw new Error(`Spiel nicht gefunden: ${gameId}`);
  return game;
}

export const mockCatalog = {
  list(filter: CatalogFilter, cursor: string | null, pageSize: number): Page<Game> {
    const filtered = sortGames(
      games.filter((g) => matchesFilter(g, filter)),
      filter.sort,
    );
    const start = cursor === null ? 0 : Number(cursor);
    const end = start + pageSize;
    return {
      items: filtered.slice(start, end),
      nextCursor: end < filtered.length ? String(end) : null,
      total: filtered.length,
    };
  },
  get: (gameId: GameId): Game => gameById(gameId),
  similar(gameId: GameId): Game[] {
    const game = gameById(gameId);
    const shared = (g: Game) => g.genres.filter((x) => game.genres.includes(x)).length;
    return games
      .filter((g) => g.id !== gameId && shared(g) > 0)
      .sort((a, b) => shared(b) * 100 + b.popularity - (shared(a) * 100 + a.popularity))
      .slice(0, 8);
  },
  search(query: string): Game[] {
    const q = query.trim().toLowerCase();
    if (!q) return [];
    return games.filter((g) => g.title.toLowerCase().includes(q)).slice(0, 8);
  },
  /** Stand-in for the Python recommender: catalogue games ranked by a stable score. */
  recommended: (): Game[] => sortGames(games, "recommended"),
  newest: (): Game[] => sortGames(games, "newest"),
};
