import { useMessages } from "../i18n/index.ts";
import type { LibraryItem } from "@cove/shared";
import { PackageOpen } from "lucide-react";
import { useState } from "react";
import { api } from "../api/index.ts";
import { Button } from "../components/ui/Button.tsx";
import { Dialog } from "../components/ui/Dialog.tsx";
import { EmptyState, ErrorState, LoadingRegion, Skeleton } from "../components/ui/Feedback.tsx";
import { useQuery } from "../hooks/useQuery.ts";
import { invalidateQueries } from "../stores/query.ts";
import { errorMessage, toast } from "../stores/toasts.ts";
import { DlcCheckList } from "./InstallStep.tsx";

/**
 * "DLCs ändern" for a GOG/Epic game Cove installed: tick or untick DLCs;
 * "Übernehmen" queues an update that downloads ticked and removes unticked ones.
 */
export function DlcDialog({ item, open, onClose }: { item: LibraryItem; open: boolean; onClose: () => void }) {
  const t = useMessages();
  const preview = useQuery(`dlc-choice/${item.game.id}`, () => api.downloads.previewStoreInstall(item.game.id));
  /** Changes against the stored choice: id → ticked. */
  const [changes, setChanges] = useState<ReadonlyMap<string, boolean>>(new Map());
  const [busy, setBusy] = useState(false);
  const dlcs = preview.status === "success" ? preview.data.dlcs : [];
  const ticked = (id: string, stored: boolean) => changes.get(id) ?? stored;
  const excluded = new Set(dlcs.filter((d) => !ticked(d.id, d.selected)).map((d) => d.id));
  const changed = dlcs.some((d) => ticked(d.id, d.selected) !== d.selected);

  async function apply(): Promise<void> {
    setBusy(true);
    try {
      await api.library.setGameDlcs(item.game.id, [...excluded]);
      toast(t.game.dlcsQueued(item.game.title));
      invalidateQueries();
      onClose();
    } catch (error: unknown) {
      toast(errorMessage(error), "danger");
    } finally {
      setBusy(false);
    }
  }

  return (
    <Dialog open={open} onClose={onClose} title={t.game.dlcsDialog(item.game.title)}>
      {preview.status === "loading" && (
        <LoadingRegion label={t.game.dlcsLoading} className="flex flex-col gap-2">
          <Skeleton className="h-9" />
          <Skeleton className="h-9" />
        </LoadingRegion>
      )}
      {preview.status === "error" && <ErrorState message={preview.error} onRetry={preview.retry} />}
      {preview.status === "success" && dlcs.length === 0 && (
        <EmptyState compact icon={PackageOpen} title={t.game.noOwnDlcs} description={t.game.noOwnDlcsText} />
      )}
      {preview.status === "success" && dlcs.length > 0 && (
        <div className="flex flex-col gap-4">
          <p className="text-sm text-fg-muted">{t.game.dlcsChangeHint}</p>
          <DlcCheckList
            dlcs={dlcs}
            excluded={excluded}
            legend={t.install.dlcList}
            onToggle={(id, selected) => {
              setChanges((current) => new Map(current).set(id, selected));
            }}
          />
          <div className="flex justify-end gap-2">
            <Button onClick={onClose}>{t.common.cancel}</Button>
            <Button variant="primary" disabled={!changed || busy} onClick={() => void apply()}>
              {t.game.applyDlcs}
            </Button>
          </div>
        </div>
      )}
    </Dialog>
  );
}
