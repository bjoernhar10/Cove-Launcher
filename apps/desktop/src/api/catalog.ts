import type { CatalogFilter, Game, GameId, Page } from "@cove/shared";
import { mockCatalog } from "./transport.ts";

export const CATALOG_PAGE_SIZE = 12;

export const DEFAULT_CATALOG_FILTER: CatalogFilter = {
  query: "",
  genres: [],
  platforms: [],
  releaseYears: null,
  minRating: null,
  sort: "recommended",
};

// The catalogue is still mocked. Later: invoke("catalog_list", …) via the connectors.

export function listCatalog(filter: CatalogFilter, cursor: string | null): Promise<Page<Game>> {
  return mockCatalog((c) => c.list(filter, cursor, CATALOG_PAGE_SIZE));
}

export function getGame(gameId: GameId): Promise<Game> {
  return mockCatalog((c) => c.get(gameId));
}

export function getSimilarGames(gameId: GameId): Promise<Game[]> {
  return mockCatalog((c) => c.similar(gameId));
}

export function searchCatalog(query: string): Promise<Game[]> {
  return mockCatalog((c) => c.search(query));
}

export function recommendedCatalogGames(): Promise<Game[]> {
  return mockCatalog((c) => c.recommended());
}

export function newestCatalogGames(): Promise<Game[]> {
  return mockCatalog((c) => c.newest());
}
