import type {
  CatalogCredentials,
  CatalogFilter,
  CatalogGameDetails,
  CatalogPage,
  CatalogProviderId,
  CatalogSort,
  CatalogStatus,
  TimeToBeat,
} from "@cove/shared";
import { call } from "./transport.ts";

export const DEFAULT_CATALOG_FILTER: CatalogFilter = {
  query: "",
  genres: [],
  platforms: [],
  releaseYears: null,
  minRating: null,
};

export const DEFAULT_CATALOG_SORT: CatalogSort = "recommended";

/** One page (0-based) from IGDB/RAWG — cached by the core for 7 days, stale data when offline. */
export function browseCatalog(filter: CatalogFilter, sort: CatalogSort, page: number): Promise<CatalogPage> {
  return call("catalog_browse", { filter, sort, page });
}

export function searchCatalog(query: string): Promise<CatalogPage> {
  return call("catalog_search", { query });
}

export function getCatalogGame(gameId: string): Promise<CatalogGameDetails> {
  return call("catalog_game_details", { gameId });
}

export function getSimilarGames(gameId: string): Promise<CatalogPage> {
  return call("catalog_similar", { gameId });
}

/** How long games take (IGDB, from player reports), per catalogue id; ids without data are missing. */
export function timeToBeat(catalogIds: string[]): Promise<Record<string, TimeToBeat>> {
  if (catalogIds.length === 0) return Promise.resolve({});
  return call("catalog_time_to_beat", { catalogIds });
}

export function getCatalogStatus(): Promise<CatalogStatus> {
  return call("catalog_status", {});
}

/** Verifies the credentials with the provider and stores them in the OS keyring. */
export function setCatalogCredentials(credentials: CatalogCredentials): Promise<CatalogStatus> {
  return call("catalog_set_credentials", { credentials });
}

export function clearCatalogCredentials(provider: CatalogProviderId): Promise<CatalogStatus> {
  return call("catalog_clear_credentials", { provider });
}
