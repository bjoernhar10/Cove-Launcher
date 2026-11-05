import { localized, messages, useMessages } from "../i18n/index.ts";
import type { ExecutableKind, ScanCandidate } from "@cove/shared";
import { FolderSearch } from "lucide-react";
import { useEffect, useRef, useState, type SyntheticEvent } from "react";
import { useNavigate } from "react-router";
import { api } from "../api/index.ts";
import { Button } from "../components/ui/Button.tsx";
import { Dialog } from "../components/ui/Dialog.tsx";
import { Badge, EmptyState, Skeleton } from "../components/ui/Feedback.tsx";
import { Field, Input } from "../components/ui/Field.tsx";
import { Segmented } from "../components/ui/Segmented.tsx";
import { detectPlatform } from "../lib/env.ts";
import { formatBytes } from "../lib/format.ts";
import { invalidateQueries } from "../stores/query.ts";
import { errorMessage, toast } from "../stores/toasts.ts";
import { useUi } from "../stores/ui.ts";

const examplePath = () => (detectPlatform() === "windows" ? messages().game.windowsPathExample : messages().game.linuxPathExample);
const exampleFolder = () => (detectPlatform() === "windows" ? "D:\\Games" : messages().addGame.linuxFolderExample);

export const KIND_LABELS: Record<ExecutableKind, string> = localized((m) => m.addGame.kinds);

function ManualForm({ onDone }: { onDone: () => void }) {
  const t = useMessages();
  const navigate = useNavigate();
  const [title, setTitle] = useState("");
  const [path, setPath] = useState("");
  const [submitted, setSubmitted] = useState(false);
  const [serverError, setServerError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);

  const titleError = submitted && !title.trim() ? t.addGame.nameMissing : null;
  const pathError = submitted && !path.trim() ? t.addGame.pathMissing : serverError;

  async function submit(event: SyntheticEvent): Promise<void> {
    event.preventDefault();
    setSubmitted(true);
    setServerError(null);
    if (!title.trim() || !path.trim()) return;
    setBusy(true);
    try {
      const item = await api.library.addManualGame({ title: title.trim(), executablePath: path.trim(), sizeBytes: null });
      toast(t.detail.added(item.game.title));
      invalidateQueries();
      onDone();
      void navigate(`/library/${item.game.id}`);
    } catch (error: unknown) {
      setServerError(errorMessage(error));
    } finally {
      setBusy(false);
    }
  }

  return (
    <form className="flex flex-col gap-5" onSubmit={(e) => void submit(e)} noValidate>
      <Field label={t.addGame.name} error={titleError}>
        {(props) => (
          <Input
            {...props}
            autoFocus
            placeholder={t.addGame.namePlaceholder}
            value={title}
            onChange={(e) => {
              setTitle(e.target.value);
            }}
          />
        )}
      </Field>
      <Field
        label={t.addGame.path}
        description={t.addGame.pathHint}
        error={pathError}
      >
        {(props) => (
          <Input
            {...props}
            placeholder={examplePath()}
            spellCheck={false}
            value={path}
            onChange={(e) => {
              setPath(e.target.value);
              setServerError(null);
            }}
          />
        )}
      </Field>
      <div className="flex justify-end gap-2 pt-1">
        <Button onClick={onDone}>{t.common.cancel}</Button>
        <Button type="submit" variant="primary" disabled={busy}>
          {t.addGame.add}
        </Button>
      </div>
    </form>
  );
}

type ScanPhase = { phase: "idle" } | { phase: "scanning" } | { phase: "error"; message: string } | { phase: "done"; candidates: ScanCandidate[] };

