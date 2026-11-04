import type { ConnectableStore, ConnectorLoginStart, LocalStoreGame, StoreAccount, SyncProgress } from "@cove/shared";
import { ExternalLink, HardDrive, LogIn, RefreshCw, Trophy } from "lucide-react";
import { useEffect, useId, useState } from "react";
import { api } from "../../api/index.ts";
import { Button } from "../../components/ui/Button.tsx";
import { Dialog } from "../../components/ui/Dialog.tsx";
import { Badge, EmptyState, ErrorState, LoadingRegion, ProgressBar, Skeleton } from "../../components/ui/Feedback.tsx";
import { Input, Switch } from "../../components/ui/Field.tsx";
import { useQuery } from "../../hooks/useQuery.ts";
import { formatRelative } from "../../lib/format.ts";
import { SOURCE_LABELS } from "../../lib/labels.ts";
import { invalidateQueries } from "../../stores/query.ts";
import { errorMessage, toast } from "../../stores/toasts.ts";
import { Panel, SettingsSkeleton } from "./parts.tsx";

const PHASE_LABELS: Record<SyncProgress["phase"], string> = {
  library: "Bibliothek wird importiert",
  matching: "Katalogdaten werden ergänzt",
  achievements: "Achievements werden gelesen",
};

function statusText(account: StoreAccount): string {
  if (account.status === "needsReauth") return "Die Anmeldung ist abgelaufen.";
  if (account.status === "disconnected") {
    return account.gameCount > 0 ? `Nicht verbunden · ${String(account.gameCount)} Spiele in der Bibliothek` : "Nicht verbunden";
  }
  const parts = [`Verbunden als ${account.accountName ?? "unbekannt"}`, `${String(account.gameCount)} Spiele`];
  if (account.lastSyncedAt) parts.push(`synchronisiert ${formatRelative(account.lastSyncedAt)}`);
  return parts.join(" · ");
}

/** Waiting for the store login; Epic additionally offers the code login. */
function PendingLogin({ pending, onCancel }: { pending: ConnectorLoginStart; onCancel: () => void }) {
  const [code, setCode] = useState("");
  const [busy, setBusy] = useState(false);
  const codeId = useId();
  const name = SOURCE_LABELS[pending.store];

  async function submit(): Promise<void> {
    setBusy(true);
    try {
      await api.accounts.completeLogin(pending.store, code);
      toast(`Mit ${name} verbunden`);
      invalidateQueries();
    } catch (error: unknown) {
      toast(errorMessage(error), "danger");
    } finally {
      setBusy(false);
    }
  }

  return (
    <div className="mt-3 rounded-md border border-border bg-sidebar px-4 py-3">
      <div className="flex items-center justify-between gap-4">
        <p role="status" className="text-sm text-fg-muted">
          {pending.mode === "browser"
            ? `Melde dich im Browser bei ${name} an. Cove wartet auf die Rückmeldung.`
            : `Melde dich im geöffneten Fenster bei ${name} an.`}
        </p>
        <Button size="sm" variant="ghost" onClick={onCancel}>
          Abbrechen
        </Button>
      </div>
      {pending.fallbackUrl && (
        <details className="mt-3 border-t border-border pt-3">
          <summary className="cursor-pointer text-sm font-medium text-fg">Alternativer Login</summary>
          <p className="mt-2 text-sm text-fg-muted">
            Klappt das Fenster nicht? Öffne die Anmeldeseite im Browser, melde dich an und kopiere den angezeigten Code
            (den Wert hinter „authorizationCode“) hierher.
          </p>
          <div className="mt-3 flex flex-wrap items-center gap-2">
            <Button
              size="sm"
              icon={ExternalLink}
              onClick={() => {
                if (pending.fallbackUrl) void api.system.openExternal(pending.fallbackUrl);
              }}
            >
              Anmeldeseite öffnen
            </Button>
            <label htmlFor={codeId} className="sr-only">
              Anmeldecode
            </label>
            <Input
              id={codeId}
              value={code}
              placeholder="Anmeldecode"
              autoComplete="off"
              spellCheck={false}
              className="w-56"
              onChange={(e) => {
                setCode(e.target.value);
              }}
            />
            <Button size="sm" variant="primary" disabled={busy || code.trim() === ""} onClick={() => void submit()}>
              Anmelden
            </Button>
          </div>
        </details>
      )}
    </div>
  );
}

