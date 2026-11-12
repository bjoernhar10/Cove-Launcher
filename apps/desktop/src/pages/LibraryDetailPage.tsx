import { locale, useMessages } from "../i18n/index.ts";
import type { Achievement, CompatibilityTool, EnvVar, LaunchOptions, LibraryItem, ProtonVersionId } from "@cove/shared";
import { ArrowLeft, Compass, ExternalLink, FolderInput, PackageOpen, Library, Link2, Lock, PackageMinus, Plus, RefreshCw, Star, Trash2, Trophy } from "lucide-react";
import { useState, type SyntheticEvent, type ReactNode } from "react";
import { useNavigate, useParams, useSearchParams } from "react-router";
import { api, CoveError } from "../api/index.ts";
import { CollectionsButton } from "../features/CollectionMenu.tsx";
import { DEFAULT_OPTION, ProtonSelect, ProtonStatus } from "../features/ProtonPicker.tsx";
import { DlcDialog } from "../features/DlcDialog.tsx";
import { MatchDialog } from "../features/MatchDialog.tsx";
import { GameTweaks } from "../features/GameTweaks.tsx";
import { PrefixTools } from "../features/PrefixTools.tsx";
import { SaveBackups } from "../features/SaveBackups.tsx";
import { ProtonRating } from "../features/ProtonRating.tsx";
import { TimeToBeat } from "../features/TimeToBeat.tsx";
import { GameCover } from "../components/game/GameCover.tsx";
import { PlayButton } from "../components/game/PlayButton.tsx";
import { PlaytimeChart } from "../components/game/PlaytimeChart.tsx";
import { Button, ButtonLink } from "../components/ui/Button.tsx";
import { Dialog } from "../components/ui/Dialog.tsx";
import { Badge, EmptyState, ErrorState, LoadingRegion, ProgressBar, Skeleton } from "../components/ui/Feedback.tsx";
import { Field, Input, Select } from "../components/ui/Field.tsx";
import { Page } from "../components/ui/Page.tsx";
import { Segmented } from "../components/ui/Segmented.tsx";
import { Tabs } from "../components/ui/Tabs.tsx";
import { useQuery } from "../hooks/useQuery.ts";
import { cx } from "../lib/cx.ts";
import { detectPlatform } from "../lib/env.ts";
import { formatBytes, formatDate, formatPercent, formatPlaytime, formatRelative } from "../lib/format.ts";
import { COMPAT_LABELS, LAUNCH_VIA_LABELS, SOURCE_LABELS } from "../lib/labels.ts";
import { useMoves } from "../stores/moves.ts";
import { invalidateQueries } from "../stores/query.ts";
import { errorMessage, toast } from "../stores/toasts.ts";

type Tab = "overview" | "achievements" | "settings";
const TABS: Tab[] = ["overview", "achievements", "settings"];

function Stat({ label, children }: { label: string; children: ReactNode }) {
  return (
    <div className="rounded-lg border border-border px-4 py-3">
      <p className="text-xs text-fg-subtle">{label}</p>
      <p className="num mt-1 text-base font-medium text-fg">{children}</p>
    </div>
  );
}

function Overview({ item }: { item: LibraryItem }) {
  const t = useMessages();
  const stats = useQuery(`playtime/${item.game.id}`, () => api.library.getPlaytimeStats(item.game.id));
  const { entry, game } = item;

  return (
    <div className="flex flex-col gap-10">
      <div className="grid grid-cols-1 gap-10 xl:grid-cols-[1fr_320px]">
        <section aria-label={t.library.colPlaytime}>
          <h2 className="text-base font-semibold text-fg">{t.game.last14Days}</h2>
          {stats.status === "loading" && (
            <LoadingRegion label={t.game.loadingPlaytime}>
              <Skeleton className="mt-4 h-40" />
            </LoadingRegion>
          )}
          {stats.status === "error" && <ErrorState message={stats.error} onRetry={stats.retry} />}
          {stats.status === "success" && (
            <>
              <p className="num mt-1 text-sm text-fg-muted">
                {formatPlaytime(stats.data.last14DaysMinutes)} · {t.game.sessions(stats.data.sessionCount)}
              </p>
              <div className="mt-4">
                <PlaytimeChart days={stats.data.days} title={t.game.perDay(game.title)} />
              </div>
            </>
          )}
        </section>
        <div className="flex flex-col gap-8">
          {detectPlatform() === "linux" && <ProtonRating item={item} />}
          {entry.catalogId && <TimeToBeat catalogId={entry.catalogId} />}
          <section aria-label={t.game.info}>
            <h2 className="mb-3 text-base font-semibold text-fg">{t.game.info}</h2>
            <dl className="m-0 rounded-lg border border-border px-4 text-sm">
              {[
                [t.library.colSource, SOURCE_LABELS[entry.source]],
                [t.game.launchViaLabel, entry.launchVia ? LAUNCH_VIA_LABELS[entry.launchVia] : "—"],
                [t.game.status, entry.installed ? t.library.installed : t.library.notInstalled],
                [t.game.location, entry.installPath ?? "—"],
                [t.game.size, entry.installSizeBytes === null ? "—" : formatBytes(entry.installSizeBytes)],
                [t.game.added, formatDate(entry.addedAt)],
                [t.detail.developer, game.developer],
              ].map(([label, value]) => (
                <div key={label} className="flex justify-between gap-4 border-b border-border py-3 last:border-b-0">
                  <dt className="text-fg-subtle">{label}</dt>
                  <dd className="m-0 min-w-0 truncate text-right text-fg" title={value}>
                    {value}
                  </dd>
                </div>
              ))}
            </dl>
          </section>
        </div>
      </div>
      {entry.source !== "manual" && entry.storeGameId && <GameDlcs item={item} />}
    </div>
  );
}

