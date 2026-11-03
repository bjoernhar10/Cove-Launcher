import type { AccentColor, StoreSource, ThemePreference } from "@cove/shared";
import { create } from "zustand";
import { persist } from "zustand/middleware";
import { api } from "../api/index.ts";

export type LibraryView = "grid" | "list";
export type LibraryGroup = "all" | "installed" | "favorites" | StoreSource;

interface PreferencesState {
  theme: ThemePreference;
  accent: AccentColor;
  sidebarCollapsed: boolean;
  libraryView: LibraryView;
  libraryGroup: LibraryGroup;
  setTheme: (theme: ThemePreference) => void;
  setAccent: (accent: AccentColor) => void;
  toggleSidebar: () => void;
  setLibraryView: (view: LibraryView) => void;
  setLibraryGroup: (group: LibraryGroup) => void;
}

/** Client-side UI preferences, persisted in localStorage so the theme applies before first paint. */
export const usePreferences = create<PreferencesState>()(
  persist(
    (set) => ({
      theme: "system",
      accent: "blue",
      sidebarCollapsed: false,
      libraryView: "grid",
      libraryGroup: "all",
      setTheme: (theme) => {
        set({ theme });
      },
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
    }),
    { name: "cove.preferences" },
  ),
);

export function resolveTheme(theme: ThemePreference): "light" | "dark" {
  if (theme !== "system") return theme;
  return globalThis.matchMedia("(prefers-color-scheme: dark)").matches ? "dark" : "light";
}

/** Applies theme + accent to <html>. Called before first render and on every change. */
export function applyAppearance(theme: ThemePreference, accent: AccentColor): void {
  const root = document.documentElement;
  root.dataset.theme = resolveTheme(theme);
  if (accent === "blue") delete root.dataset.accent;
  else root.dataset.accent = accent;
}

const THEMES: readonly ThemePreference[] = ["system", "light", "dark"];
const ACCENTS: readonly AccentColor[] = ["blue", "teal", "sage", "plum", "clay"];

/**
 * Keeps theme + accent in sync with the core's settings: the stored values win
 * at startup (localStorage only avoids a flash), every later change is saved.
 * Returns the unsubscribe function.
 */
export function syncAppearanceWithCore(): () => void {
  let ready = false;
  api.settings.getSettings().then(
    (settings) => {
      const theme = THEMES.find((t) => t === settings.appearance.theme);
      const accent = ACCENTS.find((a) => a === settings.appearance.accent);
      usePreferences.setState({ ...(theme ? { theme } : {}), ...(accent ? { accent } : {}) });
      ready = true;
    },
    () => undefined, // no core (browser preview): keep local values
  );
  return usePreferences.subscribe((state, prev) => {
    if (!ready || (state.theme === prev.theme && state.accent === prev.accent)) return;
    void api.settings
      .getSettings()
      .then((settings) => api.settings.saveSettings({ ...settings, appearance: { theme: state.theme, accent: state.accent } }))
      .catch(() => undefined);
  });
}
