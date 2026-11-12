import type { AppSettings, ProtonOverview, ProtonVersion } from "@cove/shared";
import { Cpu, Download, RefreshCw, Trash2 } from "lucide-react";
import { useState } from "react";
import { api } from "../../api/index.ts";
import { UMU_ID } from "../../api/proton.ts";
import { Button } from "../../components/ui/Button.tsx";
import { Badge, EmptyState, ErrorState, LoadingRegion, ProgressBar, Skeleton } from "../../components/ui/Feedback.tsx";
import { Field, SettingRow, Switch } from "../../components/ui/Field.tsx";
import { CancelInstall, progressText, ProtonSelect, ProtonStatus } from "../../features/ProtonPicker.tsx";
import { useQuery } from "../../hooks/useQuery.ts";
import { useMessages } from "../../i18n/index.ts";
import { formatBytes, formatDate, formatPercent, formatRelative } from "../../lib/format.ts";
import { useProtonInstall } from "../../stores/proton.ts";
import { invalidateQueries } from "../../stores/query.ts";
import { errorMessage, toast } from "../../stores/toasts.ts";
import { Panel } from "./parts.tsx";

/** Not installed GE-Proton releases shown before "Alle … zeigen". */
const NEWEST_SHOWN = 5;

function VersionRow({ version, overview }: { version: ProtonVersion; overview: ProtonOverview }) {
  const t = useMessages();
  const install = useProtonInstall(version.id, overview);
  const [busy, setBusy] = useState(false);

  async function run(action: () => Promise<void>): Promise<void> {
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

  const meta = [
    version.releasedAt ? t.proton.released(formatDate(version.releasedAt)) : null,
    !version.installed && version.downloadBytes !== null ? formatBytes(version.downloadBytes) : null,
    version.games > 0 ? t.proton.usedBy(version.games) : null,
  ].filter((part) => part !== null);

  return (
    <li className="flex items-center gap-4 border-b border-border py-3 pr-2 pl-4 last:border-b-0">
      <div className="min-w-0 flex-1">
        <div className="flex items-center gap-2">
          <span className="truncate text-sm font-medium text-fg">{version.name}</span>
          {version.id === overview.latest && <Badge>{t.proton.newest}</Badge>}
        </div>
        {install ? (
          <div className="mt-2 flex items-center gap-3">
            <ProgressBar
              value={install.downloadedBytes}
              max={install.totalBytes || 1}
              label={t.proton.installing(version.name)}
              className="max-w-48 flex-1"
            />
            <span className="num text-xs text-fg-muted">
              {install.phase === "downloading"
                ? formatPercent(install.totalBytes > 0 ? install.downloadedBytes / install.totalBytes : 0)
                : install.phase === "verifying"
                  ? t.proton.verifying
                  : t.proton.unpacking}
            </span>
            <CancelInstall id={version.id} />
          </div>
        ) : (
          meta.length > 0 && <p className="num mt-1 truncate text-xs text-fg-muted">{meta.join(" · ")}</p>
        )}
      </div>
      {version.kind === "ge" && !version.installed && !install && (
        <Button
          size="sm"
          icon={Download}
          aria-label={t.proton.installTitle(version.name)}
          disabled={busy}
          onClick={() =>
            void run(async () => {
              await api.proton.install(version.id);
            })
          }
        >
          {t.proton.install}
        </Button>
      )}
      {version.installed && version.kind === "ge" && <Badge>{t.proton.installedBadge}</Badge>}
      {version.removable && (
        <Button
          variant="ghost"
          size="sm"
          iconOnly
          icon={Trash2}
          label={t.proton.remove(version.name)}
          disabled={busy}
          onClick={() =>
            void run(async () => {
              await api.proton.remove(version.id);
              toast(t.proton.removed(version.name));
            })
          }
        />
      )}
    </li>
  );
}

function DefaultVersion({ overview }: { overview: ProtonOverview }) {
  const t = useMessages();
  const settings = useQuery("settings", api.settings.getSettings);
  if (settings.status === "error") return <ErrorState message={settings.error} onRetry={settings.retry} />;
  if (settings.status !== "success") return <Skeleton className="h-16" />;
  const current = settings.data.library.defaultProton;

  function choose(defaultProton: string, before: AppSettings): void {
    const next = { ...before, library: { ...before.library, defaultProton } };
    settings.update(() => next);
    api.settings.saveSettings(next).then(
      () => {
        invalidateQueries();
      },
      (error: unknown) => {
        toast(errorMessage(error), "danger");
        invalidateQueries();
      },
    );
  }

  return (
    <div className="flex max-w-xl flex-col gap-2">
      <Field label={t.proton.defaultVersion} description={t.proton.defaultVersionHint}>
        {(props) => (
          <ProtonSelect
            {...props}
            overview={overview}
            value={current}
            onChange={(next) => {
              choose(next, settings.data);
            }}
          />
        )}
      </Field>
      <ProtonStatus id={current} overview={overview} pending={false} />
      <p className="text-xs text-fg-muted">{t.proton.latestHint}</p>
    </div>
  );
}

/** umu-launcher: on/off, where it comes from, and its install. */
function UmuSection({ overview }: { overview: ProtonOverview }) {
  const t = useMessages();
  const settings = useQuery("settings", api.settings.getSettings);
  const install = useProtonInstall(UMU_ID, overview);
  const [busy, setBusy] = useState(false);
  if (settings.status === "error") return <ErrorState message={settings.error} onRetry={settings.retry} />;
  if (settings.status !== "success") return <Skeleton className="h-16" />;
  const umu = overview.umu;

  function toggle(useUmu: boolean, before: AppSettings): void {
    const next = { ...before, library: { ...before.library, useUmu } };
    settings.update(() => next);
    api.settings.saveSettings(next).then(
      () => {
        invalidateQueries();
      },
      (error: unknown) => {
        toast(errorMessage(error), "danger");
        invalidateQueries();
      },
    );
  }

  async function installNow(): Promise<void> {
    setBusy(true);
    try {
      await api.proton.install(UMU_ID);
    } catch (error: unknown) {
      toast(errorMessage(error), "danger");
    } finally {
      setBusy(false);
    }
  }

  let status: string;
  if (!umu.enabled) status = t.umu.off;
  else if (umu.source === "system") status = t.umu.system;
  else if (umu.source === "cove" && umu.version) status = t.umu.installed(umu.version);
  else if (!umu.python) status = t.umu.noPython;
  else status = t.umu.missing;

  return (
    <div className="mt-10 max-w-xl">
      <SettingRow title={t.umu.title} description={t.umu.hint}>
        <Switch
          label={t.umu.title}
          checked={umu.enabled}
          onChange={(useUmu) => {
            toggle(useUmu, settings.data);
          }}
        />
      </SettingRow>
      {install ? (
        <div className="flex items-center gap-2">
          <div role="status" className="flex min-w-0 flex-1 flex-col gap-2">
            <p className="text-xs text-fg-muted">{progressText(install, t)}</p>
            <ProgressBar value={install.downloadedBytes} max={install.totalBytes || 1} label={t.proton.installing("umu-launcher")} />
          </div>
          <CancelInstall id={UMU_ID} />
        </div>
      ) : (
        <div className="flex flex-wrap items-center gap-3">
          <p role="status" className="text-xs text-fg-muted">
            {status}
          </p>
          {umu.enabled && umu.source === null && umu.python && (
            <Button size="sm" icon={Download} disabled={busy} onClick={() => void installNow()}>
              {t.umu.installNow}
            </Button>
          )}
        </div>
      )}
      {umu.enabled && <p className="mt-2 text-xs text-fg-muted">{t.umu.runtimeHint}</p>}
    </div>
  );
}

/** Settings → Proton (Linux): default build, umu-launcher, GE-Proton downloads and Steam's builds. */
export function ProtonTab() {
  const t = useMessages();
  const query = useQuery("proton", api.proton.overview);
  const [showAll, setShowAll] = useState(false);
  const [checking, setChecking] = useState(false);

  async function check(): Promise<void> {
    setChecking(true);
    try {
      const fresh = await api.proton.checkUpdates();
      query.update(() => fresh);
      if (!fresh.offline) toast(t.proton.upToDate);
    } catch (error: unknown) {
      toast(errorMessage(error), "danger");
    } finally {
      setChecking(false);
    }
  }

  if (query.status === "error") return <ErrorState message={query.error} onRetry={query.retry} />;
  if (query.status !== "success") {
    return (
      <LoadingRegion label={t.proton.loading} className="flex flex-col gap-4">
        <Skeleton className="h-6 w-40" />
        <Skeleton className="h-16" />
        <Skeleton className="h-40" />
      </LoadingRegion>
    );
  }
  const overview = query.data;
  const ge = overview.versions.filter((v) => v.kind === "ge");
  const steam = overview.versions.filter((v) => v.kind === "steam");
  // Installed builds always; of the others only the newest few.
  const newest = new Set(ge.filter((v) => !v.installed).slice(0, NEWEST_SHOWN).map((v) => v.id));
  const shown = showAll ? ge : ge.filter((v) => v.installed || newest.has(v.id) || v.id === overview.latest);
  const hidden = ge.length - shown.length;

  return (
    <Panel title={t.proton.title} description={t.proton.hint}>
      {overview.versions.length === 0 ? (
        <div className="rounded-lg border border-border">
          <EmptyState
            compact
            icon={Cpu}
            title={t.proton.emptyTitle}
            description={t.proton.emptyText}
            actions={
              <Button icon={RefreshCw} disabled={checking} onClick={() => void check()}>
                {checking ? t.proton.checking : t.proton.check}
              </Button>
            }
          />
        </div>
      ) : (
        <>
          <DefaultVersion overview={overview} />
          <UmuSection overview={overview} />

          <div className="mt-10 mb-3 flex flex-wrap items-end justify-between gap-4">
            <div>
              <h3 className="text-sm font-medium text-fg">{t.proton.versions}</h3>
              <p className="mt-1 text-sm text-fg-muted">
                {overview.checkedAt ? t.proton.checkedAt(formatRelative(overview.checkedAt)) : t.proton.versionsHint}
              </p>
            </div>
            <Button icon={RefreshCw} size="sm" disabled={checking} onClick={() => void check()}>
              {checking ? t.proton.checking : t.proton.check}
            </Button>
          </div>
          {overview.offline && (
            <p role="status" className="mb-3 rounded-lg border border-border px-4 py-3 text-sm text-fg-muted">
              {t.proton.offline}
            </p>
          )}
          {shown.length > 0 && (
            <ul className="rounded-lg border border-border" aria-label={t.proton.versions}>
              {shown.map((version) => (
                <VersionRow key={version.id} version={version} overview={overview} />
              ))}
            </ul>
          )}
          {(hidden > 0 || showAll) && (
            <Button
              variant="ghost"
              size="sm"
              className="mt-2"
              onClick={() => {
                setShowAll(!showAll);
              }}
            >
              {showAll ? t.proton.showLess : t.proton.showAll(ge.length)}
            </Button>
          )}

          {steam.length > 0 && (
            <>
              <h3 className="mt-10 text-sm font-medium text-fg">{t.proton.steamVersions}</h3>
              <p className="mt-1 mb-3 text-sm text-fg-muted">{t.proton.steamVersionsHint}</p>
              <ul className="rounded-lg border border-border" aria-label={t.proton.steamVersions}>
                {steam.map((version) => (
                  <VersionRow key={version.id} version={version} overview={overview} />
                ))}
              </ul>
            </>
          )}
        </>
      )}
    </Panel>
  );
}