type AchievementFilter = "all" | "unlocked" | "locked";

/** Store icon when there is one; a calm trophy/lock otherwise (or when it fails to load). */
function AchievementIcon({ achievement, unlocked }: { achievement: Achievement; unlocked: boolean }) {
  const [failed, setFailed] = useState(false);
  const url = achievement.icon.url;
  if (url && !failed) {
    return (
      <img
        src={url}
        alt=""
        loading="lazy"
        onError={() => {
          setFailed(true);
        }}
        className="size-10 shrink-0 rounded-md border border-border object-cover"
      />
    );
  }
  return (
    <div
      className={cx(
        "flex size-10 shrink-0 items-center justify-center rounded-md border border-border",
        unlocked ? "bg-accent-subtle text-accent-text" : "bg-sidebar text-fg-subtle",
      )}
    >
      {unlocked ? <Trophy aria-hidden /> : <Lock aria-hidden />}
    </div>
  );
}

function AchievementRow({ achievement }: { achievement: Achievement }) {
  const t = useMessages();
  const unlocked = achievement.unlockedAt !== null;
  const secret = achievement.hidden && !unlocked;
  return (
    <li className="flex items-center gap-4 border-b border-border py-3 last:border-b-0">
      <AchievementIcon achievement={achievement} unlocked={unlocked} />
      <div className="min-w-0 flex-1">
        <p className={cx("truncate text-sm font-medium", unlocked ? "text-fg" : "text-fg-muted")}>
          {secret ? t.game.hiddenAchievement : achievement.title}
        </p>
        <p className="truncate text-xs text-fg-subtle">{secret ? t.game.hiddenText : achievement.description}</p>
      </div>
      {achievement.rarityPercent !== null && (
        <span className="num hidden shrink-0 text-xs text-fg-subtle md:inline">
          {t.game.rarity(achievement.rarityPercent.toLocaleString(locale()))}
        </span>
      )}
      <span className="w-32 shrink-0 text-right text-xs text-fg-muted">
        {achievement.unlockedAt ? (
          <time dateTime={achievement.unlockedAt}>{formatRelative(achievement.unlockedAt)}</time>
        ) : (
          <span className="sr-only">{t.game.locked}</span>
        )}
      </span>
    </li>
  );
}

function Achievements({ item }: { item: LibraryItem }) {
  const t = useMessages();
  const query = useQuery(`achievements/${item.game.id}`, () => api.library.getAchievements(item.game.id));
  const [filter, setFilter] = useState<AchievementFilter>("all");

  if (query.status === "loading") {
    return (
      <LoadingRegion label={t.home.achievementsLoading}>
        {Array.from({ length: 6 }, (_, i) => (
          <div key={i} className="flex items-center gap-4 border-b border-border py-3">
            <Skeleton className="size-10" />
            <div className="flex-1">
              <Skeleton className="h-4 w-1/3" />
              <Skeleton className="mt-2 h-3 w-1/2" />
            </div>
          </div>
        ))}
      </LoadingRegion>
    );
  }
  if (query.status === "error") return <ErrorState message={query.error} onRetry={query.retry} />;
  if (query.data.length === 0) {
    return (
      <EmptyState
        compact
        icon={Trophy}
        title={t.achievements.none}
        description={t.game.noAchievementsText}
      />
    );
  }

  const unlocked = query.data.filter((a) => a.unlockedAt !== null);
  const locked = query.data.filter((a) => a.unlockedAt === null);
  const shown = filter === "unlocked" ? unlocked : filter === "locked" ? locked : [...unlocked, ...locked];

  return (
    <div>
      <div className="mb-4 flex flex-wrap items-center justify-between gap-4">
        <div className="w-64">
          <p className="num text-sm text-fg-muted">
            {t.game.unlockedOf(unlocked.length, query.data.length)}
          </p>
          <ProgressBar value={unlocked.length} max={query.data.length} label="Achievement-Fortschritt" className="mt-2" />
        </div>
        <Segmented
          label={t.game.filterAchievements}
          value={filter}
          onChange={setFilter}
          options={[
            { value: "all", label: t.library.groups.all },
            { value: "unlocked", label: t.home.unlocked },
            { value: "locked", label: t.game.locked },
          ]}
        />
      </div>
      {shown.length === 0 ? (
        <p className="py-8 text-center text-sm text-fg-muted">
          {filter === "locked" ? t.game.allUnlocked : t.game.noneUnlocked}
        </p>
      ) : (
        <ul className="rounded-lg border border-border px-4">
          {shown.map((a) => (
            <AchievementRow key={a.id} achievement={a} />
          ))}
        </ul>
      )}
    </div>
  );
}

