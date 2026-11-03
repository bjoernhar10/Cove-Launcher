import type { CatalogFilter, CatalogSort } from "@cove/shared";
import { useInfiniteQuery, useQuery } from "@tanstack/react-query";
import { api } from "../api/index.ts";

export const catalogKeys = {
  all: ["catalog"] as const,
  browse: (filter: CatalogFilter, sort: CatalogSort) => ["catalog", "browse", filter, sort] as const,
  details: (id: string) => ["catalog", "details", id] as const,
  similar: (id: string) => ["catalog", "similar", id] as const,
  search: (query: string) => ["catalog", "search", query] as const,
  status: ["catalog", "status"] as const,
};

/** Infinite catalogue list; the next page loads when `fetchNextPage` is called. */
export function useCatalogBrowse(filter: CatalogFilter, sort: CatalogSort) {
  return useInfiniteQuery({
    queryKey: catalogKeys.browse(filter, sort),
    queryFn: ({ pageParam }) => api.catalog.browseCatalog(filter, sort, pageParam),
    initialPageParam: 0,
    getNextPageParam: (last) => (last.hasMore ? last.page + 1 : undefined),
  });
}

export function useCatalogGame(id: string) {
  return useQuery({ queryKey: catalogKeys.details(id), queryFn: () => api.catalog.getCatalogGame(id) });
}

export function useSimilarGames(id: string) {
  return useQuery({ queryKey: catalogKeys.similar(id), queryFn: () => api.catalog.getSimilarGames(id) });
}

export function useCatalogSearch(query: string) {
  return useQuery({
    queryKey: catalogKeys.search(query),
    queryFn: () => api.catalog.searchCatalog(query),
    enabled: query.trim().length > 1,
  });
}

export function useCatalogStatus() {
  return useQuery({ queryKey: catalogKeys.status, queryFn: api.catalog.getCatalogStatus, staleTime: 0 });
}