function ScanForm({ onDone, initialFolder }: { onDone: () => void; initialFolder: string | undefined }) {
  const t = useMessages();
  const [folder, setFolder] = useState(initialFolder ?? "");
  const [state, setState] = useState<ScanPhase>({ phase: "idle" });
  const [selected, setSelected] = useState<Set<string>>(new Set());
  const [busy, setBusy] = useState(false);
  const autoStarted = useRef(false);

  async function runScan(path: string): Promise<void> {
    if (!path.trim()) return;
    setState({ phase: "scanning" });
    try {
      const candidates = await api.library.scanFolder(path.trim());
      setSelected(new Set(candidates.filter((c) => !c.alreadyInLibrary).map((c) => c.executablePath)));
      setState({ phase: "done", candidates });
    } catch (error: unknown) {
      setState({ phase: "error", message: errorMessage(error) });
    }
  }

  useEffect(() => {
    if (initialFolder && !autoStarted.current) {
      autoStarted.current = true;
      void runScan(initialFolder);
    }
  }, [initialFolder]);

  async function addSelected(candidates: ScanCandidate[]): Promise<void> {
    setBusy(true);
    const chosen = candidates.filter((c) => selected.has(c.executablePath));
    const { added, failed } = await api.library.addScannedGames(chosen);
    setBusy(false);
    invalidateQueries();
    if (added.length > 0) toast(t.addGame.added(added.length));
    for (const f of failed) toast(`${f.title}: ${f.message}`, "danger");
    if (failed.length === 0) onDone();
  }

  return (
    <div className="flex flex-col gap-5">
      <form
        className="flex items-start gap-2"
        onSubmit={(e) => {
          e.preventDefault();
          void runScan(folder);
        }}
      >
        <div className="flex-1">
          <Field
            label={t.addGame.folder}
            description={t.addGame.folderHint}
            error={state.phase === "error" ? state.message : null}
          >
            {(props) => (
              <Input
                {...props}
                autoFocus
                placeholder={exampleFolder()}
                spellCheck={false}
                value={folder}
                onChange={(e) => {
                  setFolder(e.target.value);
                }}
              />
            )}
          </Field>
        </div>
        <Button type="submit" icon={FolderSearch} disabled={!folder.trim() || state.phase === "scanning"} className="mt-7">
          {t.addGame.scan}
        </Button>
      </form>

      {state.phase === "scanning" && (
        <div role="status" aria-label={t.addGame.scanning} className="flex flex-col gap-2">
          {[0, 1, 2].map((i) => (
            <Skeleton key={i} className="h-12" />
          ))}
        </div>
      )}

      {state.phase === "done" && state.candidates.length === 0 && (
        <EmptyState compact icon={FolderSearch} title={t.catalogue.emptyTitle} description={t.addGame.nothingFound} />
      )}

      {state.phase === "done" && state.candidates.length > 0 && (
        <>
          <fieldset className="rounded-lg border border-border">
            <legend className="sr-only">{t.addGame.found}</legend>
            {state.candidates.map((c) => (
              <label
                key={c.executablePath}
                className={
                  "flex items-center gap-3 border-b border-border px-4 py-3 last:border-b-0 " +
                  (c.alreadyInLibrary ? "opacity-60" : "cursor-pointer hover:bg-hover")
                }
              >
                <input
                  type="checkbox"
                  className="size-4"
                  disabled={c.alreadyInLibrary}
                  checked={selected.has(c.executablePath)}
                  onChange={(e) => {
                    const next = new Set(selected);
                    if (e.target.checked) next.add(c.executablePath);
                    else next.delete(c.executablePath);
                    setSelected(next);
                  }}
                />
                <span className="min-w-0 flex-1">
                  <span className="flex items-center gap-2">
                    <span className="truncate text-sm font-medium text-fg">{c.title}</span>
                    <Badge>{KIND_LABELS[c.kind]}</Badge>
                    {c.alreadyInLibrary && <span className="text-xs text-fg-subtle">{t.addGame.alreadyInLibrary}</span>}
                  </span>
                  <span className="block truncate text-xs text-fg-subtle" title={c.executablePath}>
                    {c.executablePath}
                  </span>
                </span>
                <span className="num shrink-0 text-xs text-fg-muted">{formatBytes(c.sizeBytes)}</span>
              </label>
            ))}
          </fieldset>
          <div className="flex justify-end gap-2">
            <Button onClick={onDone}>{t.common.cancel}</Button>
            <Button variant="primary" disabled={selected.size === 0 || busy} onClick={() => void addSelected(state.candidates)}>
              {t.addGame.addCount(selected.size)}
            </Button>
          </div>
        </>
      )}
    </div>
  );
}

export function AddGameDialog() {
  const t = useMessages();
  const options = useUi((s) => s.addGame);
  const close = useUi((s) => s.closeAddGame);
  const [mode, setMode] = useState<"manual" | "scan">("manual");
  const [prevOptions, setPrevOptions] = useState(options);

  // Follow the requested mode whenever the dialog is (re)opened.
  if (options !== prevOptions) {
    setPrevOptions(options);
    if (options) setMode(options.mode);
  }

  return (
    <Dialog
      open={options !== null}
      onClose={close}
      title={t.palette.addGame}
      description={t.addGame.description}
      className="w-[600px]"
    >
      <Segmented
        label={t.addGame.mode}
        value={mode}
        onChange={setMode}
        className="mb-6"
        options={[
          { value: "manual", label: t.labels.source.manual },
          { value: "scan", label: t.addGame.scanFolder },
        ]}
      />
      {mode === "manual" ? <ManualForm onDone={close} /> : <ScanForm onDone={close} initialFolder={options?.folder} />}
    </Dialog>
  );
}
