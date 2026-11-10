import type { GameId, GameMoveProgress } from "@cove/shared";
import { create } from "zustand";
import { api } from "../api/index.ts";
import { messages } from "../i18n/index.ts";
import { invalidateQueries } from "./query.ts";
import { toast } from "./toasts.ts";

function withoutGame(moves: Record<GameId, GameMoveProgress>, gameId: GameId): Record<GameId, GameMoveProgress> {
  return Object.fromEntries(Object.entries(moves).filter(([id]) => id !== gameId));
}

interface MovesState {
  /** Games being moved, with their latest progress. */
  moves: Record<GameId, GameMoveProgress>;
  titles: Record<GameId, string>;
  /** Starts "Spiel verschieben"; errors before the start are thrown. */
  start: (gameId: GameId, title: string, folder: string) => Promise<void>;
  /** Listens for `game-move-progress`. Returns the unsubscribe function. */
  connect: () => () => void;
}

export const useMoves = create<MovesState>()((set, get) => ({
  moves: {},
  titles: {},
  start: async (gameId, title, folder) => {
    // Before the call: on the same drive the move can finish before the call returns.
    set((s) => ({
      titles: { ...s.titles, [gameId]: title },
      moves: { ...s.moves, [gameId]: { gameId, status: "copying", copiedBytes: 0, totalBytes: 0, error: null } },
    }));
    try {
      await api.library.moveGame(gameId, folder);
    } catch (error: unknown) {
      set((s) => ({ moves: withoutGame(s.moves, gameId) }));
      throw error;
    }
  },
  connect: () =>
    api.library.subscribeMoveProgress((progress) => {
      const title = get().titles[progress.gameId] ?? "";
      if (progress.status === "copying" || progress.status === "verifying") {
        set((s) => ({ moves: { ...s.moves, [progress.gameId]: progress } }));
        return;
      }
      set((s) => ({ moves: withoutGame(s.moves, progress.gameId) }));
      if (progress.status === "completed") toast(messages().game.moved(title));
      else toast(messages().game.moveFailed(title, progress.error ?? messages().common.genericError), "danger");
      invalidateQueries();
    }),
}));
