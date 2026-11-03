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

const EXAMPLE_PATH = detectPlatform() === "windows" ? "C:\\Games\\Spiel\\spiel.exe" : "/home/du/Games/spiel/spiel.x86_64";
const EXAMPLE_FOLDER = detectPlatform() === "windows" ? "D:\\Games" : "/home/du/Games";

export const KIND_LABELS: Record<ExecutableKind, string> = {
  "windows-exe": "Windows",
  "linux-binary": "Linux",
  appimage: "AppImage",
  "desktop-entry": "Desktop-Eintrag",
  script: "Skript",
};

function ManualForm({ onDone }: { onDone: () => void }) {
  const navigate = useNavigate();
  const [title, setTitle] = useState("");
  const [path, setPath] = useState("");
  const [submitted, setSubmitted] = useState(false);
  const [serverError, setServerError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);

  const titleError = submitted && !title.trim() ? "Bitte gib einen Namen ein." : null;
  const pathError = submitted && !path.trim() ? "Bitte gib den Pfad zur ausführbaren Datei an." : serverError;

  async function submit(event: SyntheticEvent): Promise<void> {
    event.preventDefault();
    setSubmitted(true);
    setServerError(null);
    if (!title.trim() || !path.trim()) return;
    setBusy(true);
    try {
      const item = await api.library.addManualGame({ title: title.trim(), executablePath: path.trim(), sizeBytes: null });
      toast(`„${item.game.title}“ wurde zur Bibliothek hinzugefügt`);
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
      <Field label="Name" error={titleError}>
        {(props) => (
          <Input
            {...props}
            autoFocus
            placeholder="z. B. Mein Lieblingsspiel"
            value={title}
            onChange={(e) => {
              setTitle(e.target.value);
            }}
          />
        )}
      </Field>
      <Field
        label="Pfad zur .exe / Binary"
        description="Die Datei, mit der das Spiel startet: .exe, Linux-Binary, AppImage, .desktop oder Skript."
        error={pathError}
      >
        {(props) => (
          <Input
            {...props}
            placeholder={EXAMPLE_PATH}
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
        <Button onClick={onDone}>Abbrechen</Button>
        <Button type="submit" variant="primary" disabled={busy}>
          Hinzufügen
        </Button>
      </div>
    </form>
  );
}

type ScanPhase = { phase: "idle" } | { phase: "scanning" } | { phase: "error"; message: string } | { phase: "done"; candidates: ScanCandidate[] };

function ScanForm({ onDone, initialFolder }: { onDone: () => void; initialFolder: string | undefined }) {
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
    if (added.length > 0) toast(added.length === 1 ? "1 Spiel hinzugefügt" : `${String(added.length)} Spiele hinzugefügt`);
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
            label="Ordner"
            description="Cove schlägt pro Spiel eine Startdatei vor. Installer, Uninstaller und Crash-Reporter werden übersprungen."
            error={state.phase === "error" ? state.message : null}
          >
            {(props) => (
              <Input
                {...props}
                autoFocus
                placeholder={EXAMPLE_FOLDER}
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
          Scannen
        </Button>
      </form>

      {state.phase === "scanning" && (
        <div role="status" aria-label="Ordner wird durchsucht" className="flex flex-col gap-2">
          {[0, 1, 2].map((i) => (
            <Skeleton key={i} className="h-12" />
          ))}
        </div>
      )}

      {state.phase === "done" && state.candidates.length === 0 && (
        <EmptyState compact icon={FolderSearch} title="Keine Spiele gefunden" description="In diesem Ordner hat Cove nichts Startbares entdeckt." />
      )}

      {state.phase === "done" && state.candidates.length > 0 && (
        <>
          <fieldset className="rounded-lg border border-border">
            <legend className="sr-only">Gefundene Spiele</legend>
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
                    {c.alreadyInLibrary && <span className="text-xs text-fg-subtle">Bereits in der Bibliothek</span>}
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
            <Button onClick={onDone}>Abbrechen</Button>
            <Button variant="primary" disabled={selected.size === 0 || busy} onClick={() => void addSelected(state.candidates)}>
              {selected.size === 1 ? "1 Spiel hinzufügen" : `${String(selected.size)} Spiele hinzufügen`}
            </Button>
          </div>
        </>
      )}
    </div>
  );
}

export function AddGameDialog() {
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
      title="Spiel hinzufügen"
      description="Füge ein Spiel hinzu, das nicht aus einem verbundenen Store stammt."
      className="w-[600px]"
    >
      <Segmented
        label="Art"
        value={mode}
        onChange={setMode}
        className="mb-6"
        options={[
          { value: "manual", label: "Manuell" },
          { value: "scan", label: "Ordner scannen" },
        ]}
      />
      {mode === "manual" ? <ManualForm onDone={close} /> : <ScanForm onDone={close} initialFolder={options?.folder} />}
    </Dialog>
  );
}
