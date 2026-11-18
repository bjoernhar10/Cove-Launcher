import type { LibraryItem, SaveSyncKeep } from "@cove/shared";
import { RefreshCw } from "lucide-react";
import { useEffect, useState } from "react";
import { api } from "../api/index.ts";
import { Button } from "../components/ui/Button.tsx";
import { ErrorState, LoadingRegion, Skeleton } from "../components/ui/Feedback.tsx";
import { useQuery } from "../hooks/useQuery.ts";
import { useMessages } from "../i18n/index.ts";
import { formatRelative } from "../lib/format.ts";
import { invalidateQueries } from "../stores/query.ts";
import { errorMessage, toast } from "../stores/toasts.ts";

/** A game's cloud sync: state, "sync now" and the choice after a conflict. */
export function SaveSync({ item }: { item: LibraryItem }) {
  const t = useMessages();
  const id = item.game.id;
  const query = useQuery(`save-sync/${id}`, () => api.saveSync.game(id));
  const [busy, setBusy] = useState(false);

  // A sync in the background (before a start, after a session).
  useEffect(
    () =>
      api.saveSync.subscribeChanged((event) => {
        if (event.gameId === id) invalidateQueries();
      }),
    [id],
  );

  async function run(keep: SaveSyncKeep | null): Promise<void> {
    setBusy(true);
    try {
      if (keep) {
        await api.saveSync.resolve(id, keep);
        toast(t.saveSync.kept);
      } else {
        await api.saveSync.syncNow(id);
        toast(t.saveSync.done);
      }
    } catch (error: unknown) {
      toast(errorMessage(error), "danger");
    } finally {
      setBusy(false);
      invalidateQueries();
    }
  }

  return (
    <section aria-labelledby={`save-sync-${id}`} className="max-w-2xl border-t border-border pt-6">
      <h3 id={`save-sync-${id}`} className="text-sm font-medium text-fg">
        {t.saveSync.gameTitle}
      </h3>
      {query.status === "loading" && (
        <LoadingRegion label={t.saveSync.loading}>
          <Skeleton className="mt-3 h-10" />
        </LoadingRegion>
      )}
      {query.status === "error" && <ErrorState message={query.error} onRetry={query.retry} />}
      {query.status === "success" &&
        (() => {
          const data = query.data;
          switch (data.state) {
            case "off":
              return <p className="mt-1 text-sm text-fg-muted">{t.saveSync.off}</p>;
            case "unsupported":
              return <p className="mt-1 text-sm text-fg-muted">{t.saveSync.unsupported}</p>;
            case "storeCloud":
              return <p className="mt-1 text-sm text-fg-muted">{t.saveSync.storeCloud}</p>;
            case "conflict": {
              const conflict = data.conflict;
              return (
                <div className="mt-3 rounded-lg border border-border-strong px-4 py-3" role="group" aria-label={t.saveSync.conflictTitle}>
                  <p className="text-sm font-medium text-fg">{t.saveSync.conflictTitle}</p>
                  <p className="mt-1 text-sm text-fg-muted">
                    {conflict?.device && conflict.savedAt ? t.saveSync.conflictText(conflict.device, formatRelative(conflict.savedAt)) : t.saveSync.conflictUnknown}
                  </p>
                  <div className="mt-3 flex flex-wrap gap-2">
                    <Button size="sm" disabled={busy} onClick={() => void run("cloud")}>
                      {t.saveSync.keepCloud}
                    </Button>
                    <Button size="sm" disabled={busy} onClick={() => void run("local")}>
                      {t.saveSync.keepLocal}
                    </Button>
                  </div>
                </div>
              );
            }
            case "pending":
            case "synced":
              return (
                <>
                  <p className="mt-1 text-sm text-fg-muted">{data.syncedAt ? t.saveSync.synced(formatRelative(data.syncedAt)) : t.saveSync.pending}</p>
                  <Button className="mt-4" icon={RefreshCw} disabled={busy} onClick={() => void run(null)}>
                    {t.saveSync.now}
                  </Button>
                </>
              );
          }
        })()}
    </section>
  );
}
