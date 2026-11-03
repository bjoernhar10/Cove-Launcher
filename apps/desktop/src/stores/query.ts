import { create } from "zustand";

interface QueryState {
  /** Bumped on every mutation; all queries refetch silently. */
  version: number;
  invalidate: () => void;
}

export const useQueryVersion = create<QueryState>()((set) => ({
  version: 0,
  invalidate: () => {
    set((s) => ({ version: s.version + 1 }));
  },
}));

/** Call after any mutation so every visible view reloads its data. */
export function invalidateQueries(): void {
  useQueryVersion.getState().invalidate();
}
