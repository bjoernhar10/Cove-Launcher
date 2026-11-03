import type { AccentColor, AppSettings, ConnectableStore, ThemePreference } from "@cove/shared";
import { Check, FolderOpen, FolderSearch, Monitor, Moon, Plus, Puzzle, RefreshCw, Sun, Trash2 } from "lucide-react";
import { useState, type SyntheticEvent, type ReactNode } from "react";
import { Navigate, NavLink, useParams } from "react-router";
import { api } from "../../api/index.ts";
import { Button } from "../../components/ui/Button.tsx";
import { EmptyState, ErrorState, LoadingRegion, Skeleton } from "../../components/ui/Feedback.tsx";
import { Field, Input, Select, SettingRow, Switch } from "../../components/ui/Field.tsx";
import { Page } from "../../components/ui/Page.tsx";
import { useQuery } from "../../hooks/useQuery.ts";
import { cx } from "../../lib/cx.ts";
import { modKeyLabel } from "../../lib/env.ts";
import { formatRelative } from "../../lib/format.ts";
import { ACCENT_LABELS, PLATFORM_LABELS, SOURCE_LABELS, THEME_LABELS } from "../../lib/labels.ts";
import { usePreferences } from "../../stores/preferences.ts";
import { invalidateQueries } from "../../stores/query.ts";
import { errorMessage, toast } from "../../stores/toasts.ts";
import { useUi } from "../../stores/ui.ts";

const TABS = [
  { id: "general", label: "Allgemein" },
  { id: "appearance", label: "Aussehen" },
  { id: "library", label: "Bibliothek" },
  { id: "accounts", label: "Konten & Stores" },
  { id: "downloads", label: "Downloads" },
  { id: "plugins", label: "Plugins" },
  { id: "about", label: "Über Cove" },
] as const;

type TabId = (typeof TABS)[number]["id"];

function Panel({ title, description, children }: { title: string; description?: string; children: ReactNode }) {
  return (
    <section aria-label={title}>
      <h2 className="text-lg font-semibold tracking-tight text-fg">{title}</h2>
      {description && <p className="mt-1 text-sm text-fg-muted">{description}</p>}
      <div className="mt-6">{children}</div>
    </section>
  );
}

function Group({ children }: { children: ReactNode }) {
  return <div className="divide-y divide-border border-y border-border">{children}</div>;
}

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
    const { theme, accent } = usePreferences.getState();
    const next = { ...fn(query.data), appearance: { theme, accent } };
    query.update(() => next);
    api.settings.saveSettings(next).catch((error: unknown) => {
      toast(errorMessage(error), "danger");
      query.retry();
    });
  };
  return {
    settings: query.status === "success" ? query.data : null,
    error: query.status === "error" ? query.error : null,
    retry: query.retry,
    save,
  };
}

function SettingsSkeleton() {
  return (
    <LoadingRegion label="Einstellungen werden geladen" className="flex flex-col gap-4">
      {[0, 1, 2].map((i) => (
        <Skeleton key={i} className="h-14" />
      ))}
    </LoadingRegion>
  );
}

