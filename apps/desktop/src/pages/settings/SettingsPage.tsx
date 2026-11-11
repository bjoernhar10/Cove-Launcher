import { isLanguage, localized, messages, setLanguage, useMessages } from "../../i18n/index.ts";
import type {
  AccentColor,
  AppSettings,
  CatalogCredentials,
  CatalogProviderId,
  CatalogSource,
  ComponentVersion,
  ServiceStatus,
  UpdateCheck,
  UpdateProgress,
} from "@cove/shared";
import { Check, ChevronRight, Download, ExternalLink, FolderOpen, FolderPlus, FolderSearch, Plus, RefreshCw, Trash2 } from "lucide-react";
import { useState, type SyntheticEvent, type ReactNode } from "react";
import { Navigate, NavLink, useParams } from "react-router";
import { api } from "../../api/index.ts";
import { Button } from "../../components/ui/Button.tsx";
import { Badge, EmptyState, ErrorState, ProgressBar } from "../../components/ui/Feedback.tsx";
import { Field, Input, Select, SettingRow, Switch } from "../../components/ui/Field.tsx";
import { Page } from "../../components/ui/Page.tsx";
import { useCatalogStatus } from "../../hooks/useCatalog.ts";
import { AccountsTab } from "./AccountsTab.tsx";
import { PluginsTab } from "./PluginsTab.tsx";
import { ProtonTab } from "./ProtonTab.tsx";
import { Group, Panel, SettingsSkeleton } from "./parts.tsx";
import { useQuery } from "../../hooks/useQuery.ts";
import { queryClient } from "../../lib/queryClient.ts";
import { cx } from "../../lib/cx.ts";
import { detectPlatform, modKeyLabel } from "../../lib/env.ts";
import { formatPercent } from "../../lib/format.ts";
import { ACCENT_LABELS, PLATFORM_LABELS } from "../../lib/labels.ts";
import { usePreferences } from "../../stores/preferences.ts";
import { invalidateQueries } from "../../stores/query.ts";
import { errorMessage, toast } from "../../stores/toasts.ts";
import { useUi } from "../../stores/ui.ts";

const TABS = ["general", "appearance", "library", "accounts", "catalog", "downloads", "plugins", "proton", "about"] as const;

type TabId = (typeof TABS)[number];

const TAB_LABELS: Record<TabId, string> = localized((m) => m.settings.tabs);

/** Loads settings once and saves every change immediately. */
function useSettings(): {
  settings: AppSettings | null;
  error: string | null;
  retry: () => void;
  save: (fn: (s: AppSettings) => AppSettings) => void;
} {
  const query = useQuery("settings", api.settings.getSettings);
  const save = (fn: (s: AppSettings) => AppSettings): void => {
    if (query.status !== "success") return;
    const { accent } = usePreferences.getState();
    const next = { ...fn(query.data), appearance: { theme: "dark" as const, accent } };
    query.update(() => next);
    const languageChanged = next.general.language !== query.data.general.language;
    api.settings.saveSettings(next).then(
      () => {
        // Texts from the core (errors, store status, reasons) follow the new language.
        if (!languageChanged) return;
        invalidateQueries();
        void queryClient.invalidateQueries();
      },
      (error: unknown) => {
        toast(errorMessage(error), "danger");
        query.retry();
      },
    );
  };
  return {
    settings: query.status === "success" ? query.data : null,
    error: query.status === "error" ? query.error : null,
    retry: query.retry,
    save,
  };
}

function GeneralTab() {
  const t = useMessages();
  const { settings, error, retry, save } = useSettings();
  if (error) return <ErrorState message={error} onRetry={retry} />;
  if (!settings) return <SettingsSkeleton />;
  const g = settings.general;
  const set = (patch: Partial<AppSettings["general"]>) => {
    save((s) => ({ ...s, general: { ...s.general, ...patch } }));
  };
  return (
    <Panel title={t.settings.tabs.general}>
      <Group>
        <SettingRow title={t.settings.language} description={t.settings.languageHint}>
          <Select
            className="w-40"
            aria-label={t.settings.language}
            value={g.language}
            onChange={(language) => {
              set({ language });
              if (isLanguage(language)) setLanguage(language);
            }}
            options={[
              { value: "de", label: "Deutsch" },
              { value: "en", label: "English" },
            ]}
          />
        </SettingRow>
        <SettingRow title={t.settings.startPage} description={t.settings.startPageHint}>
          <Select
            className="w-40"
            aria-label={t.settings.startPage}
            value={g.startPage}
            onChange={(startPage) => {
              set({ startPage });
            }}
            options={[
              { value: "home", label: "Home" },
              { value: "library", label: "My Library" },
            ]}
          />
        </SettingRow>
        <SettingRow title={t.settings.autostart} description={t.settings.autostartHint}>
          <Switch
            label={t.settings.autostart}
            checked={g.launchOnStartup}
            onChange={(launchOnStartup) => {
              set({ launchOnStartup });
            }}
          />
        </SettingRow>
        <SettingRow title={t.settings.tray} description={t.settings.trayHint}>
          <Switch
            label={t.settings.tray}
            checked={g.minimizeToTray}
            onChange={(minimizeToTray) => {
              set({ minimizeToTray });
            }}
          />
        </SettingRow>
      </Group>
    </Panel>
  );
}

