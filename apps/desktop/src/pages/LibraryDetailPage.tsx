import type { Achievement, CompatibilityTool, EnvVar, LaunchOptions, LibraryItem } from "@cove/shared";
import { ArrowLeft, Compass, Library, Link2, Lock, Plus, Star, Trash2, Trophy } from "lucide-react";
import { useState, type SyntheticEvent, type ReactNode } from "react";
import { useNavigate, useParams, useSearchParams } from "react-router";
import { api, CoveError } from "../api/index.ts";
import { MatchDialog } from "../features/MatchDialog.tsx";
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
import { formatBytes, formatDate, formatPlaytime, formatRelative } from "../lib/format.ts";
import { COMPAT_LABELS, SOURCE_LABELS } from "../lib/labels.ts";
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
  const stats = useQuery(`playtime/${item.game.id}`, () => api.library.getPlaytimeStats(item.game.id));
  const { entry, game } = item;

  return (
    <div className="grid grid-cols-1 gap-10 xl:grid-cols-[1fr_320px]">
      <section aria-label="Spielzeit">
        <h2 className="text-base font-semibold text-fg">Spielzeit der letzten 14 Tage</h2>
        {stats.status === "loading" && (
          <LoadingRegion label="Lädt Spielzeit">
            <Skeleton className="mt-4 h-40" />
          </LoadingRegion>
        )}
        {stats.status === "error" && <ErrorState message={stats.error} onRetry={stats.retry} />}
        {stats.status === "success" && (
          <>
            <p className="num mt-1 text-sm text-fg-muted">
              {formatPlaytime(stats.data.last14DaysMinutes)} · {stats.data.sessionCount} Sitzungen insgesamt
            </p>
            <div className="mt-4">
              <PlaytimeChart days={stats.data.days} title={`Spielzeit pro Tag für ${game.title}`} />
            </div>
          </>
        )}
      </section>
      <section aria-label="Informationen">
        <h2 className="mb-3 text-base font-semibold text-fg">Informationen</h2>
        <dl className="m-0 rounded-lg border border-border px-4 text-sm">
          {[
            ["Quelle", SOURCE_LABELS[entry.source]],
            ["Status", entry.installed ? "Installiert" : "Nicht installiert"],
            ["Speicherort", entry.installPath ?? "—"],
            ["Größe", entry.installSizeBytes === null ? "—" : formatBytes(entry.installSizeBytes)],
            ["Hinzugefügt", formatDate(entry.addedAt)],
            ["Entwickler", game.developer],
          ].map(([label, value]) => (
            <div key={label} className="flex justify-between gap-4 border-b border-border py-2.5 last:border-b-0">
              <dt className="text-fg-subtle">{label}</dt>
              <dd className="m-0 min-w-0 truncate text-right text-fg" title={value}>
                {value}
              </dd>
            </div>
          ))}
        </dl>
      </section>
    </div>
  );
}

type AchievementFilter = "all" | "unlocked" | "locked";

function AchievementRow({ achievement }: { achievement: Achievement }) {
  const unlocked = achievement.unlockedAt !== null;
  const secret = achievement.hidden && !unlocked;
  return (
    <li className="flex items-center gap-4 border-b border-border py-3 last:border-b-0">
      <div
        className={cx(
          "flex size-10 shrink-0 items-center justify-center rounded-md border border-border",
          unlocked ? "bg-accent-subtle text-accent-text" : "bg-sidebar text-fg-subtle",
        )}
      >
        {unlocked ? <Trophy aria-hidden /> : <Lock aria-hidden />}
      </div>
      <div className="min-w-0 flex-1">
        <p className={cx("truncate text-sm font-medium", unlocked ? "text-fg" : "text-fg-muted")}>
          {secret ? "Verstecktes Achievement" : achievement.title}
        </p>
        <p className="truncate text-xs text-fg-subtle">{secret ? "Wird nach dem Freischalten angezeigt." : achievement.description}</p>
      </div>
      {achievement.rarityPercent !== null && (
        <span className="num hidden shrink-0 text-xs text-fg-subtle md:inline">
          {achievement.rarityPercent.toLocaleString("de-DE")} % der Spieler
        </span>
      )}
      <span className="w-32 shrink-0 text-right text-xs text-fg-muted">
        {achievement.unlockedAt ? (
          <time dateTime={achievement.unlockedAt}>{formatRelative(achievement.unlockedAt)}</time>
        ) : (
          <span className="sr-only">Gesperrt</span>
        )}
      </span>
    </li>
  );
}

