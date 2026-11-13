import { useEffect, useMemo } from "react";
import { NavLink, Outlet, useLocation, useNavigate } from "react-router";
import { api } from "../api/index.ts";
import { CoveLogo } from "../components/ui/CoveLogo.tsx";
import { PadGlyph, PadHints } from "../components/ui/PadGlyph.tsx";
import { onPad } from "../hooks/useGamepadNavigation.ts";
import { useHotkey } from "../hooks/useHotkey.ts";
import { useMessages } from "../i18n/index.ts";
import { cx } from "../lib/cx.ts";

/** Everything is 1.5 times larger: the same type scale, readable from the sofa. */
export const TV_SCALE = 1.5;

/**
 * The TV mode: full screen, large, made for controllers (Steam Deck, TV).
 * LB/RB (L1/R1, L/R) switch sections, the hint bar names the buttons of the
 * connected controller.
 */
export function TvShell() {
  const t = useMessages();
  const navigate = useNavigate();
  const location = useLocation();
  const tabs = useMemo(
    () => [
      { to: "/tv", label: t.tv.home },
      { to: "/tv/library", label: t.tv.library },
      { to: "/tv/search", label: t.tv.search },
      { to: "/tv/downloads", label: t.tv.downloads },
      { to: "/tv/settings", label: t.tv.settings },
    ],
    [t],
  );
  const current = tabs.findIndex((tab, i) => (i === 0 ? location.pathname === "/tv" : location.pathname.startsWith(tab.to)));

  useEffect(() => {
    void api.gamepad.setFullscreen(true).catch(() => undefined);
    return () => {
      void api.gamepad.setFullscreen(false).catch(() => undefined);
    };
  }, []);

  useEffect(
    () =>
      onPad((action) => {
        if (action !== "prevTab" && action !== "nextTab") return;
        const step = action === "nextTab" ? 1 : -1;
        const next = tabs[(Math.max(0, current) + step + tabs.length) % tabs.length];
        if (next) void navigate(next.to);
      }),
    [current, navigate, tabs],
  );

  useHotkey("F11", () => {
    void navigate("/");
  });

  return (
    <div className="fixed inset-0 overflow-hidden bg-bg" data-tv>
      <div
        className="flex flex-col"
        style={{ zoom: TV_SCALE, width: `calc(100vw / ${String(TV_SCALE)})`, height: `calc(100vh / ${String(TV_SCALE)})` }}
      >
        <header className="flex shrink-0 items-center gap-8 px-12 pt-8 pb-4">
          <span className="flex items-center gap-2 text-lg font-semibold tracking-tight text-fg">
            <CoveLogo />
            Cove
          </span>
          <nav aria-label={t.tv.sections} className="flex items-center gap-2">
            <PadGlyph action="prevTab" />
            {tabs.map((tab, i) => (
              <NavLink
                key={tab.to}
                to={tab.to}
                end={i === 0}
                className={({ isActive }) =>
                  cx(
                    "rounded-md px-4 py-2 text-base transition-colors duration-150 ease-out",
                    isActive ? "bg-active font-medium text-fg" : "text-fg-muted hover:bg-hover hover:text-fg",
                  )
                }
              >
                {tab.label}
              </NavLink>
            ))}
            <PadGlyph action="nextTab" />
          </nav>
        </header>
        <main id="main" tabIndex={-1} className="min-h-0 flex-1 overflow-y-auto px-12 py-6 outline-none">
          <Outlet />
        </main>
        <footer className="shrink-0 border-t border-border px-12 py-4">
          <PadHints
            hints={[
              { action: "confirm", label: t.tv.select },
              { action: "back", label: t.tv.back },
              { action: "tertiary", label: t.tv.toSearch },
              { action: "menu", label: t.tv.settings },
            ]}
          />
        </footer>
      </div>
    </div>
  );
}
