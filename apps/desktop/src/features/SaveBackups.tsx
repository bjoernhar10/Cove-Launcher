import type { BackupInfo, LibraryItem } from "@cove/shared";
import { ArchiveRestore, Save } from "lucide-react";
import { useState } from "react";
import { api } from "../api/index.ts";
import { Button } from "../components/ui/Button.tsx";
import { Dialog } from "../components/ui/Dialog.tsx";
import { ErrorState, LoadingRegion, Skeleton } from "../components/ui/Feedback.tsx";
import { useQuery } from "../hooks/useQuery.ts";
import { useMessages } from "../i18n/index.ts";
import { formatBytes, formatDate, formatRelative } from "../lib/format.ts";
import { invalidateQueries } from "../stores/query.ts";
import { errorMessage, toast } from "../stores/toasts.ts";

/** Save backups of a game: back up now, list, restore. */
export function SaveBackups({ item }: { item: LibraryItem }) {
  const t = useMessages();
  const id = item.game.id;
  const query = useQuery(`backups/${id}`, () => api.library.backups(id));
  const [busy, setBusy] = useState(false);
  const [restoring, setRestoring] = useState<BackupInfo | null>(null);

  async function backUp(): Promise<void> {
    setBusy(true);
    try {
      const created = await api.library.backUpNow(id);
      toast(created ? t.backups.done(created.files) : t.backups.unchanged);
    } catch (error: unknown) {
      toast(errorMessage(error), "danger");
    } finally {
      setBusy(false);
      invalidateQueries();
    }
  }

  async function restore(backup: BackupInfo): Promise<void> {
    setBusy(true);
    try {
      const count = await api.library.restoreBackup(id, backup.id);
      toast(t.backups.restored(count));
    } catch (error: unknown) {
      toast(errorMessage(error), "danger");
    } finally {
      setBusy(false);
      setRestoring(null);
      invalidateQueries();
    }
  }

  return (
    <section aria-labelledby={`backups-${id}`} className="max-w-2xl border-t border-border pt-6">
      <h3 id={`backups-${id}`} className="text-sm font-medium text-fg">
        {t.backups.gameTitle}
      </h3>
      {query.status === "loading" && (
        <LoadingRegion label={t.backups.loading}>
          <Skeleton className="mt-3 h-16" />
        </LoadingRegion>
      )}
      {query.status === "error" && <ErrorState message={query.error} onRetry={query.retry} />}
      {query.status === "success" && (
        <>
          <p className="num mt-1 text-sm text-fg-muted">
            {!query.data.known
              ? t.backups.unknown
              : query.data.files === 0
                ? t.backups.noFiles
                : t.backups.found(query.data.files, formatBytes(query.data.bytes))}
          </p>
          {query.data.known && (
            <Button className="mt-4" icon={Save} disabled={busy || query.data.files === 0} onClick={() => void backUp()}>
              {t.backups.now}
            </Button>
          )}
          {query.data.backups.length > 0 && (
            <ul className="mt-4 rounded-lg border border-border" aria-label={t.backups.list}>
              {query.data.backups.map((backup) => (
                <li key={backup.id} className="flex items-center gap-4 border-b border-border py-2 pr-2 pl-4 last:border-b-0">
                  <div className="min-w-0 flex-1">
                    <p className="text-sm text-fg">{formatRelative(backup.createdAt)}</p>
                    <p className="num text-xs text-fg-muted">
                      {formatDate(backup.createdAt)} · {t.backups.files(backup.files)} · {formatBytes(backup.bytes)}
                    </p>
                  </div>
                  <Button
                    variant="ghost"
                    size="sm"
                    icon={ArchiveRestore}
                    aria-label={t.backups.restoreTitle(formatDate(backup.createdAt))}
                    disabled={busy}
                    onClick={() => {
                      setRestoring(backup);
                    }}
                  >
                    {t.backups.restore}
                  </Button>
                </li>
              ))}
            </ul>
          )}
        </>
      )}
      <Dialog
        open={restoring !== null}
        onClose={() => {
          setRestoring(null);
        }}
        title={t.backups.restoreConfirm(item.game.title)}
        description={t.backups.restoreText}
        footer={
          <>
            <Button
              onClick={() => {
                setRestoring(null);
              }}
            >
              {t.common.cancel}
            </Button>
            <Button variant="primary" disabled={busy} onClick={() => restoring && void restore(restoring)}>
              {t.backups.restore}
            </Button>
          </>
        }
      >
        {null}
      </Dialog>
    </section>
  );
}