const COMPAT_TOOLS = Object.keys(COMPAT_LABELS) as CompatibilityTool[];

/** The folder that holds `path` ("/a/b/Game" → "/a/b"). */
function parentFolder(path: string): string {
  const cut = Math.max(path.lastIndexOf("/"), path.lastIndexOf("\\"));
  return cut > 0 ? path.slice(0, cut) : path;
}

const OTHER_FOLDER = "\u0000other";

/** "Spiel verschieben": into another games folder, with progress. */
function MoveGame({ item }: { item: LibraryItem }) {
  const t = useMessages();
  const settings = useQuery("settings", api.settings.getSettings);
  const move = useMoves((s) => s.moves[item.game.id]);
  const start = useMoves((s) => s.start);
  const [picked, setPicked] = useState<string | null>(null);
  const [chosen, setChosen] = useState<string | null>(null);
  const current = parentFolder(item.entry.installPath ?? "");
  const folders = [
    ...new Set([...(settings.status === "success" ? settings.data.library.gameFolders : []), ...(picked ? [picked] : [])]),
  ].filter((f) => f !== current);
  const target = chosen ?? folders[0] ?? null;

  async function pick(): Promise<void> {
    try {
      const folder = await api.system.pickFolder(current);
      if (!folder) return;
      setPicked(folder);
      setChosen(folder);
    } catch (error: unknown) {
      toast(errorMessage(error), "danger");
    }
  }

  async function run(): Promise<void> {
    if (!target) return;
    try {
      await start(item.game.id, item.game.title, target);
    } catch (error: unknown) {
      toast(errorMessage(error), "danger");
    }
  }

  return (
    <section aria-labelledby={`move-${item.game.id}`} className="max-w-2xl border-t border-border pt-6">
      <h3 id={`move-${item.game.id}`} className="text-sm font-medium text-fg">
        {t.game.moveTitle}
      </h3>
      <p className="mt-1 text-sm text-fg-muted">{t.game.moveText}</p>
      {move ? (
        <div className="mt-4" aria-live="polite">
          <ProgressBar
            value={move.totalBytes > 0 ? move.copiedBytes : 0}
            max={Math.max(move.totalBytes, 1)}
            label={t.game.moveTitle}
          />
          <p className="num mt-2 text-xs text-fg-muted">
            {move.status === "verifying"
              ? t.game.verifyingMove
              : t.game.moving(move.totalBytes > 0 ? formatPercent(move.copiedBytes / move.totalBytes) : "")}
          </p>
        </div>
      ) : (
        <div className="mt-4 flex flex-wrap items-center gap-2">
          <Select
            className="min-w-64 flex-1"
            aria-label={t.game.moveTo}
            value={target ?? OTHER_FOLDER}
            onChange={(value) => {
              if (value === OTHER_FOLDER) void pick();
              else setChosen(value);
            }}
            options={[...folders.map((f) => ({ value: f, label: f })), { value: OTHER_FOLDER, label: t.game.otherFolder }]}
          />
          <Button icon={FolderInput} disabled={!target} onClick={() => void run()}>
            {t.game.move}
          </Button>
        </div>
      )}
    </section>
  );
}

/** A GOG or Epic game Cove installed itself (updates, moving). */
function ownStoreInstall(item: LibraryItem): boolean {
  return (item.entry.source === "gog" || item.entry.source === "epic") && item.entry.installedByCove;
}