function LogoutDialog({ account, onClose }: { account: StoreAccount | null; onClose: () => void }) {
  const [keepGames, setKeepGames] = useState(true);
  const [busy, setBusy] = useState(false);
  const name = account ? SOURCE_LABELS[account.store] : "";

  async function confirm(): Promise<void> {
    if (!account) return;
    setBusy(true);
    try {
      await api.accounts.logout(account.store, keepGames);
      toast(`Von ${name} abgemeldet`);
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
      open={account !== null}
      onClose={onClose}
      title={`Von ${name} abmelden?`}
      description="Cove löscht die gespeicherte Anmeldung aus dem Schlüsselbund deines Systems."
      footer={
        <>
          <Button onClick={onClose}>Abbrechen</Button>
          <Button variant="danger" disabled={busy} onClick={() => void confirm()}>
            Abmelden
          </Button>
        </>
      }
    >
      <div className="flex items-center justify-between gap-4">
        <div>
          <p className="text-sm font-medium text-fg" id="keep-games-label">
            Importierte Spiele behalten
          </p>
          <p className="text-sm text-fg-muted">Spielzeit und Einstellungen bleiben erhalten.</p>
        </div>
        <Switch checked={keepGames} onChange={setKeepGames} label="Importierte Spiele behalten" />
      </div>
    </Dialog>
  );
}

function AccountRow({
  account,
  pending,
  progress,
  onLogin,
  onCancel,
  onLogout,
}: {
  account: StoreAccount;
  pending: ConnectorLoginStart | undefined;
  progress: SyncProgress | undefined;
  onLogin: () => void;
  onCancel: () => void;
  onLogout: () => void;
}) {
  const [busy, setBusy] = useState(false);
  const name = SOURCE_LABELS[account.store];
  const syncing = progress !== undefined && !progress.finished;

  async function run(action: () => Promise<string>): Promise<void> {
    setBusy(true);
    try {
      toast(await action());
    } catch (error: unknown) {
      toast(errorMessage(error), "danger");
    } finally {
      setBusy(false);
      invalidateQueries();
    }
  }

  const importNow = () =>
    run(async () => {
      const s = await api.accounts.importLibrary(account.store);
      return s.added > 0 ? `${String(s.added)} neue Spiele von ${name} importiert` : `${name}: Bibliothek ist aktuell`;
    });
  const syncAchievements = () =>
    run(async () => {
      const s = await api.accounts.syncAchievements(account.store);
      return s.newlyUnlocked > 0
        ? `${String(s.newlyUnlocked)} neue Achievements aus ${String(s.games)} Spielen`
        : `Achievements aus ${String(s.games)} Spielen sind aktuell`;
    });

  return (
    <li className="border-b border-border px-4 py-4 last:border-b-0">
      <div className="flex items-center gap-4">
        {/* Neutral monogram instead of store logos */}
        <span aria-hidden className="flex size-10 shrink-0 items-center justify-center rounded-md border border-border bg-sidebar text-sm font-semibold text-fg-muted">
          {name.charAt(0)}
        </span>
        <div className="min-w-0 flex-1">
          <p className="text-sm font-medium text-fg">{name}</p>
          <p className={account.status === "needsReauth" ? "truncate text-sm text-danger" : "truncate text-sm text-fg-muted"}>
            {statusText(account)}
          </p>
        </div>
        <div className="flex shrink-0 items-center gap-2">
          {account.status === "disconnected" && (
            <Button variant="primary" icon={LogIn} disabled={pending !== undefined} onClick={onLogin}>
              Anmelden
            </Button>
          )}
          {account.status === "needsReauth" && (
            <Button variant="primary" icon={LogIn} disabled={pending !== undefined} onClick={onLogin}>
              Erneut anmelden
            </Button>
          )}
          {account.status === "connected" && (
            <>
              {account.store === "steam" && (
                <Button
                  iconOnly
                  icon={Trophy}
                  label="Achievements abgleichen"
                  disabled={busy || syncing}
                  onClick={() => void syncAchievements()}
                />
              )}
              <Button icon={RefreshCw} disabled={busy || syncing} onClick={() => void importNow()}>
                Synchronisieren
              </Button>
            </>
          )}
          {account.connected && (
            <Button variant="ghost" onClick={onLogout}>
              Abmelden
            </Button>
          )}
        </div>
      </div>
      {account.problem && (
        <p className="mt-3 text-sm text-fg-muted">
          {account.problem.message}{" "}
          {account.problem.helpUrl && (
            <button
              type="button"
              className="font-medium text-accent-text hover:underline"
              onClick={() => {
                if (account.problem?.helpUrl) void api.system.openExternal(account.problem.helpUrl);
              }}
            >
              Privatsphäre-Einstellungen öffnen
            </button>
          )}
        </p>
      )}
      {pending && <PendingLogin pending={pending} onCancel={onCancel} />}
      {syncing && (
        <div className="mt-3">
          <p className="num text-xs text-fg-muted" role="status">
            {PHASE_LABELS[progress.phase]} · {progress.done} / {progress.total}
          </p>
          <ProgressBar value={progress.done} max={progress.total} label={PHASE_LABELS[progress.phase]} className="mt-1.5" />
        </div>
      )}
    </li>
  );
}

function LocalGames() {
  const [state, setState] = useState<
    { status: "idle" } | { status: "loading" } | { status: "error"; error: string } | { status: "done"; games: LocalStoreGame[] }
  >({ status: "idle" });
  const [importing, setImporting] = useState<ConnectableStore | null>(null);

  async function scan(): Promise<void> {
    setState({ status: "loading" });
    try {
      setState({ status: "done", games: await api.accounts.detectLocal(null) });
    } catch (error: unknown) {
      setState({ status: "error", error: errorMessage(error) });
    }
  }

  async function importStore(store: ConnectableStore): Promise<void> {
    setImporting(store);
    try {
      const summary = await api.accounts.importLibrary(store);
      toast(`${String(summary.added)} Spiele von ${SOURCE_LABELS[store]} hinzugefügt`);
      invalidateQueries();
      await scan();
    } catch (error: unknown) {
      toast(errorMessage(error), "danger");
    } finally {
      setImporting(null);
    }
  }

  const missing = state.status === "done" ? state.games.filter((g) => !g.inLibrary) : [];
  const stores = [...new Set(missing.map((g) => g.store))];

  return (
    <section aria-label="Auf diesem Computer" className="mt-10">
      <div className="flex items-center justify-between gap-4">
        <div>
          <h3 className="text-base font-semibold text-fg">Auf diesem Computer</h3>
          <p className="mt-1 text-sm text-fg-muted">Installierte Spiele aus Steam, GOG (Heroic, Galaxy) und Epic (Legendary, Heroic) finden, ganz ohne Anmeldung.</p>
        </div>
        <Button icon={HardDrive} disabled={state.status === "loading"} onClick={() => void scan()}>
          Suchen
        </Button>
      </div>
      <div className="mt-4">
        {state.status === "loading" && (
          <LoadingRegion label="Sucht installierte Spiele" className="flex flex-col gap-2">
            {[0, 1, 2].map((i) => (
              <Skeleton key={i} className="h-10" />
            ))}
          </LoadingRegion>
        )}
        {state.status === "error" && <ErrorState message={state.error} onRetry={() => void scan()} />}
        {state.status === "done" && state.games.length === 0 && (
          <div className="rounded-lg border border-border">
            <EmptyState compact icon={HardDrive} title="Keine installierten Spiele gefunden" description="Cove schaut in die Ordner von Steam, Heroic, Legendary und den Store-Launchern." />
          </div>
        )}
        {state.status === "done" && state.games.length > 0 && (
          <>
            <ul className="rounded-lg border border-border px-4">
              {state.games.map((game) => (
                <li key={`${game.store}:${game.storeGameId}`} className="flex items-center gap-4 border-b border-border py-3 last:border-b-0">
                  <div className="min-w-0 flex-1">
                    <p className="truncate text-sm font-medium text-fg">{game.title}</p>
                    <p className="truncate text-xs text-fg-subtle">{game.installPath}</p>
                  </div>
                  <Badge>{SOURCE_LABELS[game.store]}</Badge>
                  <span className="w-28 shrink-0 text-right text-xs text-fg-muted">{game.inLibrary ? "In der Bibliothek" : "Neu"}</span>
                </li>
              ))}
            </ul>
            {stores.length > 0 && (
              <div className="mt-4 flex flex-wrap gap-2">
                {stores.map((store) => (
                  <Button key={store} variant="primary" disabled={importing !== null} onClick={() => void importStore(store)}>
                    {`${String(missing.filter((g) => g.store === store).length)} von ${SOURCE_LABELS[store]} hinzufügen`}
                  </Button>
                ))}
              </div>
            )}
          </>
        )}
      </div>
    </section>
  );
}

export function AccountsTab() {
  const query = useQuery("accounts", api.accounts.listAccounts);
  const [pending, setPending] = useState<Partial<Record<ConnectableStore, ConnectorLoginStart>>>({});
  const [progress, setProgress] = useState<Partial<Record<ConnectableStore, SyncProgress>>>({});
  const [loggingOut, setLoggingOut] = useState<StoreAccount | null>(null);

  useEffect(
    () =>
      api.accounts.subscribeLoginFinished((event) => {
        setPending((p) => {
          return Object.fromEntries(Object.entries(p).filter(([k]) => k !== event.store));
        });
        const name = SOURCE_LABELS[event.store];
        if (event.account) toast(`Mit ${name} verbunden. Deine Bibliothek wird importiert.`);
        else if (event.error) toast(event.error.message, "danger");
        invalidateQueries();
      }),
    [],
  );
  useEffect(
    () =>
      api.accounts.subscribeSyncProgress((event) => {
        setProgress((p) => ({ ...p, [event.store]: event }));
        if (event.finished) invalidateQueries();
      }),
    [],
  );

  async function login(store: ConnectableStore): Promise<void> {
    try {
      const started = await api.accounts.startLogin(store);
      setPending((p) => ({ ...p, [store]: started }));
    } catch (error: unknown) {
      toast(errorMessage(error), "danger");
    }
  }

  function cancel(store: ConnectableStore): void {
    setPending((p) => {
      return Object.fromEntries(Object.entries(p).filter(([k]) => k !== store));
    });
    void api.accounts.cancelLogin(store);
  }

  return (
    <Panel title="Konten & Stores" description="Melde dich bei deinen Stores an, um Spiele, Spielzeit und Achievements zu importieren.">
      {query.status === "loading" && <SettingsSkeleton />}
      {query.status === "error" && <ErrorState message={query.error} onRetry={query.retry} />}
      {query.status === "success" && (
        <ul className="rounded-lg border border-border">
          {query.data.map((account) => (
            <AccountRow
              key={account.store}
              account={account}
              pending={pending[account.store]}
              progress={progress[account.store]}
              onLogin={() => void login(account.store)}
              onCancel={() => {
                cancel(account.store);
              }}
              onLogout={() => {
                setLoggingOut(account);
              }}
            />
          ))}
        </ul>
      )}
      <p className="mt-4 text-xs text-fg-subtle">
        Du meldest dich direkt beim Store an. Cove sieht und speichert keine Passwörter, nur die Zugangstokens des Stores, und
        die ausschließlich im Schlüsselbund deines Systems.
      </p>
      <LocalGames />
      <LogoutDialog
        account={loggingOut}
        onClose={() => {
          setLoggingOut(null);
        }}
      />
    </Panel>
  );
}
