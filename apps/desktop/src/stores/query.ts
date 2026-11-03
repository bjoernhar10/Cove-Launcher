import { create } from "zustand";
import { queryClient } from "../lib/queryClient.ts";

interface QueryState {
  /** Bumped on every mutation; all `useQuery` hooks refetch silently. */
  version: number;
  invalidate: () => void;
}

export const useQueryVersion = create<QueryState>()((set) => ({
  version: 0,
  invalidate: () => {
    set((s) => ({ version: s.version + 1 }));
  },
}));

/** Call after any mutation so every visible view reloads its data (own hooks and TanStack Query). */
export function invalidateQueries(): void {
  useQueryVersion.getState().invalidate();
  void queryClient.invalidateQueries();
}