/** Store games Cove installed: installed vs. newest build, "Aktualisieren". */
function GameUpdates({ item }: { item: LibraryItem }) {
  const t = useMessages();
  const update = useQuery(`update/${item.game.id}`, () => api.library.checkGameUpdate(item.game.id));
  const [busy, setBusy] = useState(false);

  async function run(): Promise<void> {
    setBusy(true);
    try {
      await api.library.installGameUpdate(item.game.id);
      toast(t.game.updateQueued(item.game.title));
      invalidateQueries();
    } catch (error: unknown) {
      toast(errorMessage(error), "danger");
    } finally {
      setBusy(false);
    }
  }

  return (
    <section aria-labelledby={`updates-${item.game.id}`} className="max-w-2xl border-t border-border pt-6">
      <h3 id={`updates-${item.game.id}`} className="text-sm font-medium text-fg">
        {t.game.updatesTitle}
      </h3>
      {update.status === "loading" && (
        <LoadingRegion label={t.game.checkingUpdate}>
          <Skeleton className="mt-2 h-5 w-64" />
        </LoadingRegion>
      )}
      {update.status === "error" && <ErrorState message={update.error} onRetry={update.retry} />}
      {update.status === "success" && (
        <>
          <p className="num mt-1 text-sm text-fg-muted">
            {t.game.installedVersion(update.data.installedVersion)} ·{" "}
            {update.data.available ? t.game.updateAvailable(update.data.latestVersion) : t.game.upToDate}
          </p>
          {update.data.available && (
            <Button variant="primary" icon={RefreshCw} className="mt-4" disabled={busy} onClick={() => void run()}>
              {t.game.updateNow}
            </Button>
          )}
        </>
      )}
    </section>
  );
}

/**
 * DLCs of a store game. Steam: every DLC (installed ones marked) and "In Steam verwalten".
 * GOG/Epic: the DLCs the account owns; "DLCs ändern" for games Cove installed.
 */
function GameDlcs({ item }: { item: LibraryItem }) {
  const t = useMessages();
  const dlcs = useQuery(`dlcs/${item.game.id}`, () => api.library.gameDlcs(item.game.id));
  const [busy, setBusy] = useState(false);
  const [changing, setChanging] = useState(false);
  const steam = item.entry.source === "steam";
  const changeable = item.entry.installed && ownStoreInstall(item);

  async function manage(): Promise<void> {
    setBusy(true);
    try {
      await api.library.manageDlcs(item.game.id);
      toast(t.game.dlcsOpened);
    } catch (error: unknown) {
      toast(errorMessage(error), "danger");
    } finally {
      setBusy(false);
    }
  }

  const hint = steam
    ? t.game.dlcsSteamHint
    : changeable
      ? t.game.dlcsStoreHint
      : item.entry.installed
        ? t.game.dlcsLauncherHint
        : t.game.dlcsInstallHint;

  return (
    <section aria-labelledby={`dlcs-${item.game.id}`} className="max-w-2xl">
      <h2 id={`dlcs-${item.game.id}`} className="text-base font-semibold text-fg">
        {t.game.dlcsTitle}
      </h2>
      {dlcs.status === "loading" && (
        <LoadingRegion label={t.game.dlcsLoading} className="mt-2 flex flex-col gap-2">
          <Skeleton className="h-5 w-64" />
          <Skeleton className="h-5 w-48" />
        </LoadingRegion>
      )}
      {dlcs.status === "error" && <ErrorState message={dlcs.error} onRetry={dlcs.retry} />}
      {dlcs.status === "success" && dlcs.data.dlcs.length === 0 && (
        <p className="mt-1 text-sm text-fg-muted">{steam ? t.game.dlcsNone : t.game.dlcsNoneOwned}</p>
      )}
      {dlcs.status === "success" && dlcs.data.dlcs.length > 0 && (
        <>
          <p className="mt-1 text-sm text-fg-muted">{hint}</p>
          <ul className="mt-3 max-h-64 overflow-y-auto rounded-lg border border-border">
            {dlcs.data.dlcs.map((dlc) => (
              <li key={dlc.id} className="flex items-center gap-3 border-b border-border px-3 py-2 text-sm text-fg last:border-b-0">
                <span className="min-w-0 flex-1 truncate">{dlc.title}</span>
                {dlc.unlockOnly === true ? <Badge>{t.game.dlcUnlockOnly}</Badge> : dlc.installed && <Badge>{t.game.dlcInstalled}</Badge>}
              </li>
            ))}
          </ul>
          {dlcs.data.total > dlcs.data.dlcs.length && (
            <p className="num mt-2 text-xs text-fg-subtle">{t.game.dlcsCut(dlcs.data.dlcs.length, dlcs.data.total)}</p>
          )}
          {steam && (
            <Button icon={ExternalLink} className="mt-4" disabled={busy} onClick={() => void manage()}>
              {t.game.manageInSteam}
            </Button>
          )}
        </>
      )}
      {changeable && (
        <Button
          icon={PackageOpen}
          className="mt-4"
          onClick={() => {
            setChanging(true);
          }}
        >
          {t.game.changeDlcs}
        </Button>
      )}
      {changing && (
        <DlcDialog
          item={item}
          open
          onClose={() => {
            setChanging(false);
            dlcs.retry();
          }}
        />
      )}
    </section>
  );
}

