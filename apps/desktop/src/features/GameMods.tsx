import type { LibraryItem, ModInfo } from "@cove/shared";
import { ArrowDown, ArrowUp, FileArchive, FolderPlus, Puzzle, Trash2 } from "lucide-react";
import { useState } from "react";
import { api } from "../api/index.ts";
import { Button } from "../components/ui/Button.tsx";
import { Dialog } from "../components/ui/Dialog.tsx";
import { EmptyState, ErrorState, LoadingRegion, Skeleton } from "../components/ui/Feedback.tsx";
import { Field, Input, Switch } from "../components/ui/Field.tsx";
import { useQuery } from "../hooks/useQuery.ts";
import { useMessages } from "../i18n/index.ts";
import { formatBytes, formatNumber } from "../lib/format.ts";
import { invalidateQueries } from "../stores/query.ts";
import { errorMessage, toast } from "../stores/toasts.ts";

/** The name a mod gets from its file: "HD Pack-1.2.zip" → "HD Pack-1.2". */
function nameOf(path: string): string {
  const file = path.split(/[\\/]/).pop() ?? path;
  return file.replace(/\.(zip|7z|tar\.gz|tgz|tar)$/i, "");
}

function AddDialog({ item, source, onClose }: { item: LibraryItem; source: string; onClose: () => void }) {
  const t = useMessages();
  const [name, setName] = useState(nameOf(source));
  const [target, setTarget] = useState("");
  const [busy, setBusy] = useState(false);

  async function add(): Promise<void> {
    setBusy(true);
    try {
      await api.library.addMod(item.game.id, source, name, target);
      toast(t.mods.added(name));
      invalidateQueries();
      onClose();
    } catch (error: unknown) {
      toast(errorMessage(error), "danger");
    } finally {
      setBusy(false);
    }
  }

  return (
    <Dialog
      open
      onClose={onClose}
      title={t.mods.addTitle}
      description={source}
      footer={
        <>
          <Button onClick={onClose}>{t.common.cancel}</Button>
          <Button variant="primary" disabled={busy} onClick={() => void add()}>
            {t.mods.add}
          </Button>
        </>
      }
    >
      <div className="flex flex-col gap-4">
        <Field label={t.mods.name}>
          {(props) => (
            <Input
              {...props}
              value={name}
              onChange={(e) => {
                setName(e.target.value);
              }}
            />
          )}
        </Field>
        <Field label={t.mods.target} description={t.mods.targetHint}>
          {(props) => (
            <Input
              {...props}
              spellCheck={false}
              placeholder={t.mods.gameFolder}
              value={target}
              onChange={(e) => {
                setTarget(e.target.value);
              }}
            />
          )}
        </Field>
      </div>
    </Dialog>
  );
}

function ModRow({
  gameId,
  mod,
  index,
  count,
  busy,
  onChange,
  onRemove,
}: {
  gameId: string;
  mod: ModInfo;
  index: number;
  count: number;
  busy: boolean;
  onChange: (run: () => Promise<unknown>) => void;
  onRemove: (mod: ModInfo) => void;
}) {
  const t = useMessages();
  return (
    <li className="flex items-center gap-3 border-b border-border py-2 pr-2 pl-4 last:border-b-0">
      <Switch
        label={t.mods.enabled(mod.name)}
        checked={mod.enabled}
        disabled={busy}
        onChange={(enabled) => {
          onChange(() => api.library.setModEnabled(gameId, mod.id, enabled));
        }}
      />
      <div className="min-w-0 flex-1">
        <p className={mod.enabled ? "truncate text-sm font-medium text-fg" : "truncate text-sm font-medium text-fg-muted"}>{mod.name}</p>
        <p className="num truncate text-xs text-fg-muted">
          {t.mods.files(formatNumber(mod.files), formatBytes(mod.sizeBytes))} · {mod.target === "" ? t.mods.gameFolder : mod.target}
          {mod.overridden > 0 && ` · ${t.mods.overridden(formatNumber(mod.overridden))}`}
        </p>
      </div>
      <Button
        variant="ghost"
        size="sm"
        iconOnly
        icon={ArrowUp}
        label={t.mods.up(mod.name)}
        disabled={busy || index === 0}
        onClick={() => {
          onChange(() => api.library.moveMod(gameId, mod.id, index - 1));
        }}
      />
      <Button
        variant="ghost"
        size="sm"
        iconOnly
        icon={ArrowDown}
        label={t.mods.down(mod.name)}
        disabled={busy || index === count - 1}
        onClick={() => {
          onChange(() => api.library.moveMod(gameId, mod.id, index + 1));
        }}
      />
      <Button
        variant="ghost"
        size="sm"
        iconOnly
        icon={Trash2}
        label={t.mods.remove(mod.name)}
        disabled={busy}
        onClick={() => {
          onRemove(mod);
        }}
      />
    </li>
  );
}

