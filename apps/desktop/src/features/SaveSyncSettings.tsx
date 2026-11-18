import type { CloudProvider } from "@cove/shared";
import { Cloud, CloudOff } from "lucide-react";
import { useState } from "react";
import { api } from "../api/index.ts";
import { Button } from "../components/ui/Button.tsx";
import { ErrorState, LoadingRegion, Skeleton } from "../components/ui/Feedback.tsx";
import { useQuery } from "../hooks/useQuery.ts";
import { useMessages } from "../i18n/index.ts";
import { invalidateQueries } from "../stores/query.ts";
import { errorMessage, toast } from "../stores/toasts.ts";

/** Settings → Spielstände: the user's cloud that carries saves between devices. */
export function SaveSyncSettings() {
  const t = useMessages();
  const status = useQuery("save-sync", api.saveSync.status);
  const [busy, setBusy] = useState<CloudProvider | "disconnect" | null>(null);

  async function connect(provider: CloudProvider): Promise<void> {
    setBusy(provider);
    try {
      await api.saveSync.connect(provider);
      toast(t.saveSync.connected(t.saveSync.providers[provider]));
    } catch (error: unknown) {
      toast(errorMessage(error), "danger");
    } finally {
      setBusy(null);
      invalidateQueries();
    }
  }

  async function disconnect(): Promise<void> {
    setBusy("disconnect");
    try {
      await api.saveSync.disconnect();
      toast(t.saveSync.disconnected);
    } catch (error: unknown) {
      toast(errorMessage(error), "danger");
    } finally {
      setBusy(null);
      invalidateQueries();
    }
  }

  return (
    <section aria-labelledby="save-sync" className="mt-10">
      <h3 id="save-sync" className="text-base font-semibold text-fg">
        {t.saveSync.title}
      </h3>
      <p className="mt-1 text-sm text-fg-muted">{t.saveSync.hint}</p>
      {status.status === "loading" && (
        <LoadingRegion label={t.saveSync.loading}>
          <Skeleton className="mt-4 h-16" />
        </LoadingRegion>
      )}
      {status.status === "error" && <ErrorState message={status.error} onRetry={status.retry} />}
      {status.status === "success" &&
        (() => {
          const data = status.data;
          if (data.provider) {
            const name = t.saveSync.providers[data.provider];
            const provider = data.provider;
            return (
              <>
                <div className="mt-4 flex items-center gap-4 rounded-lg border border-border px-4 py-3">
                  <Cloud aria-hidden className="shrink-0 text-fg-muted" />
                  <div className="min-w-0 flex-1">
                    <p className="truncate text-sm text-fg">{data.account ? t.saveSync.connectedTo(name, data.account) : t.saveSync.connected(name)}</p>
                    <p className="truncate text-xs text-fg-muted">{t.saveSync.device(data.device)}</p>
                  </div>
                  <Button variant="ghost" size="sm" icon={CloudOff} disabled={busy !== null} onClick={() => void disconnect()}>
                    {t.saveSync.disconnect}
                  </Button>
                </div>
                {data.needsReauth && (
                  <div className="mt-4 rounded-lg border border-border px-4 py-3">
                    <p className="text-sm text-fg">{t.saveSync.reauth}</p>
                    <Button className="mt-3" size="sm" disabled={busy !== null} onClick={() => void connect(provider)}>
                      {busy === provider ? t.saveSync.waiting : t.saveSync.reauthButton}
                    </Button>
                  </div>
                )}
                <p className="mt-2 text-xs text-fg-muted">{t.saveSync.how}</p>
              </>
            );
          }
          if (data.available.length === 0) {
            return <p className="mt-4 rounded-lg border border-border px-4 py-3 text-sm text-fg-muted">{t.saveSync.unavailable}</p>;
          }
          return (
            <>
              <div className="mt-4 flex flex-wrap gap-2">
                {data.available.map((provider) => (
                  <Button key={provider} icon={Cloud} disabled={busy !== null} onClick={() => void connect(provider)}>
                    {busy === provider ? t.saveSync.waiting : t.saveSync.connect(t.saveSync.providers[provider])}
                  </Button>
                ))}
              </div>
              <p className="mt-2 text-xs text-fg-muted">{t.saveSync.how}</p>
            </>
          );
        })()}
    </section>
  );
}
