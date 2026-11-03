import type { CatalogFilter, Game } from "@cove/shared";
import { useCallback, useEffect, useRef, useState } from "react";
import { api } from "../api/index.ts";
import { errorMessage } from "../stores/toasts.ts";

interface PagesState {
  key: string;
  items: Game[];
  nextCursor: string | null;
  total: number;
  error: string | null;
}

export interface InfiniteCatalog {
  status: "loading" | "error" | "success";
  items: Game[];
  total: number;
  hasMore: boolean;
  loadingMore: boolean;
  error: string | null;
  loadMore: () => void;
  retry: () => void;
}

/** Cursor-paginated catalogue. Resets whenever the filter changes. */
export function useInfiniteCatalog(filter: CatalogFilter): InfiniteCatalog {
  const key = JSON.stringify(filter);
  const [pages, setPages] = useState<PagesState | null>(null);
  const [loadingMore, setLoadingMore] = useState(false);
  const [attempt, setAttempt] = useState(0);
  const inFlight = useRef(false);
  const filterRef = useRef(filter);

  useEffect(() => {
    filterRef.current = filter;
  });

  useEffect(() => {
    let cancelled = false;
    api.catalog.listCatalog(filterRef.current, null).then(
      (page) => {
        if (!cancelled) setPages({ key, items: page.items, nextCursor: page.nextCursor, total: page.total, error: null });
      },
      (error: unknown) => {
        if (!cancelled) setPages({ key, items: [], nextCursor: null, total: 0, error: errorMessage(error) });
      },
    );
    return () => {
      cancelled = true;
    };
  }, [key, attempt]);

  const current = pages?.key === key ? pages : null;

  const loadMore = useCallback(() => {
    if (!current?.nextCursor || inFlight.current) return;
    inFlight.current = true;
    setLoadingMore(true);
    const requestKey = current.key;
    api.catalog
      .listCatalog(filterRef.current, current.nextCursor)
      .then(
        (page) => {
          setPages((prev) =>
            prev?.key === requestKey
              ? { ...prev, items: [...prev.items, ...page.items], nextCursor: page.nextCursor, total: page.total }
              : prev,
          );
        },
        (error: unknown) => {
          setPages((prev) => (prev?.key === requestKey ? { ...prev, error: errorMessage(error) } : prev));
        },
      )
      .finally(() => {
        inFlight.current = false;
        setLoadingMore(false);
      });
  }, [current]);

  const retry = useCallback(() => {
    setPages(null);
    setAttempt((n) => n + 1);
  }, []);

  if (!current) {
    return { status: "loading", items: [], total: 0, hasMore: false, loadingMore: false, error: null, loadMore, retry };
  }
  return {
    status: current.error && current.items.length === 0 ? "error" : "success",
    items: current.items,
    total: current.total,
    hasMore: current.nextCursor !== null,
    loadingMore,
    error: current.error,
    loadMore,
    retry,
  };
}
