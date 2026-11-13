import type { ControllerKind, GamepadInfo } from "@cove/shared";
import { create } from "zustand";
import { api } from "../api/index.ts";

interface GamepadState {
  pads: GamepadInfo[];
  /** The controller last used: its button labels are shown. */
  kind: ControllerKind | null;
  connect: () => () => void;
  setKind: (kind: ControllerKind) => void;
}

/** Connected controllers and which labels to show. */
export const useGamepads = create<GamepadState>()((set) => ({
  pads: [],
  kind: null,
  setKind: (kind) => {
    set({ kind });
  },
  connect: () => {
    api.gamepad.list().then(
      (pads) => {
        set((s) => ({ pads, kind: s.kind ?? pads[0]?.kind ?? null }));
      },
      () => undefined,
    );
    return api.gamepad.subscribeChanged((pads) => {
      set((s) => ({ pads, kind: pads.some((p) => p.kind === s.kind) ? s.kind : (pads[0]?.kind ?? null) }));
    });
  },
}));
