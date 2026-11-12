import type { RomFolder, SystemInfo } from "@cove/shared";
import { FolderOpen, Gamepad2, Plus, RefreshCw, Trash2 } from "lucide-react";
import { useState, type SyntheticEvent } from "react";
import { api } from "../../api/index.ts";
import { Button } from "../../components/ui/Button.tsx";
import { EmptyState, ErrorState } from "../../components/ui/Feedback.tsx";
import { Field, Input, Select } from "../../components/ui/Field.tsx";
import { useQuery } from "../../hooks/useQuery.ts";
import { useMessages, type Messages } from "../../i18n/index.ts";
import { invalidateQueries } from "../../stores/query.ts";
import { errorMessage, toast } from "../../stores/toasts.ts";
import { Panel, SettingsSkeleton } from "./parts.tsx";
import { useSettings } from "./useSettings.ts";

const CUSTOM = "custom";

function emulatorOptions(system: SystemInfo | undefined, t: Messages) {
  return [
    ...(system?.emulators ?? []).map((e) => ({ value: e.id, label: e.installed ? e.name : t.emulators.notInstalled(e.name) })),
    { value: CUSTOM, label: t.emulators.custom },
  ];
}

/** A new folder: system, path, emulator (and for "own program" its path and arguments). */
function AddFolder({ systems, onAdd }: { systems: SystemInfo[]; onAdd: (folder: RomFolder) => void }) {
  const t = useMessages();
  const [system, setSystem] = useState(systems[0]?.id ?? "");
  const [path, setPath] = useState("");
  const [emulator, setEmulator] = useState("retroarch");
  const [program, setProgram] = useState("");
  const [args, setArgs] = useState("{rom}");
  const info = systems.find((s) => s.id === system);

  async function pick(): Promise<void> {
    try {
      const picked = await api.system.pickFolder(path || null);
      if (picked) setPath(picked);
    } catch (error: unknown) {
      toast(errorMessage(error), "danger");
    }
  }

  function submit(event: SyntheticEvent): void {
    event.preventDefault();
    if (!path.trim()) return;
    onAdd({
      id: `${system}-${Date.now().toString(36)}`,
      system,
      path: path.trim(),
      emulator,
      program: emulator === CUSTOM ? program.trim() : null,
      arguments: emulator === CUSTOM ? args : "",
    });
    setPath("");
  }

  return (
    <form className="mt-6 flex flex-col gap-4 rounded-lg border border-border p-4" onSubmit={submit}>
      <h3 className="text-sm font-medium text-fg">{t.emulators.add}</h3>
      <div className="grid grid-cols-2 gap-4">
        <Field label={t.emulators.system}>
          {(props) => (
            <Select
              {...props}
              value={system}
              onChange={(next) => {
                setSystem(next);
                setEmulator("retroarch");
              }}
              options={systems.map((s) => ({ value: s.id, label: s.name }))}
            />
          )}
        </Field>
        <Field label={t.emulators.emulator}>
          {(props) => <Select {...props} value={emulator} onChange={setEmulator} options={emulatorOptions(info, t)} />}
        </Field>
      </div>
      <Field label={t.emulators.folder} {...(info ? { description: t.emulators.extensions(info.extensions.map((e) => `.${e}`).join(", ")) } : {})}>
        {(props) => (
          <div className="flex gap-2">
            <Input
              {...props}
              spellCheck={false}
              placeholder="/home/alex/ROMs/SNES"
              value={path}
              onChange={(e) => {
                setPath(e.target.value);
              }}
            />
            <Button icon={FolderOpen} onClick={() => void pick()}>
              {t.emulators.pick}
            </Button>
          </div>
        )}
      </Field>
      {emulator === CUSTOM && (
        <div className="grid grid-cols-2 gap-4">
          <Field label={t.emulators.program}>
            {(props) => (
              <Input
                {...props}
                spellCheck={false}
                placeholder="/usr/bin/emulator"
                value={program}
                onChange={(e) => {
                  setProgram(e.target.value);
                }}
              />
            )}
          </Field>
          <Field label={t.emulators.arguments} description={t.emulators.argumentsHint}>
            {(props) => (
              <Input
                {...props}
                spellCheck={false}
                value={args}
                onChange={(e) => {
                  setArgs(e.target.value);
                }}
              />
            )}
          </Field>
        </div>
      )}
      <div>
        <Button type="submit" variant="primary" icon={Plus} disabled={!path.trim()}>
          {t.emulators.addButton}
        </Button>
      </div>
    </form>
  );
}

