import { Compass, Download, House, Library, Settings, type LucideIcon } from "lucide-react";

export interface NavItem {
  to: string;
  label: string;
  icon: LucideIcon;
}

export const MAIN_NAV: NavItem[] = [
  { to: "/", label: "Home", icon: House },
  { to: "/catalogue", label: "Catalogue", icon: Compass },
  { to: "/library", label: "My Library", icon: Library },
  { to: "/downloads", label: "Downloads", icon: Download },
];

export const SETTINGS_NAV: NavItem = { to: "/settings", label: "Settings", icon: Settings };