const ACCENTS = Object.keys(ACCENT_LABELS) as AccentColor[];

/** Swatch colors are the accent tokens themselves, applied via data-accent on a preview element. */
function AccentSwatch({ accent, selected, onSelect }: { accent: AccentColor; selected: boolean; onSelect: () => void }) {
  return (
    <button
      type="button"
      role="radio"
      aria-checked={selected}
      aria-label={ACCENT_LABELS[accent]}
      title={ACCENT_LABELS[accent]}
      onClick={onSelect}
      className="flex flex-col items-center gap-2 rounded-md p-1"
    >
      <span
        data-swatch={accent}
        className={cx(
          "flex size-8 items-center justify-center rounded-full bg-(--swatch) text-on-accent outline-offset-2",
          selected && "outline-2 outline-(--swatch)",
        )}
      >
        {selected && <Check aria-hidden />}
      </span>
      <span className={cx("text-xs", selected ? "text-fg" : "text-fg-muted")}>{ACCENT_LABELS[accent]}</span>
    </button>
  );
}

function AppearanceTab() {
  const t = useMessages();
  const accent = usePreferences((s) => s.accent);
  const setAccent = usePreferences((s) => s.setAccent);
  const collapsed = usePreferences((s) => s.sidebarCollapsed);
  const toggleSidebar = usePreferences((s) => s.toggleSidebar);

  return (
    <Panel title={t.settings.tabs.appearance} description={t.settings.appearanceHint}>
      <div className="flex flex-col gap-10">
        <div>
          <h3 className="text-sm font-medium text-fg">{t.settings.accent}</h3>
          <p className="mt-1 mb-3 text-sm text-fg-muted">{t.settings.accentHint}</p>
          <div role="radiogroup" aria-label={t.settings.accent} className="flex flex-wrap gap-4">
            {ACCENTS.map((a) => (
              <AccentSwatch
                key={a}
                accent={a}
                selected={accent === a}
                onSelect={() => {
                  setAccent(a);
                }}
              />
            ))}
          </div>
        </div>
        <Group>
          <SettingRow title={t.settings.compactSidebar} description={t.settings.compactSidebarHint(modKeyLabel)}>
            <Switch label={t.settings.compactSidebar} checked={collapsed} onChange={toggleSidebar} />
          </SettingRow>
        </Group>
      </div>
    </Panel>
  );
}