function Achievements({ item }: { item: LibraryItem }) {
  const query = useQuery(`achievements/${item.game.id}`, () => api.library.getAchievements(item.game.id));
  const [filter, setFilter] = useState<AchievementFilter>("all");

  if (query.status === "loading") {
    return (
      <LoadingRegion label="Lädt Achievements">
        {Array.from({ length: 6 }, (_, i) => (
          <div key={i} className="flex items-center gap-4 border-b border-border py-3">
            <Skeleton className="size-10" />
            <div className="flex-1">
              <Skeleton className="h-4 w-1/3" />
              <Skeleton className="mt-1.5 h-3 w-1/2" />
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
        title="Keine Achievements"
        description="Für dieses Spiel gibt es keine Achievements – oder die Quelle stellt keine bereit."
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
            <span className="font-medium text-fg">{unlocked.length}</span> von {query.data.length} freigeschaltet
          </p>
          <ProgressBar value={unlocked.length} max={query.data.length} label="Achievement-Fortschritt" className="mt-2" />
        </div>
        <Segmented
          label="Achievements filtern"
          value={filter}
          onChange={setFilter}
          options={[
            { value: "all", label: "Alle" },
            { value: "unlocked", label: "Freigeschaltet" },
            { value: "locked", label: "Gesperrt" },
          ]}
        />
      </div>
      {shown.length === 0 ? (
        <p className="py-8 text-center text-sm text-fg-muted">
          {filter === "locked" ? "Alles freigeschaltet. Glückwunsch!" : "Noch nichts freigeschaltet."}
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

function RemoveFromLibrary({ item }: { item: LibraryItem }) {
  const navigate = useNavigate();
  const [confirming, setConfirming] = useState(false);
  const [busy, setBusy] = useState(false);

  async function remove(): Promise<void> {
    setBusy(true);
    try {
      await api.library.removeGame(item.game.id);
      toast(`„${item.game.title}“ wurde aus der Bibliothek entfernt`);
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
      <h3 className="text-sm font-medium text-fg">Aus Bibliothek entfernen</h3>
      <p className="mt-1 text-sm text-fg-muted">
        Entfernt den Eintrag aus Cove. Dateien auf der Festplatte bleiben unberührt, deine Spielzeit bleibt gespeichert.
      </p>
      <Button
        variant="danger"
        icon={Trash2}
        className="mt-4"
        onClick={() => {
          setConfirming(true);
        }}
      >
        Entfernen
      </Button>
      <Dialog
        open={confirming}
        onClose={() => {
          setConfirming(false);
        }}
        title={`„${item.game.title}“ entfernen?`}
        description="Das Spiel verschwindet aus deiner Bibliothek. Installierte Dateien werden nicht gelöscht."
        footer={
          <>
            <Button
              onClick={() => {
                setConfirming(false);
              }}
            >
              Abbrechen
            </Button>
            <Button variant="danger" disabled={busy} onClick={() => void remove()}>
              Entfernen
            </Button>
          </>
        }
      >
        {null}
      </Dialog>
    </section>
  );
}

function LaunchSettings({ item }: { item: LibraryItem }) {
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
      toast("Starteinstellungen gespeichert");
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
          label="Startdatei"
          description={
            item.entry.executablePath
              ? "Die Datei, die Cove startet."
              : "Noch nicht festgelegt – trage die .exe, das Binary, AppImage oder die .desktop-Datei ein."
          }
          error={pathError}
        >
          {(props) => (
            <Input
              {...props}
              spellCheck={false}
              placeholder={isLinux ? "/home/du/Games/spiel/spiel.x86_64" : "C:\\Games\\Spiel\\spiel.exe"}
              value={executable}
              onChange={(e) => {
                setExecutable(e.target.value);
                setPathError(null);
              }}
            />
          )}
        </Field>
      )}

      <Field label="Startparameter" description="Werden beim Start an das Spiel übergeben, z. B. -fullscreen -novid.">
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
        <Field label="Kompatibilität (Linux)" description="Wie Windows-Spiele unter Linux ausgeführt werden.">
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

      <fieldset className="m-0 flex flex-col gap-2 border-0 p-0">
        <legend className="text-sm font-medium text-fg">Umgebungsvariablen</legend>
        <p className="text-xs text-fg-muted">Zum Beispiel DXVK_HUD=fps oder PROTON_ENABLE_NVAPI=1.</p>
        {draft.env.length > 0 && (
          <ul className="mt-1 flex flex-col gap-2">
            {draft.env.map((env, index) => (
              <li key={index} className="flex items-center gap-2">
                <Input
                  aria-label={`Name der Variable ${String(index + 1)}`}
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
                  aria-label={`Wert der Variable ${String(index + 1)}`}
                  placeholder="Wert"
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
                  label={`Variable ${env.key || String(index + 1)} entfernen`}
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
            Variable hinzufügen
          </Button>
        </div>
      </fieldset>

      <div className="flex gap-2 border-t border-border pt-6">
        <Button type="submit" variant="primary" disabled={!dirty || saving}>
          Speichern
        </Button>
        <Button
          disabled={!dirty || saving}
          onClick={() => {
            setDraft(item.entry.launchOptions);
            setExecutable(item.entry.executablePath ?? "");
            setPathError(null);
          }}
        >
          Verwerfen
        </Button>
      </div>
    </form>
    <RemoveFromLibrary item={item} />
    </div>
  );
}

function HeaderSkeleton() {
  return (
    <LoadingRegion label="Spiel wird geladen" className="flex gap-8">
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
  const { gameId = "" } = useParams();
  const [params, setParams] = useSearchParams();
  const query = useQuery(`library/${gameId}`, () => api.library.getLibraryItem(gameId));
  const tabParam = params.get("tab");
  const tab: Tab = TABS.find((t) => t === tabParam) ?? "overview";
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
          title="Nicht in deiner Bibliothek"
          description="Dieses Spiel gehört (noch) nicht zu deiner Bibliothek."
          actions={
            <>
              <ButtonLink to="/library">Zur Bibliothek</ButtonLink>
              <ButtonLink to={`/catalogue/${gameId}`} icon={Compass}>
                Im Katalog ansehen
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
                {!query.data.entry.installed && <Badge>Nicht installiert</Badge>}
              </div>
              <h1 className="mt-3 text-xl font-semibold tracking-tight text-fg">{query.data.game.title}</h1>
              <p className="mt-1 text-sm text-fg-muted">{query.data.game.summary}</p>
              <div className="mt-6 flex items-center gap-2">
                <PlayButton item={query.data} />
                <Button
                  aria-pressed={query.data.entry.favorite}
                  icon={Star}
                  onClick={() => {
                    if (query.data) void toggleFavorite(query.data);
                  }}
                  className={query.data.entry.favorite ? "[&_svg]:fill-current [&_svg]:text-accent-text" : undefined}
                >
                  {query.data.entry.favorite ? "Favorit" : "Zu Favoriten"}
                </Button>
                {query.data.entry.catalogId && (
                  <ButtonLink to={`/catalogue/${query.data.entry.catalogId}`} variant="ghost">
                    Katalog-Seite
                  </ButtonLink>
                )}
                <Button
                  variant="ghost"
                  icon={Link2}
                  onClick={() => {
                    setMatchOpen(true);
                  }}
                >
                  {query.data.entry.catalogId ? "Zuordnung ändern" : "Im Katalog zuordnen"}
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
                Cove hat mehrere mögliche Treffer im Katalog gefunden. Welcher passt zu deinem Spiel?
              </p>
              <Button
                size="sm"
                variant="primary"
                onClick={() => {
                  setMatchOpen(true);
                }}
              >
                Zuordnung prüfen
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
            <Stat label="Gesamtspielzeit">{formatPlaytime(query.data.entry.playtimeMinutes)}</Stat>
            <Stat label="Zuletzt gespielt">{query.data.entry.lastPlayedAt ? formatRelative(query.data.entry.lastPlayedAt) : "Noch nie"}</Stat>
            <Stat label="Achievements">
              {query.data.entry.achievements.total > 0
                ? `${String(query.data.entry.achievements.unlocked)} / ${String(query.data.entry.achievements.total)}`
                : "—"}
            </Stat>
            <Stat label="Größe">{query.data.entry.installSizeBytes === null ? "—" : formatBytes(query.data.entry.installSizeBytes)}</Stat>
          </div>

          <div className="mt-10">
            <Tabs
              label="Spielbereiche"
              value={tab}
              onChange={(next) => {
                setParams(next === "overview" ? {} : { tab: next }, { replace: true });
              }}
              items={[
                { value: "overview", label: "Übersicht" },
                {
                  value: "achievements",
                  label: "Achievements",
                  ...(query.data.entry.achievements.total > 0
                    ? { meta: `${String(query.data.entry.achievements.unlocked)}/${String(query.data.entry.achievements.total)}` }
                    : {}),
                },
                { value: "settings", label: "Einstellungen" },
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
