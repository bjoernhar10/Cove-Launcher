import type { CatalogFilter } from "@cove/shared";
import { create } from "zustand";
import { DEFAULT_CATALOG_FILTER } from "../api/index.ts";

interface CatalogState {
  filter: CatalogFilter;
  setFilter: (patch: Partial<CatalogFilter>) => void;
  reset: () => void;
}

/** Kept outside the page so filters survive a visit to a detail page. */
export const useCatalogFilter = create<CatalogState>()((set) => ({
  filter: DEFAULT_CATALOG_FILTER,
  setFilter: (patch) => {
    set((s) => ({ filter: { ...s.filter, ...patch } }));
  },
  reset: () => {
    set((s) => ({ filter: { ...DEFAULT_CATALOG_FILTER, sort: s.filter.sort } }));
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
