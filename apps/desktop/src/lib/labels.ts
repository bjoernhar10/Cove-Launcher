import type {
  PluginPermission,
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

/** What each plugin permission allows, as shown in the approval dialog. */
export const PERMISSION_LABELS: Record<PluginPermission, { title: string; description: string }> = {
  "library.read": { title: "Bibliothek lesen", description: "Sieht deine Spiele mit Titel, Genres und Spielzeit, aber keine Dateipfade." },
  "library.add": { title: "Spiele hinzufügen", description: "Kann neue Spiele in deine Bibliothek eintragen." },
  "catalog.search": { title: "Katalog durchsuchen", description: "Kann im Spielekatalog nach Titeln suchen." },
  "achievements.define": { title: "Achievements festlegen", description: "Kann eigene Cove-Achievements mit festen Regeln anlegen." },
  "ui.addSidebarItem": { title: "Eintrag in der Seitenleiste", description: "Kann bis zu drei Einträge unter „Plugins“ anlegen." },
  "ui.notify": { title: "Hinweise anzeigen", description: "Kann kurze Hinweise unten rechts einblenden." },
  storage: { title: "Daten speichern", description: "Kann bis zu 1 MB eigene Daten speichern, nur für sich selbst." },
};
