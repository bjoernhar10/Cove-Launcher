import type { ProtonOverview, ProtonProgress, ProtonVersionId } from "@cove/shared";
import { LATEST_PROTON } from "../api/proton.ts";
import { ProgressBar } from "../components/ui/Feedback.tsx";
import { Select } from "../components/ui/Field.tsx";
import { useMessages, type Messages } from "../i18n/index.ts";
import { formatBytes, formatPercent } from "../lib/format.ts";
import { useProtonInstall } from "../stores/proton.ts";

/** Value of the "Standard" option (a game without a choice of its own). */
export const DEFAULT_OPTION = "";

/** The build an id stands for right now ("latest" → the newest GE-Proton). */
export function protonTarget(id: ProtonVersionId, overview: ProtonOverview): ProtonVersionId {
  return id === LATEST_PROTON ? (overview.latest ?? id) : id;
}

/** Readable name of a choice, e.g. "GE-Proton – immer die neueste (GE-Proton10-25)". */
export function protonLabel(id: ProtonVersionId, overview: ProtonOverview, t: Messages): string {
  if (id === LATEST_PROTON) return overview.latest ? t.proton.latestNamed(overview.latest) : t.proton.latest;
  const version = overview.versions.find((v) => v.id === id);
  if (!version) return t.proton.missing(id.startsWith("steam:") ? id.slice("steam:".length) : id);
  return version.kind === "steam" ? t.proton.steam(version.name) : version.name;
}

interface ProtonSelectProps {
  id?: string;
  "aria-describedby"?: string;
  /** `DEFAULT_OPTION` = the default; only offered when `defaultId` is given. */
  value: string;
  onChange: (value: string) => void;
  overview: ProtonOverview;
  /** The default build, for the "Standard (…)" option of a game. */
  defaultId?: ProtonVersionId;
}

/** All builds a game can use: the default, "latest", GE-Proton releases and Steam's builds. */
export function ProtonSelect({ value, onChange, overview, defaultId, ...rest }: ProtonSelectProps) {
  const t = useMessages();
  const options: { value: string; label: string }[] = [];
  if (defaultId !== undefined) options.push({ value: DEFAULT_OPTION, label: t.proton.defaultOption(protonLabel(defaultId, overview, t)) });
  options.push({ value: LATEST_PROTON, label: protonLabel(LATEST_PROTON, overview, t) });
  for (const version of overview.versions) {
    const name = protonLabel(version.id, overview, t);
    options.push({ value: version.id, label: version.kind === "ge" && version.installed ? t.proton.installedSuffix(name) : name });
  }
  // A build that is gone (e.g. removed in Steam) stays visible as the current choice.
  if (!options.some((o) => o.value === value)) options.push({ value, label: protonLabel(value, overview, t) });
  return <Select {...rest} value={value} onChange={onChange} options={options} />;
}

function progressText(progress: ProtonProgress, t: Messages): string {
  if (progress.phase === "verifying") return t.proton.verifying;
  if (progress.phase === "unpacking") return t.proton.unpacking;
  return t.proton.downloading(formatPercent(progress.totalBytes > 0 ? progress.downloadedBytes / progress.totalBytes : 0));
}

/** One line under a choice: installed, will be downloaded, or the running install with a bar. */
export function ProtonStatus({ id, overview, pending = true }: { id: ProtonVersionId; overview: ProtonOverview; pending?: boolean }) {
  const t = useMessages();
  const target = protonTarget(id, overview);
  const version = overview.versions.find((v) => v.id === target);
  const install = useProtonInstall(target, overview);

  if (install) {
    return (
      <div role="status" className="flex flex-col gap-2">
        <p className="text-xs text-fg-muted">{progressText(install, t)}</p>
        <ProgressBar value={install.downloadedBytes} max={install.totalBytes || 1} label={t.proton.installing(target)} />
      </div>
    );
  }
  let text: string;
  if (version?.installed) text = t.proton.statusInstalled;
  else if (version?.kind === "ge" && version.downloadBytes !== null && pending) text = t.proton.statusDownload(formatBytes(version.downloadBytes));
  else if (id === LATEST_PROTON && pending) text = t.proton.statusDownloadLatest;
  else if (version?.kind === "ge" || id === LATEST_PROTON) text = t.proton.statusLater;
  else text = t.proton.statusMissing;
  return (
    <p role="status" className="text-xs text-fg-muted">
      {text}
    </p>
  );
}
