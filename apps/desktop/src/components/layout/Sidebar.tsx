import { Download } from "lucide-react";
import { NavLink } from "react-router";
import { api } from "../../api/index.ts";
import { useQuery } from "../../hooks/useQuery.ts";
import { cx } from "../../lib/cx.ts";
import { formatPercent } from "../../lib/format.ts";
import { selectActive, selectQueued, selectRunning, useDownloads } from "../../stores/downloads.ts";
import { usePreferences } from "../../stores/preferences.ts";
import { Avatar, ProgressBar, Skeleton } from "../ui/Feedback.tsx";
import { MAIN_NAV, SETTINGS_NAV, type NavItem } from "./nav.ts";
import { PluginSidebarItems } from "./PluginSidebarItems.tsx";

function SidebarLink({ item, collapsed, badge }: { item: NavItem; collapsed: boolean; badge?: number }) {
  const Icon = item.icon;
  return (
    <NavLink
      to={item.to}
      end={item.to === "/"}
      aria-label={collapsed ? item.label : undefined}
      className={({ isActive }) =>
        cx(
          "group relative flex h-9 items-center gap-3 rounded-md text-sm transition-colors duration-150 ease-out",
          collapsed ? "mx-auto w-10 justify-center" : "px-2.5",
          isActive ? "bg-active font-medium text-fg" : "text-fg-muted hover:bg-hover hover:text-fg",
        )
      }
    >
      {({ isActive }) => (
        <>
          <Icon aria-hidden className={cx("shrink-0", isActive ? "text-accent-text" : "text-fg-subtle group-hover:text-fg-muted")} />
          {!collapsed && <span className="min-w-0 flex-1 truncate">{item.label}</span>}
          {!collapsed && badge !== undefined && badge > 0 && (
            <span className="num text-xs text-fg-subtle" aria-label={`${String(badge)} aktiv`}>
              {badge}
            </span>
          )}
          {collapsed && (
            <span
              role="tooltip"
              className="pointer-events-none absolute left-full z-40 ml-3 rounded-md border border-border bg-raised px-2 py-1 text-xs whitespace-nowrap text-fg opacity-0 transition-opacity duration-150 ease-out group-hover:opacity-100 group-focus-visible:opacity-100"
            >
              {item.label}
            </span>
          )}
        </>
      )}
    </NavLink>
  );
}

function DownloadIndicator({ collapsed }: { collapsed: boolean }) {
  const items = useDownloads((s) => s.items);
  const running = selectRunning(items);
  if (!running) return null;
  const { downloadedBytes, totalBytes, status } = running.download;
  const pct =
    status === "verifying" ? "Prüfen" : status === "extracting" ? "Entpacken" : totalBytes > 0 ? formatPercent(downloadedBytes / totalBytes) : "…";
  return (
    <NavLink
      to="/downloads"
      aria-label={`Download läuft: ${running.game.title}, ${pct}`}
      title={collapsed ? `${running.game.title} · ${pct}` : undefined}
      className={cx("mb-2 block rounded-md transition-colors duration-150 ease-out hover:bg-hover", collapsed ? "mx-auto w-10 px-1.5 py-2" : "mx-0 px-2.5 py-2")}
    >
      {collapsed ? (
        <Download aria-hidden className="mx-auto text-accent-text" />
      ) : (
        <div className="flex items-center justify-between gap-2 text-xs">
          <span className="min-w-0 truncate text-fg-muted">{running.game.title}</span>
          <span className="num shrink-0 text-fg-subtle">{pct}</span>
        </div>
      )}
      <ProgressBar value={downloadedBytes} max={totalBytes} label="Download-Fortschritt" className="mt-2" />
    </NavLink>
  );
}

export function Sidebar() {
  const collapsed = usePreferences((s) => s.sidebarCollapsed);
  const items = useDownloads((s) => s.items);
  const pending = selectActive(items).length + selectQueued(items).length;
  const profile = useQuery("profile", api.system.getProfile);

  return (
    <aside
      aria-label="Hauptnavigation"
      className={cx(
        "flex shrink-0 flex-col border-r border-border bg-sidebar transition-[width] duration-200 ease-out",
        collapsed ? "w-16" : "w-60",
      )}
    >
      <div data-tauri-drag-region className={cx("flex h-12 shrink-0 items-center", collapsed ? "justify-center" : "px-5")}>
        <span data-tauri-drag-region className="text-lg font-semibold tracking-tight text-fg select-none">
          {collapsed ? "C" : "Cove"}
        </span>
      </div>

      <nav aria-label="Bereiche" className={cx("flex flex-col gap-0.5 pt-3", collapsed ? "px-0" : "px-3")}>
        {MAIN_NAV.map((item) => (
          <SidebarLink key={item.to} item={item} collapsed={collapsed} {...(item.to === "/downloads" ? { badge: pending } : {})} />
        ))}
      </nav>

      <PluginSidebarItems collapsed={collapsed} />

      <div className="flex-1" data-tauri-drag-region />

      <div className={cx("pb-3", collapsed ? "px-0" : "px-3")}>
        <DownloadIndicator collapsed={collapsed} />
        <SidebarLink item={SETTINGS_NAV} collapsed={collapsed} />
      </div>

      <div className={cx("flex h-14 shrink-0 items-center gap-2.5 border-t border-border", collapsed ? "justify-center" : "px-5")}>
        {profile.status === "success" ? (
          <>
            <Avatar name={profile.data.displayName} />
            {!collapsed && (
              <div className="min-w-0">
                <p className="truncate text-sm font-medium text-fg">{profile.data.displayName}</p>
                <p className="truncate text-xs text-fg-subtle">Lokales Profil</p>
              </div>
            )}
          </>
        ) : (
          <>
            <Skeleton className="size-7 rounded-full" />
            {!collapsed && <Skeleton className="h-4 w-24" />}
          </>
        )}
      </div>
    </aside>
  );
}