/** "Mods" on a library game page: add, switch on and off, order, remove. */
export function GameMods({ item }: { item: LibraryItem }) {
  const t = useMessages();
  const id = item.game.id;
  const query = useQuery(`mods/${id}`, () => api.library.mods(id));
  const [source, setSource] = useState<string | null>(null);
  const [removing, setRemoving] = useState<ModInfo | null>(null);
  const [busy, setBusy] = useState(false);

  async function run(action: () => Promise<unknown>): Promise<void> {
    setBusy(true);
    try {
      await action();
    } catch (error: unknown) {
      toast(errorMessage(error), "danger");
    } finally {
      setBusy(false);
      invalidateQueries();
    }
  }

  async function pick(folder: boolean): Promise<void> {
    try {
      const picked = await api.library.pickMod(folder);
      if (picked) setSource(picked);
    } catch (error: unknown) {
      toast(errorMessage(error), "danger");
    }
  }

  return (
    <section aria-label={t.mods.title} className="flex flex-col gap-6">
      <div className="flex flex-wrap items-start justify-between gap-4">
        <p className="max-w-2xl text-sm text-fg-muted">{t.mods.text}</p>
        <div className="flex shrink-0 gap-2">
          <Button icon={FileArchive} disabled={busy} onClick={() => void pick(false)}>
            {t.mods.addFile}
          </Button>
          <Button icon={FolderPlus} disabled={busy} onClick={() => void pick(true)}>
            {t.mods.addFolder}
          </Button>
        </div>
      </div>
      {query.status === "loading" && (
        <LoadingRegion label={t.mods.loading}>
          <Skeleton className="h-32" />
        </LoadingRegion>
      )}
      {query.status === "error" && <ErrorState message={query.error} onRetry={query.retry} />}
      {query.status === "success" && query.data.length === 0 && (
        <div className="rounded-lg border border-border">
          <EmptyState compact icon={Puzzle} title={t.mods.emptyTitle} description={t.mods.emptyText} />
        </div>
      )}
      {query.status === "success" && query.data.length > 0 && (
        <div>
          <ul className="rounded-lg border border-border" aria-label={t.mods.list}>
            {query.data.map((mod, index) => (
              <ModRow
                key={mod.id}
                gameId={id}
                mod={mod}
                index={index}
                count={query.data.length}
                busy={busy}
                onChange={(action) => {
                  void run(action);
                }}
                onRemove={setRemoving}
              />
            ))}
          </ul>
          <p className="mt-3 text-xs text-fg-subtle">{t.mods.order}</p>
        </div>
      )}
      <p className="text-xs text-fg-subtle">{t.mods.warning}</p>
      {source && (
        <AddDialog
          item={item}
          source={source}
          onClose={() => {
            setSource(null);
          }}
        />
      )}
      <Dialog
        open={removing !== null}
        onClose={() => {
          setRemoving(null);
        }}
        title={removing ? t.mods.removeConfirm(removing.name) : ""}
        description={t.mods.removeText}
        footer={
          <>
            <Button
              onClick={() => {
                setRemoving(null);
              }}
            >
              {t.common.cancel}
            </Button>
            <Button
              variant="danger"
              disabled={busy}
              onClick={() => {
                const mod = removing;
                setRemoving(null);
                if (mod) void run(() => api.library.removeMod(id, mod.id));
              }}
            >
              {t.mods.removeNow}
            </Button>
          </>
        }
      >
        {null}
      </Dialog>
    </section>
  );
}
