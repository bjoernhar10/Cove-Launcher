import { useMessages } from "../i18n/index.ts";
import type { InstallTarget, LibraryItem } from "@cove/shared";
import { ExternalLink, TriangleAlert } from "lucide-react";
import { useState } from "react";
import { api } from "../api/index.ts";
import { Button } from "../components/ui/Button.tsx";
import { Dialog } from "../components/ui/Dialog.tsx";
import { ErrorState, LoadingRegion, Skeleton } from "../components/ui/Feedback.tsx";
import { useQuery } from "../hooks/useQuery.ts";
import { useDownloads } from "../stores/downloads.ts";
import { invalidateQueries } from "../stores/query.ts";
import { errorMessage, toast } from "../stores/toasts.ts";
import { InstallStep } from "./InstallStep.tsx";

/**
 * Install dialog for a store game Cove installs itself (GOG, Epic): sizes come from the
 * store's manifests. Epic games with anti-cheat or a third-party launcher go to the Epic Launcher.
 */
export function StoreInstallDialog({ item, open, onClose }: { item: LibraryItem; open: boolean; onClose: () => void }) {
  const t = useMessages();
  const { game } = item;
  const preview = useQuery(`install-preview/${game.id}`, () => api.downloads.previewStoreInstall(game.id));
  const [busy, setBusy] = useState(false);

  /** Hands the install to the Epic Launcher (Heroic on Linux). */
  async function viaLauncher(): Promise<void> {
    setBusy(true);
    try {
      await api.downloads.enqueueDownload(game.id, null);
      toast(t.play.handedOff(game.title, "Epic"));
      invalidateQueries();
      onClose();
    } catch (error: unknown) {
      toast(errorMessage(error), "danger");
    } finally {
      setBusy(false);
    }
  }

  const launcherButton = item.entry.source === "epic" && (
    <Button icon={ExternalLink} disabled={busy} onClick={() => void viaLauncher()}>
      {t.install.viaLauncher}
    </Button>
  );

  async function install(target: InstallTarget): Promise<void> {
    setBusy(true);
    try {
      await api.downloads.enqueueDownload(game.id, target);
      toast(t.itch.downloading(game.title));
      invalidateQueries();
      await useDownloads.getState().refresh();
      onClose();
    } catch (error: unknown) {
      toast(errorMessage(error), "danger");
    } finally {
      setBusy(false);
    }
  }

  return (
    <Dialog open={open} onClose={onClose} title={t.install.title(game.title)}>
      {preview.status === "loading" && (
        <LoadingRegion label={t.install.loadingSizes} className="flex flex-col gap-3">
          <Skeleton className="h-24" />
          <Skeleton className="h-9" />
        </LoadingRegion>
      )}
      {preview.status === "error" && (
        <>
          <ErrorState message={preview.error} onRetry={preview.retry} />
          {launcherButton && <div className="mt-4 flex justify-end">{launcherButton}</div>}
        </>
      )}
      {preview.status === "success" && preview.data.launcherOnly !== null && (
        <div className="flex flex-col gap-6">
          <p className="flex items-start gap-3 text-sm text-fg">
            <TriangleAlert aria-hidden className="mt-0.5 shrink-0" />
            {preview.data.launcherOnly}
          </p>
          <div className="flex justify-end gap-2">
            <Button onClick={onClose}>{t.common.cancel}</Button>
            {launcherButton}
          </div>
        </div>
      )}
      {preview.status === "success" && preview.data.launcherOnly === null && (
        <InstallStep
          subject={{
            game,
            downloadBytes: preview.data.downloadBytes,
            installBytes: preview.data.installBytes,
            dlcs: preview.data.dlcs,
          }}
          busy={busy}
          onCancel={onClose}
          onInstall={(target) => void install(target)}
        />
      )}
    </Dialog>
  );
}