function GeneralTab() {
  const { settings, error, retry, save } = useSettings();
  if (error) return <ErrorState message={error} onRetry={retry} />;
  if (!settings) return <SettingsSkeleton />;
  const g = settings.general;
  const set = (patch: Partial<AppSettings["general"]>) => {
    save((s) => ({ ...s, general: { ...s.general, ...patch } }));
  };
  return (
    <Panel title="Allgemein">
      <Group>
        <SettingRow title="Sprache" description="Die Sprache der Oberfläche.">
          <Select
            className="w-40"
            aria-label="Sprache"
            value={g.language}
            onChange={(language) => {
              set({ language });
            }}
            options={[
              { value: "de", label: "Deutsch" },
              { value: "en", label: "English" },
            ]}
          />
        </SettingRow>
        <SettingRow title="Startseite" description="Was Cove nach dem Öffnen zeigt.">
          <Select
            className="w-40"
            aria-label="Startseite"
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
        <SettingRow title="Mit dem System starten" description="Cove öffnet sich automatisch nach der Anmeldung.">
          <Switch
            label="Mit dem System starten"
            checked={g.launchOnStartup}
            onChange={(launchOnStartup) => {
              set({ launchOnStartup });
            }}
          />
        </SettingRow>
        <SettingRow title="Beim Schließen in den Infobereich" description="Cove läuft im Hintergrund weiter, z. B. für Downloads.">
          <Switch
            label="Beim Schließen in den Infobereich"
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

const THEME_ICONS = { light: Sun, dark: Moon, system: Monitor } as const;
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

function ThemeCard({ theme, selected, onSelect }: { theme: ThemePreference; selected: boolean; onSelect: () => void }) {
  const Icon = THEME_ICONS[theme];
  return (
    <button
      type="button"
      role="radio"
      aria-checked={selected}
      onClick={onSelect}
      className={cx(
        "flex w-36 flex-col overflow-hidden rounded-lg border text-left transition-colors duration-150 ease-out",
        selected ? "border-accent" : "border-border hover:border-border-strong",
      )}
    >
      <span data-preview={theme} className="flex h-20 gap-1.5 border-b border-border bg-(--pv-bg) p-2">
        <span className="w-6 rounded-md bg-(--pv-side)" />
        <span className="flex flex-1 flex-col gap-1.5">
          <span className="h-2 w-3/4 rounded-full bg-(--pv-line)" />
          <span className="h-2 w-1/2 rounded-full bg-(--pv-line)" />
          <span className="mt-auto h-3 w-8 rounded-md bg-accent" />
        </span>
      </span>
      <span className="flex items-center gap-2 px-3 py-2 text-sm">
        <Icon aria-hidden className={selected ? "text-accent-text" : "text-fg-subtle"} />
        <span className={selected ? "font-medium text-fg" : "text-fg-muted"}>{THEME_LABELS[theme]}</span>
      </span>
    </button>
  );
}

function AppearanceTab() {
  const theme = usePreferences((s) => s.theme);
  const accent = usePreferences((s) => s.accent);
  const setTheme = usePreferences((s) => s.setTheme);
  const setAccent = usePreferences((s) => s.setAccent);
  const collapsed = usePreferences((s) => s.sidebarCollapsed);
  const toggleSidebar = usePreferences((s) => s.toggleSidebar);

  return (
    <Panel title="Aussehen" description="Änderungen werden sofort übernommen.">
      <div className="flex flex-col gap-10">
        <div>
          <h3 className="mb-3 text-sm font-medium text-fg">Design</h3>
          <div role="radiogroup" aria-label="Design" className="flex flex-wrap gap-3">
            {(["light", "dark", "system"] as const).map((t) => (
              <ThemeCard
                key={t}
                theme={t}
                selected={theme === t}
                onSelect={() => {
                  setTheme(t);
                }}
              />
            ))}
          </div>
        </div>
        <div>
          <h3 className="text-sm font-medium text-fg">Akzentfarbe</h3>
          <p className="mt-0.5 mb-3 text-sm text-fg-muted">Eine ruhige Farbe für aktive Elemente und Hervorhebungen.</p>
          <div role="radiogroup" aria-label="Akzentfarbe" className="flex flex-wrap gap-4">
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
          <SettingRow title="Kompakte Seitenleiste" description={`Zeigt nur Symbole. Auch mit ${modKeyLabel}+B umschaltbar.`}>
            <Switch label="Kompakte Seitenleiste" checked={collapsed} onChange={toggleSidebar} />
          </SettingRow>
        </Group>
      </div>
    </Panel>
  );
}

function LibraryTab() {
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

  return (
    <Panel title="Bibliothek" description="Ordner, in denen Cove nach installierten Spielen sucht.">
      <h3 className="mb-3 text-sm font-medium text-fg">Scan-Ordner</h3>
      {lib.scanFolders.length === 0 ? (
        <div className="rounded-lg border border-border">
          <EmptyState compact icon={FolderOpen} title="Noch keine Ordner" description="Füge einen Ordner hinzu, z. B. deinen Spiele- oder Steam-Library-Ordner." />
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
                label={`${path} scannen`}
                onClick={() => {
                  openAddGame({ mode: "scan", folder: path });
                }}
              />
              <Button
                variant="ghost"
                size="sm"
                iconOnly
                icon={Trash2}
                label={`${path} entfernen`}
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
          aria-label="Neuer Scan-Ordner"
          placeholder="/pfad/zum/ordner"
          spellCheck={false}
          value={folder}
          onChange={(e) => {
            setFolder(e.target.value);
          }}
        />
        <Button type="submit" icon={Plus} disabled={!folder.trim()}>
          Hinzufügen
        </Button>
      </form>
      <div className="mt-8">
        <Group>
          <SettingRow title="Beim Start scannen" description="Neue Spiele in den Ordnern automatisch erkennen.">
            <Switch
              label="Beim Start scannen"
              checked={lib.scanOnStartup}
              onChange={(scanOnStartup) => {
                save((s) => ({ ...s, library: { ...s.library, scanOnStartup } }));
              }}
            />
          </SettingRow>
        </Group>
      </div>
    </Panel>
  );
}

function AccountsTab() {
  const query = useQuery("accounts", api.settings.listAccounts);
  const [busy, setBusy] = useState<ConnectableStore | null>(null);

  async function connect(store: ConnectableStore): Promise<void> {
    setBusy(store);
    try {
      await api.settings.connectStore(store);
      toast(`${SOURCE_LABELS[store]} verbunden`);
      invalidateQueries();
    } catch (error: unknown) {
      toast(errorMessage(error), "danger");
    } finally {
      setBusy(null);
    }
  }

  return (
    <Panel title="Konten & Stores" description="Verbinde deine Store-Konten, um Spiele und Achievements zu importieren.">
      {query.status === "loading" && <SettingsSkeleton />}
      {query.status === "error" && <ErrorState message={query.error} onRetry={query.retry} />}
      {query.status === "success" && (
        <ul className="rounded-lg border border-border">
          {query.data.map((account) => (
            <li key={account.store} className="flex items-center gap-4 border-b border-border px-4 py-4 last:border-b-0">
              {/* Neutral monogram instead of store logos */}
              <span aria-hidden className="flex size-10 items-center justify-center rounded-md border border-border bg-sidebar text-sm font-semibold text-fg-muted">
                {SOURCE_LABELS[account.store].charAt(0)}
              </span>
              <div className="min-w-0 flex-1">
                <p className="text-sm font-medium text-fg">{SOURCE_LABELS[account.store]}</p>
                <p className="truncate text-sm text-fg-muted">
                  {account.connected && account.accountName
                    ? `Verbunden als ${account.accountName}${account.lastSyncedAt ? ` · synchronisiert ${formatRelative(account.lastSyncedAt)}` : ""}`
                    : "Nicht verbunden"}
                </p>
              </div>
              <Button
                variant={account.connected ? "secondary" : "primary"}
                disabled={busy !== null || account.connected}
                onClick={() => void connect(account.store)}
              >
                {account.connected ? "Verbunden" : "Verbinden"}
              </Button>
            </li>
          ))}
        </ul>
      )}
      <p className="mt-4 text-xs text-fg-subtle">Die Store-Anbindung kommt mit dem Connector-Service. Cove speichert keine Passwörter – die Anmeldung erfolgt direkt beim jeweiligen Store.</p>
    </Panel>
  );
}

const MB = 1e6;

function DownloadsTab() {
  const { settings, error, retry, save } = useSettings();
  if (error) return <ErrorState message={error} onRetry={retry} />;
  if (!settings) return <SettingsSkeleton />;
  const d = settings.downloads;
  const set = (patch: Partial<AppSettings["downloads"]>) => {
    save((s) => ({ ...s, downloads: { ...s.downloads, ...patch } }));
  };
  const limited = d.bandwidthLimitBytesPerSecond !== null;

  return (
    <Panel title="Downloads">
      <div className="flex max-w-xl flex-col gap-8">
        <Field label="Zielordner" description="Neue Spiele werden hier installiert.">
          {(props) => (
            <Input
              {...props}
              spellCheck={false}
              value={d.directory}
              onChange={(e) => {
                set({ directory: e.target.value });
              }}
            />
          )}
        </Field>
        <Group>
          <SettingRow title="Bandbreite begrenzen" description="Lässt Platz für andere Dinge im Netzwerk.">
            <Switch
              label="Bandbreite begrenzen"
              checked={limited}
              onChange={(on) => {
                set({ bandwidthLimitBytesPerSecond: on ? 10 * MB : null });
              }}
            />
          </SettingRow>
          {limited && (
            <SettingRow title="Limit" description="In Megabyte pro Sekunde.">
              <div className="flex items-center gap-2">
                <Input
                  type="number"
                  min={1}
                  max={1000}
                  aria-label="Limit in MB/s"
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
          <SettingRow title="Gleichzeitige Downloads" description="Wie viele Spiele parallel geladen werden.">
            <Select
              className="w-24"
              aria-label="Gleichzeitige Downloads"
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

function PluginsTab() {
  const query = useQuery("plugins", api.settings.listPlugins);

  async function setEnabled(id: string, enabled: boolean): Promise<void> {
    query.update((plugins) => plugins.map((p) => (p.id === id ? { ...p, enabled } : p)));
    try {
      await api.settings.setPluginEnabled(id, enabled);
    } catch (error: unknown) {
      toast(errorMessage(error), "danger");
      query.retry();
    }
  }

  return (
    <Panel title="Plugins" description="Plugins erweitern Cove. Sie liegen als JavaScript-Module im Plugin-Ordner.">
      {query.status === "loading" && <SettingsSkeleton />}
      {query.status === "error" && <ErrorState message={query.error} onRetry={query.retry} />}
      {query.status === "success" && query.data.length === 0 && (
        <div className="rounded-lg border border-border">
          <EmptyState icon={Puzzle} title="Keine Plugins installiert" description="Lege ein Plugin in den Plugin-Ordner, damit es hier erscheint." compact />
        </div>
      )}
      {query.status === "success" && query.data.length > 0 && (
        <ul className="rounded-lg border border-border">
          {query.data.map((plugin) => (
            <li key={plugin.id} className="flex items-start gap-4 border-b border-border px-4 py-4 last:border-b-0">
              <span aria-hidden className="flex size-10 shrink-0 items-center justify-center rounded-md border border-border bg-sidebar text-fg-subtle">
                <Puzzle />
              </span>
              <div className="min-w-0 flex-1">
                <p className="text-sm font-medium text-fg">
                  {plugin.name} <span className="num ml-1 text-xs font-normal text-fg-subtle">v{plugin.version}</span>
                </p>
                <p className="mt-0.5 text-sm text-fg-muted">{plugin.description}</p>
                <p className="mt-1 text-xs text-fg-subtle">von {plugin.author}</p>
              </div>
              <Switch
                label={`${plugin.name} aktivieren`}
                checked={plugin.enabled}
                onChange={(on) => void setEnabled(plugin.id, on)}
              />
            </li>
          ))}
        </ul>
      )}
    </Panel>
  );
}

function AboutTab() {
  const query = useQuery("about", api.system.getAbout);

  return (
    <Panel title="Über Cove">
      {query.status === "loading" && <SettingsSkeleton />}
      {query.status === "error" && <ErrorState message={query.error} onRetry={query.retry} />}
      {query.status === "success" && (
        <div className="flex flex-col gap-8">
          <div>
            <p className="text-xl font-semibold tracking-tight text-fg">Cove</p>
            <p className="num mt-1 text-sm text-fg-muted">
              Version {query.data.appVersion} · {PLATFORM_LABELS[query.data.platform]}
            </p>
            <p className="mt-3 max-w-md text-sm text-fg-muted">Ein ruhiger Ort für alle deine Spiele – egal aus welchem Store.</p>
          </div>
          <div>
            <h3 className="mb-3 text-sm font-medium text-fg">Komponenten</h3>
            <dl className="m-0 max-w-md rounded-lg border border-border px-4 text-sm">
              {query.data.components.map((c) => (
                <div key={c.name} className="flex justify-between border-b border-border py-2.5 last:border-b-0">
                  <dt className="text-fg-muted">
                    {{ core: "Core (Rust)", connectors: "Connectors (C#)", recommender: "Recommender (Python)" }[c.name]}
                  </dt>
                  <dd className="num m-0 text-fg">{c.version}</dd>
                </div>
              ))}
            </dl>
          </div>
          <div>
            <Button
              icon={RefreshCw}
              onClick={() => {
                toast("Cove ist auf dem neuesten Stand.");
              }}
            >
              Nach Updates suchen
            </Button>
          </div>
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
  downloads: DownloadsTab,
  plugins: PluginsTab,
  about: AboutTab,
};

export function SettingsPage() {
  const { tab } = useParams();
  const current = TABS.find((t) => t.id === tab);
  if (!current) return <Navigate to="/settings/general" replace />;
  const PanelComponent = PANELS[current.id];

  return (
    <Page className="max-w-5xl">
      <h1 className="mb-8 text-xl font-semibold tracking-tight text-fg">Settings</h1>
      <div className="grid grid-cols-[200px_1fr] gap-12">
        <nav aria-label="Einstellungen">
          <ul className="flex flex-col gap-0.5">
            {TABS.map((t) => (
              <li key={t.id}>
                <NavLink
                  to={`/settings/${t.id}`}
                  className={({ isActive }) =>
                    cx(
                      "flex h-8 items-center rounded-md px-3 text-sm transition-colors duration-150 ease-out",
                      isActive ? "bg-active font-medium text-fg" : "text-fg-muted hover:bg-hover hover:text-fg",
                    )
                  }
                >
                  {t.label}
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
