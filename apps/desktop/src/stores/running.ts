import type { GameId, RunningGame } from "@cove/shared";
import { create } from "zustand";
import { api } from "../api/index.ts";
import { formatPlaytime } from "../lib/format.ts";
import { invalidateQueries } from "./query.ts";
import { toast } from "./toasts.ts";

interface RunningState {
  games: Record<GameId, RunningGame>;
  /** Loads running games and listens for `game-state-changed`. Returns the unsubscribe function. */
  connect: () => () => void;
}

export const useRunningGames = create<RunningState>()((set, get) => ({
  games: {},
  connect: () => {
    let active = true;
    api.library.listRunningGames().then(
      (list) => {
        if (active) set({ games: Object.fromEntries(list.map((g) => [g.gameId, g])) });
      },
      () => undefined, // browser preview without core
    );
    const unsubscribe = api.library.subscribeGameState((event) => {
      if (event.state === "running") {
        set({ games: { ...get().games, [event.gameId]: { gameId: event.gameId, startedAt: event.startedAt } } });
      } else {
        set({ games: Object.fromEntries(Object.entries(get().games).filter(([id]) => id !== event.gameId)) });
        const minutes = Math.round((event.sessionSeconds ?? 0) / 60);
        if (minutes > 0) toast(`Sitzung gespeichert: ${formatPlaytime(minutes)}`);
      }
      invalidateQueries();
    });
    return () => {
      active = false;
      unsubscribe();
    };
  },
}));
