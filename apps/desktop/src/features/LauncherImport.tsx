import type { ExternalGame } from "@cove/shared";
import { Gamepad2 } from "lucide-react";
import { useState } from "react";
import { api } from "../api/index.ts";
import { Button } from "../components/ui/Button.tsx";
import { Badge, EmptyState, ErrorState, LoadingRegion, Skeleton } from "../components/ui/Feedback.tsx";
import { useQuery } from "../hooks/useQuery.ts";
import { useMessages } from "../i18n/index.ts";
import { formatPlaytime } from "../lib/format.ts";
import { invalidateQueries } from "../stores/query.ts";
import { errorMessage, toast } from "../stores/toasts.ts";

const LAUNCHER_NAMES = { heroic: "Heroic", lutris: "Lutris" } as const;

/** "Aus Heroic oder Lutris": their own games with Wine prefix, Wine build and playtime. */
export function LauncherImport({ onDone }: { onDone: () => void }) {
  const t = useMessages();
  const query = useQuery("launchers/scan", api.library.launcherGames);
  const [picked, setPicked] = useState<Set<string> | null>(null);
  const [busy, setBusy] = useState(false);

  if (query.status === "loading") {
    return (
      <LoadingRegion label={t.launchers.loading} className="flex flex-col gap-2">
        {[0, 1, 2].map((i) => (
          <Skeleton key={i} className="h-14" />
        ))}
      </LoadingRegion>
    );
  }
  if (query.status === "error") return <ErrorState message={query.error} onRetry={query.retry} />;
  const games = query.data;
  if (games.length === 0) {
    return (
      <div className="rounded-lg border border-border">
        <EmptyState compact icon={Gamepad2} title={t.launchers.emptyTitle} description={t.launchers.emptyText} />
      </div>
    );
  }
  // Everything that isn't in the library yet is ticked at first.
  const selected = picked ?? new Set(games.filter((g) => !g.inLibrary).map((g) => g.id));

  function toggle(game: ExternalGame, on: boolean): void {
    const next = new Set(selected);
    if (on) next.add(game.id);
    else next.delete(game.id);
    setPicked(next);
  }

  async function add(): Promise<void> {
    setBusy(true);
    try {
      const added = await api.library.importLauncherGames([...selected]);
      toast(t.addGame.added(added.length));
      invalidateQueries();
      onDone();
    } catch (error: unknown) {
      toast(errorMessage(error), "danger");
    } finally {
      setBusy(false);
    }
  }

  return (
    <div className="flex flex-col gap-5">
      <p className="text-sm text-fg-muted">{t.launchers.text}</p>
      <fieldset className="max-h-80 overflow-y-auto rounded-lg border border-border">
        <legend className="sr-only">{t.addGame.found}</legend>
        {games.map((game) => (
          <label
            key={game.id}
            className={
              "flex items-center gap-3 border-b border-border px-4 py-3 last:border-b-0 " +
              (game.inLibrary ? "opacity-60" : "cursor-pointer hover:bg-hover")
            }
          >
            <input
              type="checkbox"
              className="size-4"
              disabled={game.inLibrary}
              checked={selected.has(game.id)}
              onChange={(e) => {
                toggle(game, e.target.checked);
              }}
            />
            <span className="min-w-0 flex-1">
              <span className="flex items-center gap-2">
                <span className="truncate text-sm font-medium text-fg">{game.title}</span>
                <Badge>{LAUNCHER_NAMES[game.launcher]}</Badge>
                {game.inLibrary && <span className="text-xs text-fg-subtle">{t.addGame.alreadyInLibrary}</span>}
              </span>
              <span className="block truncate text-xs text-fg-subtle" title={game.winePrefix ?? game.executablePath}>
                {game.windows ? (game.winePrefix ? t.launchers.withPrefix(game.proton ? "Proton" : "Wine") : t.launchers.windows) : t.launchers.native}
                {game.playtimeMinutes > 0 && ` · ${formatPlaytime(game.playtimeMinutes)}`}
              </span>
            </span>
          </label>
        ))}
      </fieldset>
      <div className="flex justify-end gap-2">
        <Button onClick={onDone}>{t.common.cancel}</Button>
        <Button variant="primary" disabled={selected.size === 0 || busy} onClick={() => void add()}>
          {t.addGame.addCount(selected.size)}
        </Button>
      </div>
    </div>
  );
}
