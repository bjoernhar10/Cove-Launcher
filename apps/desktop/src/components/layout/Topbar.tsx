import { ArrowLeft, ArrowRight, PanelLeftClose, PanelLeftOpen, Search } from "lucide-react";
import { useHistoryNav } from "../../hooks/useHistoryNav.ts";
import { useHotkey } from "../../hooks/useHotkey.ts";
import { modKeyLabel } from "../../lib/env.ts";
import { usePreferences } from "../../stores/preferences.ts";
import { useUi } from "../../stores/ui.ts";
import { Button } from "../ui/Button.tsx";
import { Kbd } from "../ui/Feedback.tsx";
import { WindowControls } from "./WindowControls.tsx";

/** Top bar = custom title bar: drag region, history, global search, window buttons. */
export function Topbar() {
  const { canBack, canForward, back, forward } = useHistoryNav();
  const collapsed = usePreferences((s) => s.sidebarCollapsed);
  const toggleSidebar = usePreferences((s) => s.toggleSidebar);
  const openPalette = useUi((s) => s.setPaletteOpen);

  useHotkey("k", () => { openPalette(true); }, { mod: true });
  useHotkey("b", toggleSidebar, { mod: true });
  useHotkey("ArrowLeft", () => { if (canBack) back(); }, { alt: true });
  useHotkey("ArrowRight", () => { if (canForward) forward(); }, { alt: true });

  return (
    <header data-tauri-drag-region className="flex h-12 shrink-0 items-center gap-1 border-b border-border px-3">
      <Button
        variant="ghost"
        size="sm"
        iconOnly
        icon={collapsed ? PanelLeftOpen : PanelLeftClose}
        label={`${collapsed ? "Seitenleiste ausklappen" : "Seitenleiste einklappen"} (${modKeyLabel}+B)`}
        onClick={toggleSidebar}
      />
      <Button variant="ghost" size="sm" iconOnly icon={ArrowLeft} label="Zurück (Alt+←)" disabled={!canBack} onClick={back} />
      <Button variant="ghost" size="sm" iconOnly icon={ArrowRight} label="Vor (Alt+→)" disabled={!canForward} onClick={forward} />

      <button
        type="button"
        onClick={() => {
          openPalette(true);
        }}
        aria-keyshortcuts="Control+K"
        className="ml-3 flex h-8 w-full max-w-sm items-center gap-2 rounded-md border border-border bg-sidebar px-2.5 text-sm text-fg-subtle transition-colors duration-150 ease-out hover:border-border-strong"
      >
        <Search aria-hidden />
        <span className="flex-1 text-left">Spiele, Seiten, Aktionen suchen …</span>
        <span className="flex gap-1">
          <Kbd>{modKeyLabel}</Kbd>
          <Kbd>K</Kbd>
        </span>
      </button>

      <div data-tauri-drag-region className="h-full flex-1" />
      <WindowControls />
    </header>
  );
}
