import { useState, type SyntheticEvent } from "react";
import { api } from "../api/index.ts";
import { Button } from "../components/ui/Button.tsx";
import { Dialog } from "../components/ui/Dialog.tsx";
import { Field, Input } from "../components/ui/Field.tsx";
import { useDownloads } from "../stores/downloads.ts";
import { errorMessage, toast } from "../stores/toasts.ts";

/** A direct link: DRM-free games, mods, patches. */
export function AddDownloadDialog({ open, onClose }: { open: boolean; onClose: () => void }) {
  const [url, setUrl] = useState("");
  const [title, setTitle] = useState("");
  const [sha256, setSha256] = useState("");
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);

  function close(): void {
    setUrl("");
    setTitle("");
    setSha256("");
    setError(null);
    onClose();
  }

  async function submit(event: SyntheticEvent): Promise<void> {
    event.preventDefault();
    if (!url.trim()) {
      setError("Bitte gib einen Link ein.");
      return;
    }
    setBusy(true);
    try {
      const item = await api.downloads.addUrlDownload({
        url: url.trim(),
        title: title.trim() || null,
        sha256: sha256.trim() || null,
      });
      toast(`„${item.game.title}“ wird heruntergeladen`);
      await useDownloads.getState().refresh();
      close();
    } catch (e: unknown) {
      setError(errorMessage(e));
    } finally {
      setBusy(false);
    }
  }

  return (
    <Dialog
      open={open}
      onClose={close}
      title="Download per Link"
      description="Für DRM-freie Spiele, Mods und Patches. Archive (zip, 7z, tar.gz) entpackt Cove und fügt das Spiel zur Bibliothek hinzu."
    >
      <form className="flex flex-col gap-4" onSubmit={(e) => void submit(e)} noValidate>
        <Field label="Link" error={error}>
          {(props) => (
            <Input
              {...props}
              autoFocus
              type="url"
              inputMode="url"
              placeholder="https://example.com/spiel-linux.zip"
              value={url}
              onChange={(e) => {
                setUrl(e.target.value);
                setError(null);
              }}
            />
          )}
        </Field>
        <Field label="Name (optional)" description="Ohne Angabe nimmt Cove den Dateinamen.">
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
        <Field label="SHA-256-Prüfsumme (optional)" description="Steht oft neben dem Download-Link. Cove prüft die Datei nach dem Download.">
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
            Abbrechen
          </Button>
          <Button type="submit" variant="primary" disabled={busy}>
            Herunterladen
          </Button>
        </div>
      </form>
    </Dialog>
  );
}
