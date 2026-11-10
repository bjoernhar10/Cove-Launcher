import type {
  PluginPermission,
  AccentColor,
  CatalogSort,
  CompatibilityTool,
  Genre,
  LaunchVia,
  Platform,
  StoreSource,
} from "@cove/shared";
import { localized } from "../i18n/index.ts";

// Label tables answer in the current UI language (see i18n/de.ts, i18n/en.ts).

export const GENRE_LABELS: Record<Genre, string> = localized((m) => m.labels.genre);

export const PLATFORM_LABELS: Record<Platform, string> = {
  windows: "Windows",
  linux: "Linux",
  macos: "macOS",
};

export const SOURCE_LABELS: Record<StoreSource, string> = localized((m) => m.labels.source);

export const SORT_LABELS: Record<CatalogSort, string> = localized((m) => m.labels.sort);


export const ACCENT_LABELS: Record<AccentColor, string> = localized((m) => m.labels.accent);

/** How a game starts: "Direkt", "Über Steam" … */
export const LAUNCH_VIA_LABELS: Record<LaunchVia, string> = localized((m) => m.game.launchVia);

export const COMPAT_LABELS: Record<CompatibilityTool, string> = localized((m) => m.labels.compat);

/** What each plugin permission allows, as shown in the approval dialog. */
export const PERMISSION_LABELS: Record<PluginPermission, { title: string; description: string }> = localized(
  (m) => m.labels.permission,
);