function LibraryTab() {
  const t = useMessages();
  const { settings, error, retry, save } = useSettings();
  const openAddGame = useUi((s) => s.openAddGame);
  const [folder, setFolder] = useState("");
  if (error) return <ErrorState message={error} onRetry={retry} />;
  if (!settings) return <SettingsSkeleton />;
  const lib = settings.library;

  function add(event: SyntheticEvent): void {
    event.preventDefault();
    const path = folder.trim();
    if (!path || lib.scanFolders.includes(path)) return;
    save((s) => ({ ...s, library: { ...s.library, scanFolders: [...s.library.scanFolders, path] } }));
    setFolder("");
  }

  async function addGameFolder(): Promise<void> {
    try {
      const picked = await api.system.pickFolder(settings?.downloads.directory ?? null);
      if (!picked) return;
      const info = await api.system.folderInfo(picked);
      if (!info.writable) {
        toast(t.install.notWritable, "danger");
        return;
      }
      save((s) => ({ ...s, library: { ...s.library, gameFolders: [...s.library.gameFolders.filter((f) => f !== info.path), info.path] } }));
    } catch (error: unknown) {
      toast(errorMessage(error), "danger");
    }
  }

  return (
    <Panel title={t.settings.tabs.library} description={t.settings.libraryHint}>
      <h3 className="text-sm font-medium text-fg">{t.settings.gameFolders}</h3>
      <p className="mt-1 mb-3 text-sm text-fg-muted">{t.settings.gameFoldersHint}</p>
      <ul className="rounded-lg border border-border">
        {lib.gameFolders.map((path) => {
          const isDefault = path === settings.downloads.directory;
          return (
            <li key={path} className="flex items-center gap-3 border-b border-border py-2 pr-2 pl-4 last:border-b-0">
              <FolderOpen aria-hidden className="text-fg-subtle" />
              <span className="min-w-0 flex-1 truncate text-sm text-fg">{path}</span>
              {isDefault ? (
                <Badge>{t.settings.defaultFolder}</Badge>
              ) : (
                <>
                  <Button
                    variant="ghost"
                    size="sm"
                    aria-label={t.settings.makeDefaultFolder(path)}
                    onClick={() => {
                      save((s) => ({ ...s, downloads: { ...s.downloads, directory: path } }));
                    }}
                  >
                    {t.settings.defaultFolder}
                  </Button>
                  <Button
                    variant="ghost"
                    size="sm"
                    iconOnly
                    icon={Trash2}
                    label={t.settings.removeGameFolder(path)}
                    onClick={() => {
                      save((s) => ({ ...s, library: { ...s.library, gameFolders: s.library.gameFolders.filter((p) => p !== path) } }));
                    }}
                  />
                </>
              )}
            </li>
          );
        })}
      </ul>
      <Button className="mt-3" icon={FolderPlus} onClick={() => void addGameFolder()}>
        {t.settings.addGameFolder}
      </Button>

      <h3 className="mt-8 mb-3 text-sm font-medium text-fg">{t.settings.scanFolders}</h3>
      {lib.scanFolders.length === 0 ? (
        <div className="rounded-lg border border-border">
          <EmptyState compact icon={FolderOpen} title={t.settings.noFolders} description={t.settings.noFoldersHint} />
        </div>
      ) : (
        <ul className="rounded-lg border border-border">
          {lib.scanFolders.map((path) => (
            <li key={path} className="flex items-center gap-3 border-b border-border py-2 pr-2 pl-4 last:border-b-0">
              <FolderOpen aria-hidden className="text-fg-subtle" />
              <span className="min-w-0 flex-1 truncate text-sm text-fg">{path}</span>
              <Button
                variant="ghost"
                size="sm"
                iconOnly
                icon={FolderSearch}
                label={t.settings.scanPath(path)}
                onClick={() => {
                  openAddGame({ mode: "scan", folder: path });
                }}
              />
              <Button
                variant="ghost"
                size="sm"
                iconOnly
                icon={Trash2}
                label={t.settings.removePath(path)}
                onClick={() => {
                  save((s) => ({ ...s, library: { ...s.library, scanFolders: s.library.scanFolders.filter((p) => p !== path) } }));
                }}
              />
            </li>
          ))}
        </ul>
      )}
      <form className="mt-3 flex gap-2" onSubmit={add}>
        <Input
          aria-label={t.settings.newFolder}
          placeholder={t.settings.folderPlaceholder}
          spellCheck={false}
          value={folder}
          onChange={(e) => {
            setFolder(e.target.value);
          }}
        />
        <Button type="submit" icon={Plus} disabled={!folder.trim()}>
          {t.addGame.add}
        </Button>
      </form>
      <div className="mt-8">
        <Group>
          <SettingRow title={t.settings.scanOnStart} description={t.settings.scanOnStartHint}>
            <Switch
              label={t.settings.scanOnStart}
              checked={lib.scanOnStartup}
              onChange={(scanOnStartup) => {
                save((s) => ({ ...s, library: { ...s.library, scanOnStartup } }));
              }}
            />
          </SettingRow>
        </Group>
      </div>
      <h3 className="mt-8 mb-3 text-sm font-medium text-fg">{t.settings.stores}</h3>
      <Group>
        <SettingRow title={t.settings.steamSilent} description={t.settings.steamSilentHint}>
          <Switch
            label={t.settings.steamSilent}
            checked={lib.steamSilentStart}
            onChange={(steamSilentStart) => {
              save((s) => ({ ...s, library: { ...s.library, steamSilentStart } }));
            }}
          />
        </SettingRow>
        <SettingRow title={t.settings.epicCloudSaves} description={t.settings.epicCloudSavesHint}>
          <Switch
            label={t.settings.epicCloudSaves}
            checked={lib.epicCloudSaves}
            onChange={(epicCloudSaves) => {
              save((s) => ({ ...s, library: { ...s.library, epicCloudSaves } }));
            }}
          />
        </SettingRow>
      </Group>
    </Panel>
  );
}