/** Steam: Steam's own dialog. Games Cove installed: deletes the folder after a confirmation. */
function Uninstall({ item }: { item: LibraryItem }) {
  const t = useMessages();
  const [confirming, setConfirming] = useState(false);
  const [busy, setBusy] = useState(false);
  const steam = item.entry.source === "steam";

  async function uninstall(): Promise<void> {
    setBusy(true);
    try {
      const method = await api.library.uninstallGame(item.game.id);
      toast(method === "steam" ? t.game.uninstallInSteam(item.game.title) : t.game.uninstalled(item.game.title));
      invalidateQueries();
    } catch (error: unknown) {
      toast(errorMessage(error), "danger");
    } finally {
      setBusy(false);
      setConfirming(false);
    }
  }

  return (
    <section aria-labelledby={`uninstall-${item.game.id}`} className="max-w-2xl border-t border-border pt-6">
      <h3 id={`uninstall-${item.game.id}`} className="text-sm font-medium text-fg">
        {t.game.uninstallTitle}
      </h3>
      <p className="mt-1 text-sm text-fg-muted">{steam ? t.game.uninstallSteamText : t.game.uninstallText}</p>
      <Button
        icon={PackageMinus}
        className="mt-4"
        disabled={busy}
        onClick={() => {
          if (steam) void uninstall();
          else setConfirming(true);
        }}
      >
        {t.game.uninstall}
      </Button>
      <Dialog
        open={confirming}
        onClose={() => {
          setConfirming(false);
        }}
        title={t.game.uninstallConfirm(item.game.title)}
        description={t.game.uninstallConfirmText(item.entry.installPath ?? "")}
        footer={
          <>
            <Button
              onClick={() => {
                setConfirming(false);
              }}
            >
              {t.common.cancel}
            </Button>
            <Button variant="danger" disabled={busy} onClick={() => void uninstall()}>
              {t.game.uninstall}
            </Button>
          </>
        }
      >
        {null}
      </Dialog>
    </section>
  );
}

function RemoveFromLibrary({ item }: { item: LibraryItem }) {
  const t = useMessages();
  const navigate = useNavigate();
  const [confirming, setConfirming] = useState(false);
  const [busy, setBusy] = useState(false);

  async function remove(): Promise<void> {
    setBusy(true);
    try {
      await api.library.removeGame(item.game.id);
      toast(t.game.removed(item.game.title));
      invalidateQueries();
      void navigate("/library");
    } catch (error: unknown) {
      toast(errorMessage(error), "danger");
      setBusy(false);
      setConfirming(false);
    }
  }

  return (
    <section aria-label="Entfernen" className="max-w-2xl border-t border-border pt-6">
      <h3 className="text-sm font-medium text-fg">{t.game.removeTitle}</h3>
      <p className="mt-1 text-sm text-fg-muted">
        {t.game.removeText}
      </p>
      <Button
        variant="danger"
        icon={Trash2}
        className="mt-4"
        onClick={() => {
          setConfirming(true);
        }}
      >
        {t.game.remove}
      </Button>
      <Dialog
        open={confirming}
        onClose={() => {
          setConfirming(false);
        }}
        title={t.game.removeConfirm(item.game.title)}
        description={t.game.removeConfirmText}
        footer={
          <>
            <Button
              onClick={() => {
                setConfirming(false);
              }}
            >
              {t.common.cancel}
            </Button>
            <Button variant="danger" disabled={busy} onClick={() => void remove()}>
              {t.game.remove}
            </Button>
          </>
        }
      >
        {null}
      </Dialog>
    </section>
  );
}

