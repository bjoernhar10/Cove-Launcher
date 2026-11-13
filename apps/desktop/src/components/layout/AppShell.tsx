import { messages } from "../../i18n/index.ts";
import { useEffect, useRef } from "react";
import { Outlet, useLocation, useNavigate } from "react-router";
import { useGamepadNavigation } from "../../hooks/useGamepadNavigation.ts";
import { useHotkey } from "../../hooks/useHotkey.ts";
import { useGamepads } from "../../stores/gamepad.ts";
import { AddGameDialog } from "../../features/AddGameDialog.tsx";
import { api } from "../../api/index.ts";
import { useDownloads } from "../../stores/downloads.ts";
import { useMoves } from "../../stores/moves.ts";
import { useProton } from "../../stores/proton.ts";
import { invalidateQueries } from "../../stores/query.ts";
import { toast } from "../../stores/toasts.ts";
import { formatMoney } from "../../lib/format.ts";
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
  const connectMoves = useMoves((s) => s.connect);
  const connectProton = useProton((s) => s.connect);
  const connectGamepads = useGamepads((s) => s.connect);
  const location = useLocation();
  const navigate = useNavigate();
  const tv = location.pathname.startsWith("/tv");
  useGamepadNavigation();
  useEffect(() => connectGamepads(), [connectGamepads]);

  // The TV mode at the start (once): when asked for, and always on the Steam Deck's gaming mode.
  const startChecked = useRef(false);
  useEffect(() => {
    if (startChecked.current) return;
    startChecked.current = true;
    Promise.all([api.settings.getSettings(), api.gamepad.tvHint()]).then(
      ([settings, hint]) => {
        if ((settings.general.startInTvMode || hint) && !window.location.hash.startsWith("#/tv")) void navigate("/tv");
      },
      () => undefined,
    );
  }, [navigate]);

  useHotkey("F11", () => {
    if (!tv) void navigate("/tv");
  });

  useEffect(() => connectDownloads(), [connectDownloads]);
  useEffect(() => connectRunning(), [connectRunning]);
  useEffect(() => connectMoves(), [connectMoves]);
  useEffect(() => connectProton(), [connectProton]);
  useEffect(() => syncAppearanceWithCore(), []);
  useEffect(
    () =>
      api.library.subscribeMatchChanges((event) => {
        invalidateQueries();
        if (event.status !== "matched" && event.status !== "pending") return;
        void api.library.getLibraryItem(event.gameId).then((item) => {
          if (!item) return;
          toast(
            event.status === "matched" ? messages().shell.matched(item.game.title) : messages().shell.ambiguous(item.game.title),
          );
        });
      }),
    [],
  );

  useEffect(
    () =>
      api.accounts.subscribeAchievementUnlocked((achievement) => {
        invalidateQueries();
        toast(achievement.title, "neutral", { title: messages().achievements.unlocked, kind: "achievement", durationMs: 5000 });
      }),
    [],
  );

  useEffect(
    () =>
      api.wishlist.subscribePriceAlert((alert) => {
        invalidateQueries();
        const m = messages().wishlist;
        const price = formatMoney(alert.deal.price.amount, alert.deal.price.currency);
        const text = alert.reason === "target" ? m.alertTarget(alert.title, price, alert.deal.shop) : m.alertSale(alert.title, alert.deal.cut, alert.deal.shop);
        toast(text, "neutral", { title: m.title, action: { label: m.open, to: "/wishlist" }, durationMs: 8000 });
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
          toast(messages().shell.updateAvailable(check.available.version), "neutral", {
            title: messages().shell.updateTitle,
            action: { label: messages().shell.updateView, to: "/settings/about" },
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

  if (tv) {
    return (
      <>
        <Outlet />
        <Toaster />
      </>
    );
  }

  return (
    <div className="flex h-full">
      <a
        href="#main"
        className="sr-only z-50 rounded-md bg-raised px-3 py-2 text-sm focus:not-sr-only focus:fixed focus:top-2 focus:left-2"
      >
        {messages().sidebar.skipToContent}
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