interface CredentialField { key: string; label: string; secret: boolean }

const PROVIDERS: {
  id: CatalogProviderId;
  name: string;
  help: string;
  fields: CredentialField[];
}[] = [
  {
    id: "igdb",
    get name() {
      return messages().settings.igdbName;
    },
    get help() {
      return messages().settings.igdbHelp;
    },
    fields: [
      { key: "clientId", label: "Client-ID", secret: false },
      { key: "clientSecret", label: "Client-Secret", secret: true },
    ],
  },
  {
    id: "rawg",
    name: "RAWG",
    get help() {
      return messages().settings.rawgHelp;
    },
    fields: [{ key: "apiKey", label: "API-Key", secret: true }],
  },
];

function toCredentials(id: CatalogProviderId, values: Record<string, string>): CatalogCredentials {
  return id === "igdb"
    ? { provider: "igdb", clientId: values.clientId ?? "", clientSecret: values.clientSecret ?? "" }
    : { provider: "rawg", apiKey: values.apiKey ?? "" };
}

function ProviderCard({ provider, configured }: { provider: (typeof PROVIDERS)[number]; configured: boolean }) {
  const t = useMessages();
  const [values, setValues] = useState<Record<string, string>>({});
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  const complete = provider.fields.every((f) => (values[f.key] ?? "").trim() !== "");

  async function save(event: SyntheticEvent): Promise<void> {
    event.preventDefault();
    setBusy(true);
    setError(null);
    try {
      await api.catalog.setCatalogCredentials(toCredentials(provider.id, values));
      setValues({});
      toast(t.settings.providerConnected(provider.id === "igdb" ? "IGDB" : "RAWG"));
      invalidateQueries();
    } catch (e: unknown) {
      setError(errorMessage(e));
    } finally {
      setBusy(false);
    }
  }

  async function remove(): Promise<void> {
    setBusy(true);
    try {
      await api.catalog.clearCatalogCredentials(provider.id);
      toast(t.settings.providerRemoved(provider.id === "igdb" ? "IGDB" : "RAWG"));
      invalidateQueries();
    } catch (e: unknown) {
      toast(errorMessage(e), "danger");
    } finally {
      setBusy(false);
    }
  }

  return (
    <form className="rounded-lg border border-border p-5" onSubmit={(e) => void save(e)}>
      <div className="flex items-center justify-between gap-4">
        <h3 className="text-sm font-medium text-fg">{provider.name}</h3>
        <span className={cx("inline-flex items-center gap-2 text-xs", configured ? "text-accent-text" : "text-fg-subtle")}>
          {configured && <Check aria-hidden />}
          {configured ? t.settings.configured : t.settings.notConfigured}
        </span>
      </div>
      <p className="mt-1 text-sm text-fg-muted">{provider.help}</p>
      <div className="mt-4 flex flex-col gap-4">
        {provider.fields.map((field) => (
          <Field key={field.key} label={field.label} error={field === provider.fields.at(-1) ? error : null}>
            {(props) => (
              <Input
                {...props}
                type={field.secret ? "password" : "text"}
                autoComplete="off"
                spellCheck={false}
                placeholder={configured ? t.settings.savedPlaceholder : ""}
                value={values[field.key] ?? ""}
                onChange={(e) => {
                  setValues((v) => ({ ...v, [field.key]: e.target.value }));
                  setError(null);
                }}
              />
            )}
          </Field>
        ))}
      </div>
      <div className="mt-5 flex gap-2">
        <Button type="submit" variant="primary" disabled={!complete || busy}>
          {busy ? t.settings.checking : t.settings.saveAndCheck}
        </Button>
        {configured && (
          <Button variant="ghost" disabled={busy} onClick={() => void remove()}>
            {t.game.remove}
          </Button>
        )}
      </div>
    </form>
  );
}

const IGDB_URL = "https://www.igdb.com";

