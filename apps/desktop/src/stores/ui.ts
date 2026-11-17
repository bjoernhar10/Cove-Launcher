import { create } from "zustand";

export interface AddGameOptions {
  mode: "manual" | "scan" | "launchers";
  /** Pre-filled folder; the scan starts immediately. */
  folder?: string;
}

interface UiState {
  paletteOpen: boolean;
  addGame: AddGameOptions | null;
  /** "Was soll ich spielen?" */
  pickerOpen: boolean;
  setPickerOpen: (open: boolean) => void;
  setPaletteOpen: (open: boolean) => void;
  openAddGame: (options?: AddGameOptions) => void;
  closeAddGame: () => void;
}

export const useUi = create<UiState>()((set) => ({
  paletteOpen: false,
  addGame: null,
  pickerOpen: false,
  setPickerOpen: (pickerOpen) => {
    set({ pickerOpen });
  },
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
