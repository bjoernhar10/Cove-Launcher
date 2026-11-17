import type { LibraryItem, ShortcutPlace } from "@cove/shared";
import { Link2, Trash2 } from "lucide-react";
import { useState } from "react";
import { api } from "../api/index.ts";
import { Button } from "../components/ui/Button.tsx";
import { ErrorState, LoadingRegion, Skeleton } from "../components/ui/Feedback.tsx";
import { useQuery } from "../hooks/useQuery.ts";
import { useMessages } from "../i18n/index.ts";
import { detectPlatform } from "../lib/env.ts";
import { invalidateQueries } from "../stores/query.ts";
import { errorMessage, toast } from "../stores/toasts.ts";

/** "Verknüpfungen": start the game from the desktop or the app menu, through Cove. */
export function GameShortcuts({ item }: { item: LibraryItem }) {
  const t = useMessages();
  const id = item.game.id;
  const query = useQuery(`shortcuts/${id}`, () => api.library.shortcuts(id));
  const [busy, setBusy] = useState<ShortcutPlace | null>(null);
  const platform = detectPlatform();
  const label = (place: ShortcutPlace) => (place === "desktop" ? t.shortcuts.desktop : t.shortcuts.menu[platform]);

  async function run(place: ShortcutPlace, remove: boolean): Promise<void> {
    setBusy(place);
    try {
      if (remove) {
        await api.library.removeShortcut(id, place);
        toast(t.shortcuts.removed(label(place)));
      } else {
        await api.library.createShortcut(id, place);
        toast(t.shortcuts.created(label(place)));
      }
    } catch (error: unknown) {
      toast(errorMessage(error), "danger");
    } finally {
      setBusy(null);
      invalidateQueries();
    }
  }

  return (
    <section aria-labelledby={`shortcuts-${id}`} className="max-w-2xl border-t border-border pt-6">
      <h3 id={`shortcuts-${id}`} className="text-sm font-medium text-fg">
        {t.shortcuts.title}
      </h3>
      <p className="mt-1 text-sm text-fg-muted">{t.shortcuts.text}</p>
      {query.status === "loading" && (
        <LoadingRegion label={t.shortcuts.loading}>
          <Skeleton className="mt-3 h-20" />
        </LoadingRegion>
      )}
      {query.status === "error" && <ErrorState message={query.error} onRetry={query.retry} />}
      {query.status === "success" && (
        <ul className="mt-4 rounded-lg border border-border">
          {query.data.places.map((place) => {
            const existing = query.data.shortcuts.find((s) => s.place === place);
            return (
              <li key={place} className="flex items-center gap-4 border-b border-border py-2 pr-2 pl-4 last:border-b-0">
                <div className="min-w-0 flex-1">
                  <p className="text-sm text-fg">{label(place)}</p>
                  <p className="truncate text-xs text-fg-muted" title={existing?.path}>
                    {existing ? existing.path : t.shortcuts.none}
                  </p>
                </div>
                {existing ? (
                  <Button
                    variant="ghost"
                    size="sm"
                    icon={Trash2}
                    disabled={busy !== null}
                    aria-label={t.shortcuts.removeLabel(label(place))}
                    onClick={() => void run(place, true)}
                  >
                    {t.shortcuts.remove}
                  </Button>
                ) : (
                  <Button
                    size="sm"
                    icon={Link2}
                    disabled={busy !== null}
                    aria-label={t.shortcuts.createLabel(label(place))}
                    onClick={() => void run(place, false)}
                  >
                    {t.shortcuts.create}
                  </Button>
                )}
              </li>
            );
          })}
        </ul>
      )}
    </section>
  );
}
