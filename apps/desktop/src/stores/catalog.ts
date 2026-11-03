import type { CatalogFilter, CatalogSort } from "@cove/shared";
import { create } from "zustand";
import { DEFAULT_CATALOG_FILTER, DEFAULT_CATALOG_SORT } from "../api/index.ts";

interface CatalogState {
  filter: CatalogFilter;
  sort: CatalogSort;
  setFilter: (patch: Partial<CatalogFilter>) => void;
  setSort: (sort: CatalogSort) => void;
  reset: () => void;
}

/** Kept outside the page so filters survive a visit to a detail page. */
export const useCatalogFilter = create<CatalogState>()((set) => ({
  filter: DEFAULT_CATALOG_FILTER,
  sort: DEFAULT_CATALOG_SORT,
  setFilter: (patch) => {
    set((s) => ({ filter: { ...s.filter, ...patch } }));
  },
  setSort: (sort) => {
    set({ sort });
  },
  reset: () => {
    set({ filter: DEFAULT_CATALOG_FILTER });
  },
}));

export function activeFilterCount(filter: CatalogFilter): number {
  return (
    filter.genres.length +
    filter.platforms.length +
    (filter.releaseYears ? 1 : 0) +
    (filter.minRating !== null ? 1 : 0) +
    (filter.query.trim() ? 1 : 0)
  );
}
