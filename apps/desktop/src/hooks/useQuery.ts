import { useCallback, useEffect, useRef, useState } from "react";
import { errorMessage } from "../stores/toasts.ts";
import { useQueryVersion } from "../stores/query.ts";

export type QueryResult<T> =
  | { status: "loading"; data?: undefined; error?: undefined }
  | { status: "error"; data?: undefined; error: string }
  | { status: "success"; data: T; error?: undefined };

type Settled<T> = { key: string; ok: true; data: T } | { key: string; ok: false; error: string };

/**
 * Minimal data hook: loads `fetcher()` whenever `key` changes and silently
 * refetches after any mutation (see `invalidateQueries`). Keeps showing the
 * previous data while refetching the same key, so there is no skeleton flicker.
 */
export function useQuery<T>(
  key: string,
  fetcher: () => Promise<T>,
): QueryResult<T> & { retry: () => void; update: (fn: (data: T) => T) => void } {
  const version = useQueryVersion((s) => s.version);
  const [attempt, setAttempt] = useState(0);
  const [settled, setSettled] = useState<Settled<T> | null>(null);
  const fetcherRef = useRef(fetcher);

  useEffect(() => {
    fetcherRef.current = fetcher;
  });

  useEffect(() => {
    let cancelled = false;
    fetcherRef.current().then(
      (data) => {
        if (!cancelled) setSettled({ key, ok: true, data });
      },
      (error: unknown) => {
        if (!cancelled) setSettled({ key, ok: false, error: errorMessage(error) });
      },
    );
    return () => {
      cancelled = true;
    };
  }, [key, version, attempt]);

  const retry = useCallback(() => {
    setSettled(null);
    setAttempt((n) => n + 1);
  }, []);

  const update = useCallback(
    (fn: (data: T) => T) => {
      setSettled((prev) => (prev?.ok && prev.key === key ? { ...prev, data: fn(prev.data) } : prev));
    },
    [key],
  );

  if (settled?.key !== key) return { status: "loading", retry, update };
  if (!settled.ok) return { status: "error", error: settled.error, retry, update };
  return { status: "success", data: settled.data, retry, update };
}