const SOURCE_NAMES: Record<CatalogSource, string> = localized((m) => m.settings.sourceNames);

function CatalogTab() {
  const t = useMessages();
  const status = useCatalogStatus();
  const { settings, save } = useSettings();
  const [matching, setMatching] = useState(false);

  async function matchLibrary(): Promise<void> {
    setMatching(true);
    try {
      const items = await api.library.listLibrary();
      const open = items.filter((i) => i.entry.matchStatus === "none");
      for (const item of open) await api.library.rematch(item.game.id);
      toast(
        open.length === 0 ? t.settings.allMatched : t.settings.matchingCount(open.length),
      );
    } catch (e: unknown) {
      toast(errorMessage(e), "danger");
    } finally {
      setMatching(false);
    }
  }

  return (
    <Panel
      title={t.settings.tabs.catalog}
      description={t.settings.catalogHint}
    >
      {status.isPending && <SettingsSkeleton />}
      {status.isError && <ErrorState message={errorMessage(status.error)} onRetry={() => void status.refetch()} />}
      {status.isSuccess && (
        <div className="flex flex-col gap-8">
          <div role="status" className="rounded-lg border border-border px-5 py-4">
            {status.data.sources.length > 0 ? (
              <>
                <p className="flex items-center gap-2 text-sm font-medium text-fg">
                  <Check aria-hidden className="text-accent-text" />
                  {status.data.sources[0] === "cove" ? t.settings.catalogReady : t.settings.catalogConfigured}
                </p>
                <p className="mt-1 text-sm text-fg-muted">
                  {t.settings.order(status.data.sources.map((s) => SOURCE_NAMES[s]))}
                </p>
              </>
            ) : (
              <>
                <p className="text-sm font-medium text-fg">{t.catalogue.unavailableTitle}</p>
                <p className="mt-1 text-sm text-fg-muted">
                  {status.data.coveService.available
                    ? t.settings.serviceOff
                    : t.settings.serviceMissing}
                </p>
              </>
            )}
          </div>

          {status.data.sources.length > 0 && (
            <Group>
              <SettingRow
                title={t.settings.matchLibrary}
                description={t.settings.matchLibraryHint}
              >
                <Button icon={RefreshCw} disabled={matching} onClick={() => void matchLibrary()}>
                  {t.settings.match}
                </Button>
              </SettingRow>
            </Group>
          )}

          <details className="group" open={status.data.sources.length === 0}>
            <summary className="flex cursor-pointer list-none items-center gap-2 text-sm font-medium text-fg [&::-webkit-details-marker]:hidden">
              <ChevronRight aria-hidden className="text-fg-subtle transition-transform duration-150 ease-out group-open:rotate-90" />
              {t.settings.advanced}
            </summary>
            <div className="mt-4 flex flex-col gap-6">
              {settings && (
                <Group>
                  <SettingRow
                    title={t.settings.useService}
                    description={status.data.coveService.available ? t.settings.useServiceHint : t.settings.notInBuild}
                  >
                    <Switch
                      label={t.settings.useService}
                      checked={settings.catalog.useCoveService && status.data.coveService.available}
                      disabled={!status.data.coveService.available}
                      onChange={(useCoveService) => {
                        save((s) => ({ ...s, catalog: { useCoveService } }));
                        void queryClient.invalidateQueries({ queryKey: ["catalog"] });
                      }}
                    />
                  </SettingRow>
                </Group>
              )}
              <p className="text-sm text-fg-muted">
                {t.settings.ownKeysHint}
              </p>
              {PROVIDERS.map((p) => (
                <ProviderCard key={p.id} provider={p} configured={status.data.configured[p.id]} />
              ))}
            </div>
          </details>
        </div>
      )}
    </Panel>
  );
}

const MB = 1e6;

