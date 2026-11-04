import type { ItchGame, ItchUpload } from "@cove/shared";
import { ChevronLeft, Download, Gamepad2 } from "lucide-react";
import { useState, type SyntheticEvent } from "react";
import { api } from "../api/index.ts";
import { Button, ButtonLink } from "../components/ui/Button.tsx";
import { Dialog } from "../components/ui/Dialog.tsx";
import { Badge, EmptyState, ErrorState, LoadingRegion, Skeleton } from "../components/ui/Feedback.tsx";
import { Field, Input } from "../components/ui/Field.tsx";
import { useQuery } from "../hooks/useQuery.ts";
import { formatBytes } from "../lib/format.ts";
import { PLATFORM_LABELS } from "../lib/labels.ts";
import { useDownloads } from "../stores/downloads.ts";
import { errorMessage, toast } from "../stores/toasts.ts";

function ListSkeleton() {
  return (
    <LoadingRegion label="Lädt" className="flex flex-col gap-2">
      {[0, 1, 2, 3].map((i) => (
        <Skeleton key={i} className="h-12" />
      ))}
    </LoadingRegion>
  );
}

function Uploads({ game, onBack, onDone }: { game: ItchGame; onBack: () => void; onDone: () => void }) {
  const state = useQuery(`itch/uploads/${String(game.gameId)}`, () => api.downloads.itchUploads(game));
  const [busy, setBusy] = useState(false);

  async function download(upload: ItchUpload): Promise<void> {
    setBusy(true);
    try {
      await api.downloads.addItchDownload(game, upload);
      toast(`„${game.title}“ wird heruntergeladen`);
      await useDownloads.getState().refresh();
      onDone();
    } catch (error: unknown) {
      toast(errorMessage(error), "danger");
    } finally {
      setBusy(false);
    }
  }

  return (
    <div>
      <Button variant="ghost" size="sm" icon={ChevronLeft} onClick={onBack} className="-ml-2">
        Zurück
      </Button>
      <h3 className="mt-2 text-base font-medium text-fg">{game.title}</h3>
      <p className="mt-1 text-sm text-fg-muted">Wähle die Datei für dein System.</p>
      <div className="mt-4">
        {state.status === "loading" && <ListSkeleton />}
        {state.status === "error" && <ErrorState message={state.error} onRetry={state.retry} />}
        {state.status === "success" && state.data.length === 0 && (
          <EmptyState compact icon={Download} title="Keine Dateien" description="Für dieses Spiel gibt es keine herunterladbaren Dateien." />
        )}
        {state.status === "success" && state.data.length > 0 && (
          <ul className="rounded-lg border border-border">
            {state.data.map((upload) => (
              <li key={upload.uploadId} className="flex items-center gap-3 border-b border-border px-4 py-3 last:border-b-0">
                <div className="min-w-0 flex-1">
                  <p className="truncate text-sm font-medium text-fg">{upload.fileName}</p>
                  <p className="num text-xs text-fg-subtle">{upload.sizeBytes !== null ? formatBytes(upload.sizeBytes) : "Größe unbekannt"}</p>
                </div>
                {upload.platforms.map((p) => (
                  <Badge key={p}>{PLATFORM_LABELS[p]}</Badge>
                ))}
                <Button size="sm" variant="primary" disabled={busy} onClick={() => void download(upload)}>
                  Herunterladen
                </Button>
              </li>
            ))}
          </ul>
        )}
      </div>
    </div>
  );
}

function FreeGameLookup({ onFound }: { onFound: (game: ItchGame) => void }) {
  const [url, setUrl] = useState("");
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);

  async function submit(event: SyntheticEvent): Promise<void> {
    event.preventDefault();
    setBusy(true);
    try {
      onFound(await api.downloads.itchLookup(url));
    } catch (e: unknown) {
      setError(errorMessage(e));
    } finally {
      setBusy(false);
    }
  }

  return (
    <form className="mt-6 flex items-end gap-2 border-t border-border pt-4" onSubmit={(e) => void submit(e)} noValidate>
      <div className="min-w-0 flex-1">
        <Field label="Kostenloses Spiel per Link" error={error}>
          {(props) => (
            <Input
              {...props}
              type="url"
              placeholder="https://entwickler.itch.io/spiel"
              value={url}
              onChange={(e) => {
                setUrl(e.target.value);
                setError(null);
              }}
            />
          )}
        </Field>
      </div>
      <Button type="submit" disabled={busy || url.trim() === ""} className={error ? "mb-6" : undefined}>
        Suchen
      </Button>
    </form>
  );
}

function OwnedGames({ onSelect }: { onSelect: (game: ItchGame) => void }) {
  const owned = useQuery("itch/owned", api.downloads.itchOwnedGames);
  return (
    <div>
      {owned.status === "loading" && <ListSkeleton />}
      {owned.status === "error" && <ErrorState message={owned.error} onRetry={owned.retry} />}
      {owned.status === "success" && owned.data.length === 0 && (
        <EmptyState compact icon={Gamepad2} title="Noch keine Spiele" description="Gekaufte und beanspruchte Spiele aus deinem itch.io-Konto erscheinen hier." />
      )}
      {owned.status === "success" && owned.data.length > 0 && (
        <ul className="max-h-80 overflow-y-auto rounded-lg border border-border" aria-label="Deine itch.io-Spiele">
          {owned.data.map((game) => (
            <li key={game.gameId} className="border-b border-border last:border-b-0">
              <button
                type="button"
                className="flex w-full items-center gap-3 px-4 py-3 text-left transition-colors duration-150 ease-out hover:bg-hover"
                onClick={() => {
                  onSelect(game);
                }}
              >
                {game.coverUrl ? (
                  <img src={game.coverUrl} alt="" className="size-10 shrink-0 rounded-md border border-border object-cover" loading="lazy" />
                ) : (
                  <span aria-hidden className="flex size-10 shrink-0 items-center justify-center rounded-md border border-border bg-sidebar text-fg-subtle">
                    <Gamepad2 />
                  </span>
                )}
                <span className="min-w-0 flex-1 truncate text-sm font-medium text-fg">{game.title}</span>
              </button>
            </li>
          ))}
        </ul>
      )}
      <FreeGameLookup onFound={onSelect} />
    </div>
  );
}

function ItchBrowser({ onDone }: { onDone: () => void }) {
  const status = useQuery("itch/status", api.downloads.itchStatus);
  const [selected, setSelected] = useState<ItchGame | null>(null);

  if (selected) {
    return (
      <Uploads
        game={selected}
        onBack={() => {
          setSelected(null);
        }}
        onDone={onDone}
      />
    );
  }
  if (status.status === "loading") return <ListSkeleton />;
  if (status.status === "error") return <ErrorState message={status.error} onRetry={status.retry} />;
  if (!status.data.connected) {
    return (
      <EmptyState
        compact
        icon={Gamepad2}
        title="itch.io ist nicht verbunden"
        description="Trage deinen itch.io-API-Schlüssel in den Einstellungen ein, dann siehst du hier deine Spiele."
        actions={
          <ButtonLink to="/settings/accounts" onClick={onDone}>
            Zu den Einstellungen
          </ButtonLink>
        }
      />
    );
  }
  return <OwnedGames onSelect={setSelected} />;
}

export function ItchDialog({ open, onClose }: { open: boolean; onClose: () => void }) {
  return (
    <Dialog open={open} onClose={onClose} title="Von itch.io herunterladen" description="Spiele, die du besitzt, und kostenlose Spiele.">
      {open && <ItchBrowser onDone={onClose} />}
    </Dialog>
  );
}
