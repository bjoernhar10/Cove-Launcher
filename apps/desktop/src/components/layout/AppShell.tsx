import { useEffect } from "react";
import { Outlet, useLocation } from "react-router";
import { AddGameDialog } from "../../features/AddGameDialog.tsx";
import { api } from "../../api/index.ts";
import { useDownloads } from "../../stores/downloads.ts";
import { invalidateQueries } from "../../stores/query.ts";
import { toast } from "../../stores/toasts.ts";
import { syncAppearanceWithCore } from "../../stores/preferences.ts";
import { useRunningGames } from "../../stores/running.ts";
import { usePlugins } from "../../stores/plugins.ts";
import { Toaster } from "../ui/Toaster.tsx";
import { CommandPalette } from "./CommandPalette.tsx";
import { Sidebar } from "./Sidebar.tsx";
import { Topbar } from "./Topbar.tsx";

export function AppShell() {
  const connectDownloads = useDownloads((s) => s.connect);
  const connectRunning = useRunningGames((s) => s.connect);
  const location = useLocation();

  useEffect(() => connectDownloads(), [connectDownloads]);
  useEffect(() => connectRunning(), [connectRunning]);
  useEffect(() => syncAppearanceWithCore(), []);
  useEffect(
    () =>
      api.library.subscribeMatchChanges((event) => {
        invalidateQueries();
        if (event.status !== "matched" && event.status !== "pending") return;
        void api.library.getLibraryItem(event.gameId).then((item) => {
          if (!item) return;
          toast(
            event.status === "matched"
              ? `„${item.game.title}“ mit Katalogdaten ergänzt`
              : `Für „${item.game.title}“ gibt es mehrere Treffer im Katalog. Prüfe die Zuordnung auf der Spielseite.`,
          );
        });
      }),
    [],
  );

  useEffect(
    () =>
      api.accounts.subscribeAchievementUnlocked((achievement) => {
        invalidateQueries();
        toast(achievement.title, "neutral", { title: "Achievement freigeschaltet", kind: "achievement", durationMs: 5000 });
      }),
    [],
  );

  // Release builds look for an update once, shortly after start. Silent when
  // offline or up to date; Settings → Über Cove has the manual check.
  useEffect(() => {
    if (!import.meta.env.PROD) return;
    const timer = setTimeout(() => {
      api.system.checkForUpdates().then(
        (check) => {
          if (!check.available) return;
          toast(`Cove ${check.available.version} ist verfügbar.`, "neutral", {
            title: "Update",
            action: { label: "Ansehen", to: "/settings/about" },
            durationMs: 8000,
          });
        },
        () => undefined,
      );
    }, 15_000);
    return () => {
      clearTimeout(timer);
    };
  }, []);

  useEffect(() => {
    void usePlugins.getState().start();
    return () => {
      usePlugins.getState().stop();
    };
  }, []);

  // Reset scroll position on navigation.
  useEffect(() => {
    document.getElementById("main")?.scrollTo({ top: 0 });
  }, [location.pathname]);

  return (
    <div className="flex h-full">
      <a
        href="#main"
        className="sr-only z-50 rounded-md bg-raised px-3 py-2 text-sm focus:not-sr-only focus:fixed focus:top-2 focus:left-2"
      >
        Zum Inhalt springen
      </a>
      <Sidebar />
      <div className="flex min-w-0 flex-1 flex-col">
        <Topbar />
        <main id="main" tabIndex={-1} className="min-h-0 flex-1 overflow-y-auto outline-none">
          <Outlet />
        </main>
      </div>
      <CommandPalette />
      <AddGameDialog />
      <Toaster />
    </div>
  );
}