function DownloadsTab() {
  const t = useMessages();
  const { settings, error, retry, save } = useSettings();
  if (error) return <ErrorState message={error} onRetry={retry} />;
  if (!settings) return <SettingsSkeleton />;
  const d = settings.downloads;
  const set = (patch: Partial<AppSettings["downloads"]>) => {
    save((s) => ({ ...s, downloads: { ...s.downloads, ...patch } }));
  };
  const limited = d.bandwidthLimitBytesPerSecond !== null;

  return (
    <Panel title={t.settings.tabs.downloads}>
      <div className="flex max-w-xl flex-col gap-8">
        <Group>
          <SettingRow title={t.settings.limitBandwidth} description={t.settings.limitBandwidthHint}>
            <Switch
              label={t.settings.limitBandwidth}
              checked={limited}
              onChange={(on) => {
                set({ bandwidthLimitBytesPerSecond: on ? 10 * MB : null });
              }}
            />
          </SettingRow>
          {limited && (
            <SettingRow title={t.settings.limit} description={t.settings.limitHint}>
              <div className="flex items-center gap-2">
                <Input
                  type="number"
                  min={1}
                  max={1000}
                  aria-label={t.settings.limitLabel}
                  className="num w-24 text-right"
                  value={Math.round((d.bandwidthLimitBytesPerSecond ?? 0) / MB)}
                  onChange={(e) => {
                    const value = Math.max(1, Number(e.target.value) || 1);
                    set({ bandwidthLimitBytesPerSecond: value * MB });
                  }}
                />
                <span className="text-sm text-fg-muted">MB/s</span>
              </div>
            </SettingRow>
          )}
          <SettingRow title={t.settings.concurrent} description={t.settings.concurrentHint}>
            <Select
              className="w-24"
              aria-label={t.settings.concurrent}
              value={String(d.maxConcurrent) as "1" | "2" | "3"}
              onChange={(v) => {
                set({ maxConcurrent: Number(v) });
              }}
              options={[
                { value: "1", label: "1" },
                { value: "2", label: "2" },
                { value: "3", label: "3" },
              ]}
            />
          </SettingRow>
        </Group>
      </div>
    </Panel>
  );
}

const COMPONENT_LABELS: Record<ComponentVersion["name"], string> = localized((m) => m.settings.components);

/** What is off while a service is unavailable (the rest of Cove keeps working). */
const COMPONENT_IMPACT: Record<ComponentVersion["name"], string> = localized((m) => m.settings.componentImpact);

const SERVICE_STATUS_LABELS: Record<ServiceStatus, string> = localized((m) => m.settings.serviceStatus);

function UpdateSection() {
  const t = useMessages();
  const [state, setState] = useState<
    | { phase: "idle" }
    | { phase: "checking" }
    | { phase: "result"; check: UpdateCheck }
    | { phase: "installing"; progress: UpdateProgress | null }
  >({ phase: "idle" });

  async function check(): Promise<void> {
    setState({ phase: "checking" });
    try {
      setState({ phase: "result", check: await api.system.checkForUpdates() });
    } catch (error: unknown) {
      toast(errorMessage(error), "danger");
      setState({ phase: "idle" });
    }
  }

  async function install(check: UpdateCheck): Promise<void> {
    setState({ phase: "installing", progress: null });
    const unsubscribe = api.system.subscribeUpdateProgress((progress) => {
      setState({ phase: "installing", progress });
    });
    try {
      // Cove restarts when the update is installed.
      await api.system.installUpdate();
    } catch (error: unknown) {
      toast(errorMessage(error), "danger");
      setState({ phase: "result", check });
    } finally {
      unsubscribe();
    }
  }

  const available = state.phase === "result" ? state.check.available : null;
  return (
    <div aria-live="polite">
      <h3 className="mb-3 text-sm font-medium text-fg">{t.settings.updates}</h3>
      {state.phase === "installing" ? (
        <div className="max-w-md">
          <p className="text-sm text-fg-muted">
            {t.settings.updateLoading}{" "}
            {state.progress?.totalBytes ? <span className="num">{formatPercent(state.progress.downloadedBytes / state.progress.totalBytes)}</span> : "…"}
          </p>
          <ProgressBar value={state.progress?.downloadedBytes ?? 0} max={state.progress?.totalBytes ?? 0} label={t.settings.updateProgress} className="mt-3" />
        </div>
      ) : (
        <div className="flex max-w-md flex-col items-start gap-3">
          {state.phase === "result" && !available && <p className="text-sm text-fg-muted">{t.settings.upToDate}</p>}
          {state.phase === "result" && available && (
            <div className="text-sm">
              <p className="text-fg">
                {t.settings.versionAvailable(available.version)}
              </p>
              {available.notes && <p className="mt-1 whitespace-pre-line text-fg-muted">{available.notes}</p>}
              {state.check.installHint && <p className="mt-2 text-fg-muted">{state.check.installHint}</p>}
            </div>
          )}
          <div className="flex gap-2">
            {state.phase === "result" && available && state.check.canInstall && (
              <Button
                variant="primary"
                icon={Download}
                onClick={() => {
                  void install(state.check);
                }}
              >
                {t.settings.installRestart}
              </Button>
            )}
            <Button icon={RefreshCw} disabled={state.phase === "checking"} onClick={() => void check()}>
              {state.phase === "checking" ? t.settings.searching : t.settings.checkUpdates}
            </Button>
          </div>
        </div>
      )}
    </div>
  );
}

