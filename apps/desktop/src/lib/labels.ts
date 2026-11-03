import type {
  AccentColor,
  CatalogSort,
  CompatibilityTool,
  Genre,
  Platform,
  StoreSource,
  ThemePreference,
} from "@cove/shared";

export const GENRE_LABELS: Record<Genre, string> = {
  action: "Action",
  adventure: "Adventure",
  rpg: "Rollenspiel",
  strategy: "Strategie",
  simulation: "Simulation",
  puzzle: "Puzzle",
  platformer: "Platformer",
  shooter: "Shooter",
  racing: "Rennspiel",
  sports: "Sport",
  horror: "Horror",
  indie: "Indie",
};

export const PLATFORM_LABELS: Record<Platform, string> = {
  windows: "Windows",
  linux: "Linux",
  macos: "macOS",
};

export const SOURCE_LABELS: Record<StoreSource, string> = {
  steam: "Steam",
  gog: "GOG",
  epic: "Epic",
  manual: "Manuell",
};

export const SORT_LABELS: Record<CatalogSort, string> = {
  recommended: "Empfohlen",
  popular: "Beliebt",
  newest: "Neu",
  title: "A–Z",
};

export const THEME_LABELS: Record<ThemePreference, string> = {
  light: "Hell",
  dark: "Dunkel",
  system: "System",
};

export const ACCENT_LABELS: Record<AccentColor, string> = {
  blue: "Blau",
  teal: "Petrol",
  sage: "Salbei",
  plum: "Pflaume",
  clay: "Ton",
};

export const COMPAT_LABELS: Record<CompatibilityTool, string> = {
  native: "Nativ",
  proton: "Proton (stabil)",
  "proton-experimental": "Proton Experimental",
  wine: "Wine",
};
