import type { AccentColor, CollectionId, StoreSource } from "@cove/shared";
import { create } from "zustand";
import { persist } from "zustand/middleware";
import { api } from "../api/index.ts";
import { isLanguage, setLanguage } from "../i18n/index.ts";

export type LibraryView = "grid" | "list";
/** The fixed groups of the library. */
export type BuiltinGroup = "all" | "installed" | "favorites" | StoreSource;
/** A fixed group or one of the user's collections (`collection:<id>`). */
export type LibraryGroup = BuiltinGroup | `collection:${string}`;

/** Library filter by how long a game takes (main story and extras). */
export type LengthFilter = "any" | "short" | "medium" | "long" | "veryLong";

export function collectionGroup(id: CollectionId): LibraryGroup {
  return `collection:${id}`;
}

interface PreferencesState {
  accent: AccentColor;
  sidebarCollapsed: boolean;
  libraryView: LibraryView;
  libraryGroup: LibraryGroup;
  libraryLength: LengthFilter;
  setAccent: (accent: AccentColor) => void;
  toggleSidebar: () => void;
  setLibraryView: (view: LibraryView) => void;
  setLibraryGroup: (group: LibraryGroup) => void;
  setLibraryLength: (length: LengthFilter) => void;
}

/** Client-side UI preferences, persisted in localStorage so the accent applies before first paint. */
export const usePreferences = create<PreferencesState>()(
  persist(
    (set) => ({
      accent: "blue",
      sidebarCollapsed: false,
      libraryView: "grid",
      libraryGroup: "all",
      libraryLength: "any",
      setAccent: (accent) => {
        set({ accent });
      },
      toggleSidebar: () => {
        set((s) => ({ sidebarCollapsed: !s.sidebarCollapsed }));
      },
      setLibraryView: (libraryView) => {
        set({ libraryView });
      },
      setLibraryGroup: (libraryGroup) => {
        set({ libraryGroup });
      },
      setLibraryLength: (libraryLength) => {
        set({ libraryLength });
      },
    }),
    { name: "cove.preferences" },
  ),
);

/** Applies the (only, dark) theme and the accent to <html>. Called before first render and on every change. */
export function applyAppearance(accent: AccentColor): void {
  const root = document.documentElement;
  root.dataset.theme = "dark";
  if (accent === "blue") delete root.dataset.accent;
  else root.dataset.accent = accent;
}

const ACCENTS: readonly AccentColor[] = ["blue", "teal", "sage", "plum", "clay"];

/**
 * Keeps accent and language in sync with the core's settings: the stored values win
 * at startup (localStorage only avoids a flash), every later change is saved.
 * Returns the unsubscribe function.
 */
export function syncAppearanceWithCore(): () => void {
  let ready = false;
  api.settings.getSettings().then(
    (settings) => {
      const accent = ACCENTS.find((a) => a === settings.appearance.accent);
      if (accent) usePreferences.setState({ accent });
      if (isLanguage(settings.general.language)) setLanguage(settings.general.language);
      ready = true;
    },
    () => undefined, // no core (browser preview): keep local values
  );
  return usePreferences.subscribe((state, prev) => {
    if (!ready || state.accent === prev.accent) return;
    void api.settings
      .getSettings()
      .then((settings) => api.settings.saveSettings({ ...settings, appearance: { theme: "dark", accent: state.accent } }))
      .catch(() => undefined);
  });
}