function AboutTab() {
  const t = useMessages();
  const query = useQuery("about", api.system.getAbout);

  return (
    <Panel title={t.settings.tabs.about}>
      {query.status === "loading" && <SettingsSkeleton />}
      {query.status === "error" && <ErrorState message={query.error} onRetry={query.retry} />}
      {query.status === "success" && (
        <div className="flex flex-col gap-8">
          <div>
            <p className="text-xl font-semibold tracking-tight text-fg">Cove</p>
            <p className="num mt-1 text-sm text-fg-muted">
              {t.settings.version(query.data.appVersion)} · {PLATFORM_LABELS[query.data.platform]}
            </p>
            <p className="mt-3 max-w-md text-sm text-fg-muted">{t.settings.tagline}</p>
            <p className="mt-3 text-sm text-fg-muted">
              Game data provided by{" "}
              <a
                href={IGDB_URL}
                className="inline-flex items-center gap-1 text-accent-text underline-offset-2 hover:underline"
                onClick={(e) => {
                  e.preventDefault();
                  api.system.openExternal(IGDB_URL).catch((error: unknown) => {
                    toast(errorMessage(error), "danger");
                  });
                }}
              >
                IGDB.com
                <ExternalLink aria-hidden />
              </a>
            </p>
          </div>
          <div>
            <h3 className="mb-3 text-sm font-medium text-fg">{t.settings.componentsTitle}</h3>
            <dl className="m-0 max-w-md rounded-lg border border-border px-4 text-sm">
              {query.data.components.map((c) => (
                <div key={c.name} className="flex items-start justify-between gap-4 border-b border-border py-3 last:border-b-0">
                  <dt className="text-fg-muted">
                    {COMPONENT_LABELS[c.name]}
                    {c.status === "unavailable" && <span className="mt-1 block text-xs text-fg-subtle">{COMPONENT_IMPACT[c.name]}</span>}
                  </dt>
                  <dd className="m-0 text-right text-fg">{c.name === "core" ? <span className="num">{c.version}</span> : SERVICE_STATUS_LABELS[c.status]}</dd>
                </div>
              ))}
            </dl>
          </div>
          <UpdateSection />
        </div>
      )}
    </Panel>
  );
}

const PANELS: Record<TabId, () => ReactNode> = {
  general: GeneralTab,
  appearance: AppearanceTab,
  library: LibraryTab,
  accounts: AccountsTab,
  catalog: CatalogTab,
  downloads: DownloadsTab,
  plugins: PluginsTab,
  proton: ProtonTab,
  about: AboutTab,
};

/** Proton runs Windows games on Linux: its tab only exists there. */
function visibleTabs(): readonly TabId[] {
  return detectPlatform() === "linux" ? TABS : TABS.filter((id) => id !== "proton");
}

export function SettingsPage() {
  const t = useMessages();
  const { tab } = useParams();
  const tabs = visibleTabs();
  const current = tabs.find((x) => x === tab);
  if (!current) return <Navigate to="/settings/general" replace />;
  const PanelComponent = PANELS[current];

  return (
    <Page className="max-w-5xl">
      <h1 className="mb-8 text-xl font-semibold tracking-tight text-fg">Settings</h1>
      <div className="grid grid-cols-[200px_1fr] gap-12">
        <nav aria-label={t.game.settings}>
          <ul className="flex flex-col gap-1">
            {tabs.map((id) => (
              <li key={id}>
                <NavLink
                  to={`/settings/${id}`}
                  className={({ isActive }) =>
                    cx(
                      "flex h-8 items-center rounded-md px-3 text-sm transition-colors duration-150 ease-out",
                      isActive ? "bg-active font-medium text-fg" : "text-fg-muted hover:bg-hover hover:text-fg",
                    )
                  }
                >
                  {TAB_LABELS[id]}
                </NavLink>
              </li>
            ))}
          </ul>
        </nav>
        <div className="min-w-0">
          <PanelComponent />
        </div>
      </div>
    </Page>
  );
}
