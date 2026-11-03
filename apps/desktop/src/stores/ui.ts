import { create } from "zustand";

export interface AddGameOptions {
  mode: "manual" | "scan";
  /** Pre-filled folder; the scan starts immediately. */
  folder?: string;
}

interface UiState {
  paletteOpen: boolean;
  addGame: AddGameOptions | null;
  setPaletteOpen: (open: boolean) => void;
  openAddGame: (options?: AddGameOptions) => void;
  closeAddGame: () => void;
}

export const useUi = create<UiState>()((set) => ({
  paletteOpen: false,
  addGame: null,
  setPaletteOpen: (paletteOpen) => {
    set({ paletteOpen });
  },
  openAddGame: (options = { mode: "manual" }) => {
    set({ addGame: options });
  },
  closeAddGame: () => {
    set({ addGame: null });
  },
}));
