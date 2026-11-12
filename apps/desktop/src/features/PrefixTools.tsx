import type { LibraryItem, PrefixAction } from "@cove/shared";
import { FolderOpen, RotateCcw, Wrench } from "lucide-react";
import { useState } from "react";
import { api } from "../api/index.ts";
import { Button } from "../components/ui/Button.tsx";
import { Dialog } from "../components/ui/Dialog.tsx";
import { ErrorState, LoadingRegion, Skeleton } from "../components/ui/Feedback.tsx";
import { useQuery } from "../hooks/useQuery.ts";
import { useMessages } from "../i18n/index.ts";
import { formatBytes } from "../lib/format.ts";
import { invalidateQueries } from "../stores/query.ts";
import { errorMessage, toast } from "../stores/toasts.ts";

/** Linux, game with Proton: open the prefix, run winetricks on it, or start over with a fresh one. */
export function PrefixTools({ item }: { item: LibraryItem }) {
  const t = useMessages();
  const id = item.game.id;
  const info = useQuery(`prefix/${id}`, () => api.library.prefixInfo(id));
  const [busy, setBusy] = useState<PrefixAction | null>(null);
  const [confirming, setConfirming] = useState(false);

  async function run(action: PrefixAction): Promise<void> {
    setBusy(action);
    try {
      await api.library.prefixAction(id, action);
      if (action === "reset") toast(t.prefix.resetDone(item.game.title));
      if (action === "winetricks") toast(t.prefix.winetricksStarted);
    } catch (error: unknown) {
      toast(errorMessage(error), "danger");
    } finally {
      setBusy(null);
      setConfirming(false);
      invalidateQueries();
    }
  }

  return (
    <section aria-labelledby={`prefix-${id}`} className="max-w-2xl border-t border-border pt-6">
      <h3 id={`prefix-${id}`} className="text-sm font-medium text-fg">
        {t.prefix.title}
      </h3>
      <p className="mt-1 text-sm text-fg-muted">{t.prefix.hint}</p>
      {info.status === "loading" && (
        <LoadingRegion label={t.prefix.loading}>
          <Skeleton className="mt-4 h-9 w-80" />
        </LoadingRegion>
      )}
      {info.status === "error" && <ErrorState message={info.error} onRetry={info.retry} />}
      {info.status === "success" && (
        <>
          <p className="num mt-3 text-sm text-fg-muted">
            {info.data.exists && info.data.sizeBytes !== null ? t.prefix.size(formatBytes(info.data.sizeBytes)) : t.prefix.none}
          </p>
          <div className="mt-4 flex flex-wrap gap-2">
            <Button icon={FolderOpen} disabled={!info.data.exists || busy !== null} onClick={() => void run("open")}>
              {t.prefix.open}
            </Button>
            <Button
              icon={Wrench}
              disabled={!info.data.exists || !info.data.winetricks || busy !== null}
              onClick={() => void run("winetricks")}
            >
              {t.prefix.winetricks}
            </Button>
            <Button
              variant="danger"
              icon={RotateCcw}
              disabled={!info.data.exists || busy !== null}
              onClick={() => {
                setConfirming(true);
              }}
            >
              {t.prefix.reset}
            </Button>
          </div>
          {info.data.exists && !info.data.winetricks && <p className="mt-2 text-xs text-fg-muted">{t.prefix.noWinetricks}</p>}
        </>
      )}
      <Dialog
        open={confirming}
        onClose={() => {
          setConfirming(false);
        }}
        title={t.prefix.resetConfirm(item.game.title)}
        description={t.prefix.resetText}
        footer={
          <>
            <Button
              onClick={() => {
                setConfirming(false);
              }}
            >
              {t.common.cancel}
            </Button>
            <Button variant="danger" disabled={busy !== null} onClick={() => void run("reset")}>
              {t.prefix.reset}
            </Button>
          </>
        }
      >
        {null}
      </Dialog>
    </section>
  );
}
