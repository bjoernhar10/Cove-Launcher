import { useMessages } from "../i18n/index.ts";
import type { DlcOption, FolderInfo, Game, InstallTarget } from "@cove/shared";
import { ChevronDown, ChevronUp, CircleAlert, FolderOpen, TriangleAlert } from "lucide-react";
import { useEffect, useId, useState } from "react";
import { api } from "../api/index.ts";
import { GameCover } from "../components/game/GameCover.tsx";
import { Button } from "../components/ui/Button.tsx";
import { ErrorState, LoadingRegion, Skeleton } from "../components/ui/Feedback.tsx";
import { Select } from "../components/ui/Field.tsx";
import { useQuery } from "../hooks/useQuery.ts";
import { folderName, joinPath } from "../lib/folders.ts";
import { formatBytes } from "../lib/format.ts";
import { errorMessage } from "../stores/toasts.ts";

/** What the install dialog shows about the game. */
export interface InstallSubject {
  game: Pick<Game, "id" | "title" | "artwork">;
  downloadBytes: number | null;
  /** Size after installing; null when it is only known after unpacking. */
  installBytes: number | null;
  /** DLCs that can be installed with the game; the sizes above are the game alone. */
  dlcs?: DlcOption[];
}

interface InstallStepProps {
  subject: InstallSubject;
  /** What the optional add-ons are: store DLCs or itch.io extras (soundtrack …). */
  addOns?: "dlc" | "extra";
  busy: boolean;
  onCancel: () => void;
  onInstall: (target: InstallTarget) => void;
}

const OTHER = "\u0000other";

/** The game plus the ticked DLCs (unknown DLC sizes count as nothing). */
function withDlcs(subject: InstallSubject, excluded: ReadonlySet<string>): InstallSubject {
  const chosen = (subject.dlcs ?? []).filter((d) => !excluded.has(d.id));
  const add = (base: number | null, key: "downloadBytes" | "installBytes") =>
    base === null ? null : chosen.reduce((sum, d) => sum + (d[key] ?? 0), base);
  return { ...subject, downloadBytes: add(subject.downloadBytes, "downloadBytes"), installBytes: add(subject.installBytes, "installBytes") };
}

/** "DLCs auswählen": a button that opens the list of DLCs to tick or untick. */
function DlcChooser({
  dlcs,
  kind,
  excluded,
  onToggle,
}: {
  dlcs: DlcOption[];
  kind: "dlc" | "extra";
  excluded: ReadonlySet<string>;
  onToggle: (id: string, selected: boolean) => void;
}) {
  const t = useMessages();
  const [open, setOpen] = useState(false);
  const listId = useId();
  const selected = dlcs.filter((d) => !excluded.has(d.id)).length;
  return (
    <div className="flex flex-col gap-2">
      <div className="flex items-center justify-between gap-4">
        <span className="num text-sm text-fg-muted">
          {kind === "extra" ? t.install.extraCount(selected, dlcs.length) : t.install.dlcCount(selected, dlcs.length)}
        </span>
        <Button
          icon={open ? ChevronUp : ChevronDown}
          aria-expanded={open}
          aria-controls={listId}
          onClick={() => {
            setOpen((o) => !o);
          }}
        >
          {kind === "extra" ? t.install.chooseExtras : t.install.chooseDlcs}
        </Button>
      </div>
      {open && (
        <DlcCheckList
          id={listId}
          dlcs={dlcs}
          excluded={excluded}
          legend={kind === "extra" ? t.install.extraList : t.install.dlcList}
          onToggle={onToggle}
        />
      )}
    </div>
  );
}

/** DLCs (or extras) with a tick box and their download size each. */
export function DlcCheckList({
  id,
  dlcs,
  excluded,
  legend,
  onToggle,
}: {
  id?: string;
  dlcs: DlcOption[];
  excluded: ReadonlySet<string>;
  legend: string;
  onToggle: (id: string, selected: boolean) => void;
}) {
  return (
    <fieldset id={id} className="max-h-64 overflow-y-auto rounded-lg border border-border">
      <legend className="sr-only">{legend}</legend>
      {dlcs.map((dlc) => (
        <label key={dlc.id} className="flex items-center gap-3 border-b border-border px-3 py-2 text-sm text-fg last:border-b-0">
          <input
            type="checkbox"
            className="size-4 shrink-0 accent-accent"
            checked={!excluded.has(dlc.id)}
            onChange={(e) => {
              onToggle(dlc.id, e.target.checked);
            }}
          />
          <span className="min-w-0 flex-1 truncate">{dlc.title}</span>
          {dlc.downloadBytes !== null && <span className="num shrink-0 text-fg-muted">{formatBytes(dlc.downloadBytes)}</span>}
        </label>
      ))}
    </fieldset>
  );
}

