import { MonitorPlay, Trash2 } from "lucide-react";
import { useState } from "react";
import { api } from "../api/index.ts";
import { Button } from "../components/ui/Button.tsx";
import { ErrorState, LoadingRegion, Skeleton } from "../components/ui/Feedback.tsx";
import { useQuery } from "../hooks/useQuery.ts";
import { useMessages } from "../i18n/index.ts";
import { invalidateQueries } from "../stores/query.ts";
import { errorMessage, toast } from "../stores/toasts.ts";

/** "Streaming": Cove's installed games in Sunshine's app list, for Moonlight. */
export function SunshineStreaming() {
  const t = useMessages();
  const status = useQuery("sunshine", api.system.sunshineStatus);
  const [busy, setBusy] = useState(false);

  async function run(remove: boolean): Promise<void> {
    setBusy(true);
    try {
      const result = remove ? await api.system.sunshineRemove() : await api.system.sunshineSync();
      toast(remove ? t.sunshine.removed : t.sunshine.synced(result.games));
    } catch (error: unknown) {
      toast(errorMessage(error), "danger");
    } finally {
      setBusy(false);
      invalidateQueries();
    }
  }

  return (
    <section aria-labelledby="sunshine" className="mt-10">
      <h3 id="sunshine" className="text-base font-semibold text-fg">
        {t.sunshine.title}
      </h3>
      <p className="mt-1 text-sm text-fg-muted">{t.sunshine.text}</p>
      {status.status === "loading" && (
        <LoadingRegion label={t.sunshine.loading}>
          <Skeleton className="mt-4 h-16" />
        </LoadingRegion>
      )}
      {status.status === "error" && <ErrorState message={status.error} onRetry={status.retry} />}
      {status.status === "success" &&
        (status.data.found ? (
          <>
            <div className="mt-4 rounded-lg border border-border px-4 py-3">
              <p className="num text-sm text-fg">{t.sunshine.games(status.data.games)}</p>
              <p className="truncate text-xs text-fg-muted" title={status.data.appsFile ?? undefined}>
                {status.data.appsFile}
              </p>
            </div>
            <div className="mt-4 flex flex-wrap gap-2">
              <Button icon={MonitorPlay} disabled={busy} onClick={() => void run(false)}>
                {status.data.games > 0 ? t.sunshine.update : t.sunshine.add}
              </Button>
              {status.data.games > 0 && (
                <Button variant="ghost" icon={Trash2} disabled={busy} onClick={() => void run(true)}>
                  {t.sunshine.remove}
                </Button>
              )}
            </div>
            <p className="mt-2 text-xs text-fg-muted">{t.sunshine.restart}</p>
          </>
        ) : (
          <p className="mt-4 rounded-lg border border-border px-4 py-3 text-sm text-fg-muted">{t.sunshine.notFound}</p>
        ))}
    </section>
  );
}
