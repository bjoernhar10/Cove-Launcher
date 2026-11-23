import { RefreshCw } from "lucide-react";
import { useState } from "react";
import { api } from "../api/index.ts";
import { Button } from "../components/ui/Button.tsx";
import { ErrorState, LoadingRegion, Skeleton } from "../components/ui/Feedback.tsx";
import { SettingRow, Switch } from "../components/ui/Field.tsx";
import { useQuery } from "../hooks/useQuery.ts";
import { useMessages } from "../i18n/index.ts";
import { formatRelative } from "../lib/format.ts";
import { invalidateQueries } from "../stores/query.ts";
import { errorMessage, toast } from "../stores/toasts.ts";

/**
 * Settings → Spielstände, under the cloud: favourites, notes, collections, wishlist and
 * playtime through the same cloud. Shown once a cloud is connected.
 */
export function LibrarySyncSettings({ enabled, onToggle }: { enabled: boolean; onToggle: (enabled: boolean) => void }) {
  const t = useMessages();
  const status = useQuery("library-sync", api.saveSync.libraryStatus);
  const [busy, setBusy] = useState(false);

  async function syncNow(): Promise<void> {
    setBusy(true);
    try {
      const result = await api.saveSync.syncLibraryNow();
      if (result.problem) toast(result.problem, "danger");
      else toast(t.librarySync.done);
    } catch (error: unknown) {
      toast(errorMessage(error), "danger");
    } finally {
      setBusy(false);
      invalidateQueries();
    }
  }

  if (status.status === "loading") {
    return (
      <LoadingRegion label={t.librarySync.loading}>
        <Skeleton className="mt-4 h-16" />
      </LoadingRegion>
    );
  }
  if (status.status === "error") return <ErrorState message={status.error} onRetry={status.retry} />;
  if (!status.data.connected) return null;
  const { lastSyncedAt, problem } = status.data;

  return (
    <div className="mt-4 rounded-lg border border-border px-4">
      <SettingRow title={t.librarySync.title} description={t.librarySync.hint}>
        <Switch label={t.librarySync.title} checked={enabled} onChange={onToggle} />
      </SettingRow>
      {enabled && (
        <div className="flex items-center justify-between gap-4 border-t border-border py-3">
          <p className={problem ? "text-sm text-danger" : "text-sm text-fg-muted"}>
            {problem ?? (lastSyncedAt ? t.librarySync.last(formatRelative(lastSyncedAt)) : t.librarySync.never)}
          </p>
          <Button size="sm" icon={RefreshCw} disabled={busy} onClick={() => void syncNow()}>
            {busy ? t.librarySync.running : t.librarySync.now}
          </Button>
        </div>
      )}
    </div>
  );
}
