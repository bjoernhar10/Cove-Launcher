import type { PluginIcon } from "@cove/shared";
import { Clock, Dices, List, Puzzle, Sparkles, Star, Trophy, Upload, type LucideIcon } from "lucide-react";
import { useRef } from "react";
import { cx } from "../../lib/cx.ts";
import type { SidebarEntry } from "../../plugins/host.ts";
import { readPickedFile, usePlugins } from "../../stores/plugins.ts";
import { errorMessage, toast } from "../../stores/toasts.ts";

const ICONS: Record<PluginIcon, LucideIcon> = {
  puzzle: Puzzle,
  dice: Dices,
  trophy: Trophy,
  upload: Upload,
  sparkles: Sparkles,
  list: List,
  star: Star,
  clock: Clock,
};

function PluginItem({ entry, collapsed }: { entry: SidebarEntry; collapsed: boolean }) {
  const click = usePlugins((s) => s.click);
  const fileInput = useRef<HTMLInputElement>(null);
  const Icon = ICONS[entry.icon];

  function onClick(): void {
    // The file dialog must open inside the click (user gesture), not after a round trip to the worker.
    if (entry.pickFile) fileInput.current?.click();
    else click(entry, null);
  }

  return (
    <>
      <button
        type="button"
        onClick={onClick}
        aria-label={collapsed ? entry.label : undefined}
        title={collapsed ? `${entry.label} (${entry.pluginName})` : `Plugin: ${entry.pluginName}`}
        className={cx(
          "group flex h-9 items-center gap-3 rounded-md text-sm text-fg-muted transition-colors duration-150 ease-out hover:bg-hover hover:text-fg",
          collapsed ? "mx-auto w-10 justify-center" : "px-3 text-left",
        )}
      >
        <Icon aria-hidden className="shrink-0 text-fg-subtle group-hover:text-fg-muted" />
        {!collapsed && <span className="min-w-0 flex-1 truncate">{entry.label}</span>}
      </button>
      {entry.pickFile && (
        <input
          ref={fileInput}
          type="file"
          accept={entry.pickFile.accept}
          className="hidden"
          tabIndex={-1}
          aria-hidden
          onChange={(event) => {
            const file = event.target.files?.[0];
            event.target.value = "";
            if (!file || !entry.pickFile) return;
            readPickedFile(file, entry.pickFile.maxBytes).then(
              (picked) => {
                click(entry, picked);
              },
              (error: unknown) => {
                toast(errorMessage(error), "danger");
              },
            );
          }}
        />
      )}
    </>
  );
}

/** Entries added by plugins (`ui.addSidebarItem`). */
export function PluginSidebarItems({ collapsed }: { collapsed: boolean }) {
  const items = usePlugins((s) => s.items);
  if (items.length === 0) return null;
  return (
    <nav aria-label="Plugins" className={cx("mt-6 flex flex-col gap-1", collapsed ? "px-0" : "px-3")}>
      {!collapsed && <p className="px-3 pb-1 text-xs font-medium text-fg-subtle">Plugins</p>}
      {items.map((entry) => (
        <PluginItem key={`${entry.pluginId}:${entry.id}`} entry={entry} collapsed={collapsed} />
      ))}
    </nav>
  );
}
