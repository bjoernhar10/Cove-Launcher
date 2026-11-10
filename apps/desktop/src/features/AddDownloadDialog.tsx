import { useMessages } from "../i18n/index.ts";
import type { InstallPreview, InstallTarget } from "@cove/shared";
import { useState, type SyntheticEvent } from "react";
import { api } from "../api/index.ts";
import { Button } from "../components/ui/Button.tsx";
import { Dialog } from "../components/ui/Dialog.tsx";
import { Field, Input } from "../components/ui/Field.tsx";
import { useDownloads } from "../stores/downloads.ts";
import { invalidateQueries } from "../stores/query.ts";
import { errorMessage, toast } from "../stores/toasts.ts";
import { InstallStep, previewGame } from "./InstallStep.tsx";

/** A direct link: DRM-free games, mods, patches. */
export function AddDownloadDialog({ open, onClose }: { open: boolean; onClose: () => void }) {
  const t = useMessages();
  const [url, setUrl] = useState("");
  const [title, setTitle] = useState("");
  const [sha256, setSha256] = useState("");
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  /** Set once the link was checked: the install dialog step. */
  const [preview, setPreview] = useState<InstallPreview | null>(null);

  function close(): void {
    setUrl("");
    setTitle("");
    setSha256("");
    setError(null);
    setPreview(null);
    onClose();
  }

  const input = () => ({ url: url.trim(), title: title.trim() || null, sha256: sha256.trim() || null });

  async function submit(event: SyntheticEvent): Promise<void> {
    event.preventDefault();
    if (!url.trim()) {
      setError(t.addDownload.linkMissing);
      return;
    }
    setBusy(true);
    try {
      setPreview(await api.downloads.previewUrlDownload(input()));
    } catch (e: unknown) {
      setError(errorMessage(e));
    } finally {
      setBusy(false);
    }
  }

  async function install(target: InstallTarget): Promise<void> {
    setBusy(true);
    try {
      const item = await api.downloads.addUrlDownload(input(), target);
      toast(t.itch.downloading(item.game.title));
      invalidateQueries();
      await useDownloads.getState().refresh();
      close();
    } catch (e: unknown) {
      toast(errorMessage(e), "danger");
    } finally {
      setBusy(false);
    }
  }

  if (preview) {
    return (
      <Dialog open={open} onClose={close} title={t.install.title(preview.title)}>
        <InstallStep
          subject={{ game: previewGame(preview.title), downloadBytes: preview.downloadBytes, installBytes: preview.installBytes }}
          busy={busy}
          onCancel={close}
          onInstall={(target) => void install(target)}
        />
      </Dialog>
    );
  }

  return (
    <Dialog
      open={open}
      onClose={close}
      title={t.downloads.byLink}
      description={t.addDownload.description}
    >
      <form className="flex flex-col gap-4" onSubmit={(e) => void submit(e)} noValidate>
        <Field label="Link" error={error}>
          {(props) => (
            <Input
              {...props}
              autoFocus
              type="url"
              inputMode="url"
              placeholder={t.addDownload.linkPlaceholder}
              value={url}
              onChange={(e) => {
                setUrl(e.target.value);
                setError(null);
              }}
            />
          )}
        </Field>
        <Field label={t.addDownload.name} description={t.addDownload.nameHint}>
          {(props) => (
            <Input
              {...props}
              value={title}
              onChange={(e) => {
                setTitle(e.target.value);
              }}
            />
          )}
        </Field>
        <Field label={t.addDownload.checksum} description={t.addDownload.checksumHint}>
          {(props) => (
            <Input
              {...props}
              spellCheck={false}
              autoComplete="off"
              className="num font-mono"
              value={sha256}
              onChange={(e) => {
                setSha256(e.target.value);
              }}
            />
          )}
        </Field>
        <div className="flex justify-end gap-2 pt-2">
          <Button type="button" onClick={close}>
            {t.common.cancel}
          </Button>
          <Button type="submit" variant="primary" disabled={busy}>
            {t.itch.download}
          </Button>
        </div>
      </form>
    </Dialog>
  );
}