/** Linux: which Proton build the game starts with (null = the default from the settings). */
function ProtonField({
  value,
  pending,
  onChange,
}: {
  value: ProtonVersionId | null;
  pending: boolean;
  onChange: (value: ProtonVersionId | null) => void;
}) {
  const t = useMessages();
  const overview = useQuery("proton", api.proton.overview);
  const settings = useQuery("settings", api.settings.getSettings);
  if (overview.status === "error") {
    return (
      <div className="flex flex-col gap-2">
        <p className="text-sm font-medium text-fg">{t.proton.version}</p>
        <p className="text-sm text-fg-muted">{overview.error}</p>
        <div>
          <Button size="sm" onClick={overview.retry}>
            {t.common.retry}
          </Button>
        </div>
      </div>
    );
  }
  if (overview.status !== "success" || settings.status !== "success") {
    return (
      <LoadingRegion label={t.proton.loading} className="flex flex-col gap-2">
        <Skeleton className="h-4 w-32" />
        <Skeleton className="h-9 w-full" />
      </LoadingRegion>
    );
  }
  const defaultId = settings.data.library.defaultProton;
  const effective = value ?? defaultId;
  return (
    <div className="flex flex-col gap-2">
      <Field label={t.proton.version} description={t.proton.versionHint}>
        {(props) => (
          <ProtonSelect
            {...props}
            overview={overview.data}
            defaultId={defaultId}
            value={value ?? DEFAULT_OPTION}
            onChange={(next) => {
              onChange(next === DEFAULT_OPTION ? null : next);
            }}
          />
        )}
      </Field>
      <ProtonStatus id={effective} overview={overview.data} pending={pending} />
    </div>
  );
}

function LaunchSettings({ item }: { item: LibraryItem }) {
  const t = useMessages();
  const [draft, setDraft] = useState<LaunchOptions>(item.entry.launchOptions);
  const [executable, setExecutable] = useState(item.entry.executablePath ?? "");
  const [saving, setSaving] = useState(false);
  const [pathError, setPathError] = useState<string | null>(null);
  const isLinux = detectPlatform() === "linux";
  const executableChanged = executable.trim() !== (item.entry.executablePath ?? "");
  const usesExecutable = item.entry.source !== "steam";
  const dirty = JSON.stringify(draft) !== JSON.stringify(item.entry.launchOptions) || executableChanged;

  function setEnv(index: number, patch: Partial<EnvVar>): void {
    setDraft((d) => ({ ...d, env: d.env.map((e, i) => (i === index ? { ...e, ...patch } : e)) }));
  }

  async function save(event: SyntheticEvent): Promise<void> {
    event.preventDefault();
    setSaving(true);
    try {
      const cleaned = { ...draft, env: draft.env.filter((e) => e.key.trim() !== "") };
      setPathError(null);
      await api.library.updateGame(item.game.id, {
        launchOptions: cleaned,
        ...(executableChanged && executable.trim() ? { executablePath: executable.trim() } : {}),
      });
      setDraft(cleaned);
      toast(t.game.launchSaved);
      invalidateQueries();
    } catch (error: unknown) {
      if (error instanceof CoveError && error.kind === "invalidInput") setPathError(error.message);
      else toast(errorMessage(error), "danger");
    } finally {
      setSaving(false);
    }
  }

  return (
    <div className="flex flex-col gap-10">
    <form className="flex max-w-2xl flex-col gap-8" onSubmit={(e) => void save(e)}>
      {usesExecutable && (
        <Field
          label={t.game.executable}
          description={
            item.entry.executablePath
              ? t.game.executableHint
              : t.game.executableMissing
          }
          error={pathError}
        >
          {(props) => (
            <Input
              {...props}
              spellCheck={false}
              placeholder={isLinux ? t.game.linuxPathExample : t.game.windowsPathExample}
              value={executable}
              onChange={(e) => {
                setExecutable(e.target.value);
                setPathError(null);
              }}
            />
          )}
        </Field>
      )}

      <Field label={t.game.arguments} description={t.game.argumentsHint}>
        {(props) => (
          <Input
            {...props}
            spellCheck={false}
            placeholder="-fullscreen"
            value={draft.arguments}
            onChange={(e) => {
              setDraft((d) => ({ ...d, arguments: e.target.value }));
            }}
          />
        )}
      </Field>

      {isLinux && (
        <Field label={t.game.compat} description={t.game.compatHint}>
          {(props) => (
            <Select
              {...props}
              value={draft.compatibilityTool}
              onChange={(compatibilityTool) => {
                setDraft((d) => ({ ...d, compatibilityTool }));
              }}
              options={COMPAT_TOOLS.map((value) => ({ value, label: COMPAT_LABELS[value] }))}
            />
          )}
        </Field>
      )}

      {isLinux && draft.compatibilityTool === "proton" && (
        <ProtonField
          value={draft.protonVersion}
          // Saving installs a missing build; until then it "will be downloaded".
          pending={
            draft.protonVersion !== item.entry.launchOptions.protonVersion ||
            item.entry.launchOptions.compatibilityTool !== "proton"
          }
          onChange={(protonVersion) => {
            setDraft((d) => ({ ...d, protonVersion }));
          }}
        />
      )}

      {isLinux && item.entry.launchVia === "direct" && (
        <GameTweaks
          value={draft.tweaks}
          proton={draft.compatibilityTool === "proton" && executable.trim().toLowerCase().endsWith(".exe")}
          onChange={(tweaks) => {
            setDraft((d) => ({ ...d, tweaks }));
          }}
        />
      )}

      <fieldset className="m-0 flex flex-col gap-2 border-0 p-0">
        <legend className="text-sm font-medium text-fg">{t.game.env}</legend>
        <p className="text-xs text-fg-muted">{t.game.envHint}</p>
        {draft.env.length > 0 && (
          <ul className="mt-1 flex flex-col gap-2">
            {draft.env.map((env, index) => (
              <li key={index} className="flex items-center gap-2">
                <Input
                  aria-label={t.game.envName(index + 1)}
                  placeholder="NAME"
                  spellCheck={false}
                  className="w-56 font-medium"
                  value={env.key}
                  onChange={(e) => {
                    setEnv(index, { key: e.target.value });
                  }}
                />
                <span className="text-fg-subtle" aria-hidden>
                  =
                </span>
                <Input
                  aria-label={t.game.envValue(index + 1)}
                  placeholder={t.game.value}
                  spellCheck={false}
                  value={env.value}
                  onChange={(e) => {
                    setEnv(index, { value: e.target.value });
                  }}
                />
                <Button
                  variant="ghost"
                  iconOnly
                  icon={Trash2}
                  label={t.game.envRemove(env.key || String(index + 1))}
                  onClick={() => {
                    setDraft((d) => ({ ...d, env: d.env.filter((_, i) => i !== index) }));
                  }}
                />
              </li>
            ))}
          </ul>
        )}
        <div className="mt-1">
          <Button
            size="sm"
            icon={Plus}
            onClick={() => {
              setDraft((d) => ({ ...d, env: [...d.env, { key: "", value: "" }] }));
            }}
          >
            {t.game.envAdd}
          </Button>
        </div>
      </fieldset>

      <div className="flex gap-2 border-t border-border pt-6">
        <Button type="submit" variant="primary" disabled={!dirty || saving}>
          {t.common.save}
        </Button>
        <Button
          disabled={!dirty || saving}
          onClick={() => {
            setDraft(item.entry.launchOptions);
            setExecutable(item.entry.executablePath ?? "");
            setPathError(null);
          }}
        >
          {t.game.discard}
        </Button>
      </div>
    </form>
    <SaveBackups item={item} />
    {isLinux && item.entry.launchOptions.compatibilityTool === "proton" && <PrefixTools item={item} />}
    {item.entry.installed && ownStoreInstall(item) && <GameUpdates item={item} />}
    {item.entry.installed && (item.entry.source === "manual" || ownStoreInstall(item)) && item.entry.installPath && (
      <MoveGame item={item} />
    )}
    {item.entry.installed && (item.entry.source === "steam" || item.entry.installedByCove) && <Uninstall item={item} />}
    <RemoveFromLibrary item={item} />
    </div>
  );
}

