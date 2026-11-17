import type { ImportPreview } from "@cove/shared";
import { FileDown, FileUp } from "lucide-react";
import { useState } from "react";
import { api } from "../api/index.ts";
import { Button } from "../components/ui/Button.tsx";
import { Dialog } from "../components/ui/Dialog.tsx";
import { SettingRow, Switch } from "../components/ui/Field.tsx";
import { useMessages } from "../i18n/index.ts";
import { formatDate, formatNumber } from "../lib/format.ts";
import { invalidateQueries } from "../stores/query.ts";
import { errorMessage, toast } from "../stores/toasts.ts";

/** "Daten übertragen": export to a file, import one (merging, never deleting). */
export function DataTransfer() {
  const t = useMessages();
  const [busy, setBusy] = useState(false);
  const [preview, setPreview] = useState<ImportPreview | null>(null);
  const [withSettings, setWithSettings] = useState(false);

  async function exportData(): Promise<void> {
    setBusy(true);
    try {
      const path = await api.system.exportData();
      if (path) toast(t.transfer.exported(path));
    } catch (error: unknown) {
      toast(errorMessage(error), "danger");
    } finally {
      setBusy(false);
    }
  }

  async function pick(): Promise<void> {
    setBusy(true);
    try {
      const picked = await api.system.pickImport();
      if (picked) {
        setWithSettings(false);
        setPreview(picked);
      }
    } catch (error: unknown) {
      toast(errorMessage(error), "danger");
    } finally {
      setBusy(false);
    }
  }

  async function importData(file: ImportPreview): Promise<void> {
    setBusy(true);
    try {
      const result = await api.system.importData(file.path, withSettings);
      toast(t.transfer.imported(result.gamesAdded + result.gamesUpdated, result.sessionsAdded, result.gamesSkipped));
      setPreview(null);
      invalidateQueries();
    } catch (error: unknown) {
      toast(errorMessage(error), "danger");
    } finally {
      setBusy(false);
    }
  }

  return (
    <section aria-labelledby="data-transfer" className="mt-10">
      <h3 id="data-transfer" className="text-base font-semibold text-fg">
        {t.transfer.title}
      </h3>
      <p className="mt-1 text-sm text-fg-muted">{t.transfer.text}</p>
      <div className="mt-4 flex flex-wrap gap-2">
        <Button icon={FileDown} disabled={busy} onClick={() => void exportData()}>
          {t.transfer.export}
        </Button>
        <Button icon={FileUp} disabled={busy} onClick={() => void pick()}>
          {t.transfer.import}
        </Button>
      </div>
      <Dialog
        open={preview !== null}
        onClose={() => {
          setPreview(null);
        }}
        title={t.transfer.importTitle}
        description={preview ? t.transfer.from(formatDate(preview.exportedAt), preview.coveVersion) : undefined}
        footer={
          <>
            <Button
              onClick={() => {
                setPreview(null);
              }}
            >
              {t.common.cancel}
            </Button>
            <Button variant="primary" disabled={busy} onClick={() => void (preview && importData(preview))}>
              {t.transfer.importNow}
            </Button>
          </>
        }
      >
        {preview && (
          <div className="flex flex-col gap-4">
            <dl className="grid grid-cols-2 gap-x-6 gap-y-2 text-sm">
              {(
                [
                  [t.transfer.games, preview.games],
                  [t.transfer.sessions, preview.sessions],
                  [t.collections.title, preview.collections],
                  [t.wishlist.title, preview.wishlist],
                ] as const
              ).map(([label, count]) => (
                <div key={label} className="flex justify-between gap-4 border-b border-border py-1">
                  <dt className="text-fg-muted">{label}</dt>
                  <dd className="num text-fg">{formatNumber(count)}</dd>
                </div>
              ))}
            </dl>
            <p className="text-sm text-fg-muted">{t.transfer.merge}</p>
            <SettingRow title={t.transfer.withSettings} description={t.transfer.withSettingsHint}>
              <Switch label={t.transfer.withSettings} checked={withSettings} onChange={setWithSettings} />
            </SettingRow>
          </div>
        )}
      </Dialog>
    </section>
  );
}