/** Space needed on the target drive; archives count twice (download + unpacked). */
function neededBytes(subject: InstallSubject): { bytes: number; estimate: boolean } | null {
  if (subject.installBytes !== null) return { bytes: Math.max(subject.installBytes, subject.downloadBytes ?? 0), estimate: false };
  if (subject.downloadBytes !== null) return { bytes: subject.downloadBytes * 2, estimate: true };
  return null;
}

/** Free space, write access and the "Program Files" warning of the chosen folder. */
function useFolderInfo(folder: string | null): { info: FolderInfo | null; error: string | null } {
  const [state, setState] = useState<{ folder: string; info: FolderInfo | null; error: string | null } | null>(null);
  useEffect(() => {
    if (!folder) return;
    let active = true;
    api.system.folderInfo(folder).then(
      (info) => {
        if (active) setState({ folder, info, error: null });
      },
      (error: unknown) => {
        if (active) setState({ folder, info: null, error: errorMessage(error) });
      },
    );
    return () => {
      active = false;
    };
  }, [folder]);
  return state?.folder === folder ? state : { info: null, error: null };
}

/**
 * The install dialog's content: game, sizes, target folder with free space and
 * "Installieren". Shown before every download Cove installs itself.
 */
export function InstallStep({ subject: base, addOns = "dlc", busy, onCancel, onInstall }: InstallStepProps) {
  const t = useMessages();
  const [excluded, setExcluded] = useState<ReadonlySet<string>>(
    () => new Set((base.dlcs ?? []).filter((d) => !d.selected).map((d) => d.id)),
  );
  const subject = withDlcs(base, excluded);
  const settings = useQuery("settings", api.settings.getSettings);
  const [chosen, setChosen] = useState<string | null>(null);
  const [extra, setExtra] = useState<string[]>([]);
  const [makeDefault, setMakeDefault] = useState(false);
  const checkboxId = useId();
  const defaultFolder = settings.status === "success" ? settings.data.downloads.directory : null;
  const folder = chosen ?? defaultFolder;
  const { info, error } = useFolderInfo(folder);

  if (settings.status === "error") return <ErrorState message={settings.error} onRetry={settings.retry} />;
  if (settings.status !== "success" || !folder) {
    return (
      <LoadingRegion label={t.common.loading} className="flex flex-col gap-3">
        <Skeleton className="h-24" />
        <Skeleton className="h-9" />
      </LoadingRegion>
    );
  }

  const folders = [...new Set([...settings.data.library.gameFolders, ...extra, folder])];
  const needed = neededBytes(subject);
  const free = info?.freeBytes ?? null;
  const tooSmall = needed !== null && free !== null && free < needed.bytes;
  const blocked = busy || !info || !info.writable || tooSmall;

  async function pick(): Promise<void> {
    try {
      const picked = await api.system.pickFolder(folder);
      if (!picked) return;
      setExtra((list) => (list.includes(picked) ? list : [...list, picked]));
      setChosen(picked);
    } catch {
      // No native dialog (browser preview): the dropdown still works.
    }
  }

  return (
    <div className="flex flex-col gap-6">
      <div className="flex gap-4">
        <GameCover game={subject.game} showTitle={false} className="w-16 shrink-0 rounded-md" />
        <div className="min-w-0">
          <p className="truncate text-base font-medium text-fg">{subject.game.title}</p>
          <dl className="num mt-2 grid grid-cols-[auto_1fr] gap-x-4 gap-y-1 text-sm" aria-label={t.install.sizes}>
            <dt className="text-fg-muted">{t.install.downloadSize}</dt>
            <dd className="text-fg">{subject.downloadBytes !== null ? formatBytes(subject.downloadBytes) : t.common.unknown}</dd>
            <dt className="text-fg-muted">{t.install.installSize}</dt>
            <dd className="text-fg">{subject.installBytes !== null ? formatBytes(subject.installBytes) : t.install.afterUnpack}</dd>
          </dl>
        </div>
      </div>

      {base.dlcs && base.dlcs.length > 0 && (
        <DlcChooser
          dlcs={base.dlcs}
          kind={addOns}
          excluded={excluded}
          onToggle={(id, selected) => {
            setExcluded((current) => {
              const next = new Set(current);
              if (selected) next.delete(id);
              else next.add(id);
              return next;
            });
          }}
        />
      )}

      <div className="flex flex-col gap-2">
        <span className="text-sm font-medium text-fg" id={`${checkboxId}-folder`}>
          {t.install.folder}
        </span>
        <div className="flex gap-2">
          <Select
            className="min-w-0 flex-1"
            aria-labelledby={`${checkboxId}-folder`}
            value={folder}
            onChange={(value) => {
              if (value === OTHER) void pick();
              else setChosen(value);
            }}
            options={[...folders.map((f) => ({ value: f, label: f })), { value: OTHER, label: t.install.otherFolder }]}
          />
          <Button icon={FolderOpen} onClick={() => void pick()}>
            {t.install.change}
          </Button>
        </div>
        <p className="truncate text-xs text-fg-subtle">{t.install.installsInto(joinPath(folder, folderName(subject.game.title)))}</p>
        <div className="num flex flex-col gap-1 text-sm" aria-live="polite">
          {error ? (
            <p className="text-danger">{error}</p>
          ) : !info ? (
            <LoadingRegion label={t.install.checking}>
              <Skeleton className="h-5 w-48" />
            </LoadingRegion>
          ) : (
            <>
              <p className={tooSmall ? "text-danger" : "text-fg-muted"}>
                {tooSmall ? (
                  <span className="flex items-center gap-2">
                    <CircleAlert aria-hidden className="shrink-0" />
                    {t.install.notEnough(formatBytes(needed.bytes), formatBytes(free))}
                  </span>
                ) : (
                  <>
                    {free !== null ? t.install.free(formatBytes(free)) : t.install.freeUnknown}
                    {needed && <> · {needed.estimate ? t.install.neededEstimate(formatBytes(needed.bytes)) : t.install.needed(formatBytes(needed.bytes))}</>}
                  </>
                )}
              </p>
              {!info.writable && (
                <p className="flex items-center gap-2 text-danger">
                  <CircleAlert aria-hidden className="shrink-0" />
                  {t.install.notWritable}
                </p>
              )}
              {info.needsAdmin && info.writable && (
                <p className="flex items-center gap-2 text-fg">
                  <TriangleAlert aria-hidden className="shrink-0" />
                  {t.install.programFiles}
                </p>
              )}
            </>
          )}
        </div>
        {folder !== defaultFolder && (
          <label htmlFor={checkboxId} className="mt-2 flex items-center gap-2 text-sm text-fg">
            <input
              id={checkboxId}
              type="checkbox"
              className="size-4 accent-accent"
              checked={makeDefault}
              onChange={(e) => {
                setMakeDefault(e.target.checked);
              }}
            />
            {t.install.makeDefault}
          </label>
        )}
      </div>

      <div className="flex justify-end gap-2">
        <Button onClick={onCancel}>{t.common.cancel}</Button>
        <Button
          variant="primary"
          disabled={blocked}
          onClick={() => {
            const target: InstallTarget = { folder, makeDefault: makeDefault && folder !== defaultFolder };
            if (base.dlcs && base.dlcs.length > 0) target.excludedDlcs = [...excluded];
            onInstall(target);
          }}
        >
          {t.install.install}
        </Button>
      </div>
    </div>
  );
}

/** A stand-in game for the cover placeholder (downloads without catalogue data). */
export function previewGame(title: string): InstallSubject["game"] {
  return {
    id: `install-${title}`,
    title,
    artwork: { cover: { url: null, alt: "" }, header: { url: null, alt: "" }, screenshots: [] },
  };
}