function HeaderSkeleton() {
  const t = useMessages();
  return (
    <LoadingRegion label={t.detail.loading} className="flex gap-8">
      <Skeleton className="aspect-[3/4] w-44 rounded-lg" />
      <div className="flex-1 pt-2">
        <Skeleton className="h-9 w-1/3" />
        <Skeleton className="mt-3 h-4 w-1/4" />
        <Skeleton className="mt-8 h-9 w-32" />
      </div>
    </LoadingRegion>
  );
}

export function LibraryDetailPage() {
  const t = useMessages();
  const { gameId = "" } = useParams();
  const [params, setParams] = useSearchParams();
  const query = useQuery(`library/${gameId}`, () => api.library.getLibraryItem(gameId));
  const tabParam = params.get("tab");
  const tab: Tab = TABS.find((x) => x === tabParam) ?? "overview";
  const [matchOpen, setMatchOpen] = useState(false);

  async function toggleFavorite(item: LibraryItem): Promise<void> {
    const favorite = !item.entry.favorite;
    query.update((current) => (current ? { ...current, entry: { ...current.entry, favorite } } : current));
    try {
      await api.library.setFavorite(gameId, favorite);
      invalidateQueries();
    } catch (error: unknown) {
      toast(errorMessage(error), "danger");
    }
  }

  return (
    <Page>
      <ButtonLink to="/library" variant="ghost" size="sm" icon={ArrowLeft} className="-ml-3 mb-4">
        My Library
      </ButtonLink>

      {query.status === "loading" && <HeaderSkeleton />}
      {query.status === "error" && <ErrorState message={query.error} onRetry={query.retry} />}
      {query.status === "success" && query.data === null && (
        <EmptyState
          icon={Library}
          title={t.game.notInLibrary}
          description={t.game.notInLibraryText}
          actions={
            <>
              <ButtonLink to="/library">{t.downloads.toLibrary}</ButtonLink>
              <ButtonLink to={`/catalogue/${gameId}`} icon={Compass}>
                {t.game.viewInCatalogue}
              </ButtonLink>
            </>
          }
        />
      )}
      {query.status === "success" && query.data !== null && (
        <>
          <header className="flex flex-col gap-8 sm:flex-row">
            <GameCover game={query.data.game} className="w-44 shrink-0 border border-border" titleSize="sm" />
            <div className="flex min-w-0 flex-1 flex-col justify-end pb-1">
              <div className="flex items-center gap-2">
                <Badge>{SOURCE_LABELS[query.data.entry.source]}</Badge>
                {query.data.entry.launchVia && <Badge>{LAUNCH_VIA_LABELS[query.data.entry.launchVia]}</Badge>}
                {!query.data.entry.installed && <Badge>{t.library.notInstalled}</Badge>}
              </div>
              <h1 className="mt-3 text-xl font-semibold tracking-tight text-fg">{query.data.game.title}</h1>
              <p className="mt-1 text-sm text-fg-muted">{query.data.game.summary}</p>
              <div className="mt-6 flex flex-wrap items-center gap-2">
                <PlayButton item={query.data} />
                <Button
                  aria-pressed={query.data.entry.favorite}
                  icon={Star}
                  onClick={() => {
                    if (query.data) void toggleFavorite(query.data);
                  }}
                  className={query.data.entry.favorite ? "[&_svg]:fill-current [&_svg]:text-accent-text" : undefined}
                >
                  {query.data.entry.favorite ? t.game.favorite : t.library.favorite}
                </Button>
                <CollectionsButton game={query.data.game} />
                {query.data.entry.catalogId && (
                  <ButtonLink to={`/catalogue/${query.data.entry.catalogId}`} variant="ghost">
                    {t.game.cataloguePage}
                  </ButtonLink>
                )}
                <Button
                  variant="ghost"
                  icon={Link2}
                  onClick={() => {
                    setMatchOpen(true);
                  }}
                >
                  {query.data.entry.catalogId ? t.game.changeMatch : t.game.matchInCatalogue}
                </Button>
              </div>
            </div>
          </header>

          {query.data.entry.matchStatus === "pending" && (
            <div
              role="status"
              className="mt-8 flex flex-wrap items-center justify-between gap-4 rounded-lg border border-border bg-accent-subtle px-4 py-3"
            >
              <p className="text-sm text-fg">
                {t.game.pendingMatch}
              </p>
              <Button
                size="sm"
                variant="primary"
                onClick={() => {
                  setMatchOpen(true);
                }}
              >
                {t.library.checkMatch}
              </Button>
            </div>
          )}
          <MatchDialog
            key={`${query.data.game.id}-${String(matchOpen)}`}
            item={query.data}
            open={matchOpen}
            onClose={() => {
              setMatchOpen(false);
            }}
          />

          <div className="mt-8 grid grid-cols-2 gap-3 lg:grid-cols-4">
            <Stat label={t.game.totalPlaytime}>{formatPlaytime(query.data.entry.playtimeMinutes)}</Stat>
            <Stat label={t.library.colLastPlayed}>{query.data.entry.lastPlayedAt ? formatRelative(query.data.entry.lastPlayedAt) : t.game.never}</Stat>
            <Stat label="Achievements">
              {query.data.entry.achievements.total > 0
                ? `${String(query.data.entry.achievements.unlocked)} / ${String(query.data.entry.achievements.total)}`
                : "—"}
            </Stat>
            <Stat label={t.game.size}>{query.data.entry.installSizeBytes === null ? "—" : formatBytes(query.data.entry.installSizeBytes)}</Stat>
          </div>

          <div className="mt-10">
            <Tabs
              label={t.game.sections}
              value={tab}
              onChange={(next) => {
                setParams(next === "overview" ? {} : { tab: next }, { replace: true });
              }}
              items={[
                { value: "overview", label: t.game.overview },
                {
                  value: "achievements",
                  label: "Achievements",
                  ...(query.data.entry.achievements.total > 0
                    ? { meta: `${String(query.data.entry.achievements.unlocked)}/${String(query.data.entry.achievements.total)}` }
                    : {}),
                },
                { value: "settings", label: t.game.settings },
              ]}
            >
              {tab === "overview" && <Overview item={query.data} />}
              {tab === "achievements" && <Achievements item={query.data} />}
              {tab === "settings" && <LaunchSettings key={query.data.game.id} item={query.data} />}
            </Tabs>
          </div>
        </>
      )}
    </Page>
  );
}