/** Settings → Emulatoren: ROM folders per system and the emulator for each. */
export function EmulatorsTab() {
  const t = useMessages();
  const { settings, error, retry, save } = useSettings();
  const systems = useQuery("emulator-systems", api.emulators.systems);
  const [scanning, setScanning] = useState(false);
  if (error) return <ErrorState message={error} onRetry={retry} />;
  if (systems.status === "error") return <ErrorState message={systems.error} onRetry={systems.retry} />;
  if (!settings || systems.status !== "success") return <SettingsSkeleton />;
  const folders = settings.emulators.romFolders;
  const systemOf = (id: string) => systems.data.find((s) => s.id === id);

  async function scan(): Promise<void> {
    setScanning(true);
    try {
      const result = await api.emulators.scan();
      toast(t.emulators.scanned(result.added, result.found));
      invalidateQueries();
    } catch (error: unknown) {
      toast(errorMessage(error), "danger");
    } finally {
      setScanning(false);
    }
  }

  return (
    <Panel title={t.emulators.title} description={t.emulators.hint}>
      {folders.length === 0 ? (
        <div className="rounded-lg border border-border">
          <EmptyState compact icon={Gamepad2} title={t.emulators.emptyTitle} description={t.emulators.emptyText} />
        </div>
      ) : (
        <>
          <ul className="rounded-lg border border-border" aria-label={t.emulators.folders}>
            {folders.map((folder) => {
              const system = systemOf(folder.system);
              return (
                <li key={folder.id} className="flex items-center gap-4 border-b border-border py-3 pr-2 pl-4 last:border-b-0">
                  <div className="min-w-0 flex-1">
                    <p className="text-sm font-medium text-fg">{system?.name ?? folder.system}</p>
                    <p className="truncate text-xs text-fg-muted" title={folder.path}>
                      {folder.path}
                    </p>
                  </div>
                  {folder.emulator === CUSTOM ? (
                    <span className="max-w-48 truncate text-sm text-fg-muted" title={folder.program ?? ""}>
                      {folder.program}
                    </span>
                  ) : (
                    <Select
                      className="w-56"
                      aria-label={t.emulators.emulatorFor(system?.name ?? folder.system)}
                      value={folder.emulator}
                      onChange={(emulator) => {
                        save((s) => ({
                          ...s,
                          emulators: { romFolders: s.emulators.romFolders.map((f) => (f.id === folder.id ? { ...f, emulator } : f)) },
                        }));
                      }}
                      options={emulatorOptions(system, t).filter((o) => o.value !== CUSTOM)}
                    />
                  )}
                  <Button
                    variant="ghost"
                    size="sm"
                    iconOnly
                    icon={Trash2}
                    label={t.emulators.remove(folder.path)}
                    onClick={() => {
                      save((s) => ({ ...s, emulators: { romFolders: s.emulators.romFolders.filter((f) => f.id !== folder.id) } }));
                    }}
                  />
                </li>
              );
            })}
          </ul>
          <Button className="mt-3" icon={RefreshCw} disabled={scanning} onClick={() => void scan()}>
            {scanning ? t.emulators.scanning : t.emulators.scan}
          </Button>
        </>
      )}
      <AddFolder
        systems={systems.data}
        onAdd={(folder) => {
          save((s) => ({ ...s, emulators: { romFolders: [...s.emulators.romFolders, folder] } }));
        }}
      />
      <p className="mt-4 text-xs text-fg-muted">{t.emulators.legal}</p>
    </Panel>
  );
}
