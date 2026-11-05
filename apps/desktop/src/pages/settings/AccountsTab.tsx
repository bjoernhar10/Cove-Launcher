import { localized, messages, useMessages } from "../../i18n/index.ts";
import type { ConnectableStore, ConnectorLoginStart, LocalStoreGame, StoreAccount, SyncProgress } from "@cove/shared";
import { ExternalLink, HardDrive, LogIn, RefreshCw, Trophy } from "lucide-react";
import { useEffect, useId, useState } from "react";
import { api } from "../../api/index.ts";
import { Button } from "../../components/ui/Button.tsx";
import { Dialog } from "../../components/ui/Dialog.tsx";
import { Badge, EmptyState, ErrorState, LoadingRegion, ProgressBar, Skeleton } from "../../components/ui/Feedback.tsx";
import { Input, Switch } from "../../components/ui/Field.tsx";
import { StoreIcon } from "../../components/ui/StoreIcon.tsx";
import { useQuery } from "../../hooks/useQuery.ts";
import { formatRelative } from "../../lib/format.ts";
import { SOURCE_LABELS } from "../../lib/labels.ts";
import { invalidateQueries } from "../../stores/query.ts";
import { errorMessage, toast } from "../../stores/toasts.ts";
import { Panel, SettingsSkeleton } from "./parts.tsx";

const PHASE_LABELS: Record<SyncProgress["phase"], string> = localized((m) => m.accounts.phases);

function statusText(account: StoreAccount): string {
  const a = messages().accounts;
  if (account.status === "needsReauth") return a.expired;
  if (account.status === "disconnected") {
    return account.gameCount > 0 ? a.disconnectedWithGames(account.gameCount) : a.disconnected;
  }
  const parts = [a.connectedAs(account.accountName ?? messages().common.unknown), a.games(account.gameCount)];
  if (account.lastSyncedAt) parts.push(a.synced(formatRelative(account.lastSyncedAt)));
  return parts.join(" · ");
}

/** Waiting for the store login; Epic additionally offers the code login. */
function PendingLogin({ pending, onCancel }: { pending: ConnectorLoginStart; onCancel: () => void }) {
  const t = useMessages();
  const [code, setCode] = useState("");
  const [busy, setBusy] = useState(false);
  const codeId = useId();
  const name = SOURCE_LABELS[pending.store];

  async function submit(): Promise<void> {
    setBusy(true);
    try {
      await api.accounts.completeLogin(pending.store, code);
      toast(t.accounts.connected(name));
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
            ? t.accounts.signInBrowser(name)
            : t.accounts.signInWindow(name)}
        </p>
        <Button size="sm" variant="ghost" onClick={onCancel}>
          {t.common.cancel}
        </Button>
      </div>
      {pending.fallbackUrl && (
        <details className="mt-3 border-t border-border pt-3">
          <summary className="cursor-pointer text-sm font-medium text-fg">{t.accounts.altLogin}</summary>
          <p className="mt-2 text-sm text-fg-muted">
            {t.accounts.altLoginText}
          </p>
          <div className="mt-3 flex flex-wrap items-center gap-2">
            <Button
              size="sm"
              icon={ExternalLink}
              onClick={() => {
                if (pending.fallbackUrl) void api.system.openExternal(pending.fallbackUrl);
              }}
            >
              {t.accounts.openLoginPage}
            </Button>
            <label htmlFor={codeId} className="sr-only">
              {t.accounts.loginCode}
            </label>
            <Input
              id={codeId}
              value={code}
              placeholder={t.accounts.loginCode}
              autoComplete="off"
              spellCheck={false}
              className="w-56"
              onChange={(e) => {
                setCode(e.target.value);
              }}
            />
            <Button size="sm" variant="primary" disabled={busy || code.trim() === ""} onClick={() => void submit()}>
              {t.accounts.signIn}
            </Button>
          </div>
        </details>
      )}
    </div>
  );
}

function LogoutDialog({ account, onClose }: { account: StoreAccount | null; onClose: () => void }) {
  const t = useMessages();
  const [keepGames, setKeepGames] = useState(true);
  const [busy, setBusy] = useState(false);
  const name = account ? SOURCE_LABELS[account.store] : "";

  async function confirm(): Promise<void> {
    if (!account) return;
    setBusy(true);
    try {
      await api.accounts.logout(account.store, keepGames);
      toast(t.accounts.signedOut(name));
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
      title={t.accounts.signOutConfirm(name)}
      description={t.accounts.signOutText}
      footer={
        <>
          <Button onClick={onClose}>{t.common.cancel}</Button>
          <Button variant="danger" disabled={busy} onClick={() => void confirm()}>
            {t.accounts.signOut}
          </Button>
        </>
      }
    >
      <div className="flex items-center justify-between gap-4">
        <div>
          <p className="text-sm font-medium text-fg" id="keep-games-label">
            {t.accounts.keepGames}
          </p>
          <p className="text-sm text-fg-muted">{t.accounts.keepGamesHint}</p>
        </div>
        <Switch checked={keepGames} onChange={setKeepGames} label={t.accounts.keepGames} />
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
  const t = useMessages();
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
      return s.added > 0 ? t.accounts.imported(s.added, name) : t.accounts.libraryCurrent(name);
    });
  const syncAchievements = () =>
    run(async () => {
      const s = await api.accounts.syncAchievements(account.store);
      return s.newlyUnlocked > 0
        ? t.accounts.newAchievements(s.newlyUnlocked, s.games)
        : t.accounts.achievementsCurrent(s.games);
    });

  return (
    <li className="border-b border-border px-4 py-4 last:border-b-0">
      <div className="flex items-center gap-4">
        <span aria-hidden className="flex size-10 shrink-0 items-center justify-center rounded-md border border-border bg-sidebar text-fg-muted">
          <StoreIcon store={account.store} />
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
              {t.accounts.signIn}
            </Button>
          )}
          {account.status === "needsReauth" && (
            <Button variant="primary" icon={LogIn} disabled={pending !== undefined} onClick={onLogin}>
              {t.accounts.signInAgain}
            </Button>
          )}
          {account.status === "connected" && (
            <>
              {account.store === "steam" && (
                <Button
                  iconOnly
                  icon={Trophy}
                  label={t.accounts.syncAchievements}
                  disabled={busy || syncing}
                  onClick={() => void syncAchievements()}
                />
              )}
              <Button icon={RefreshCw} disabled={busy || syncing} onClick={() => void importNow()}>
                {t.accounts.sync}
              </Button>
            </>
          )}
          {account.connected && (
            <Button variant="ghost" onClick={onLogout}>
              {t.accounts.signOut}
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
              {t.accounts.openPrivacy}
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
          <ProgressBar value={progress.done} max={progress.total} label={PHASE_LABELS[progress.phase]} className="mt-2" />
        </div>
      )}
    </li>
  );
}

const ITCH_KEYS_URL = "https://itch.io/user/settings/api-keys";

/** itch.io uses a personal API key (stored only in the OS keyring). */
function ItchAccount() {
  const t = useMessages();
  const status = useQuery("itch/status", api.downloads.itchStatus);
  const [key, setKey] = useState("");
  const [busy, setBusy] = useState(false);
  const keyId = useId();

  async function run(action: () => Promise<unknown>, message: string): Promise<void> {
    setBusy(true);
    try {
      await action();
      setKey("");
      toast(message);
      invalidateQueries();
    } catch (error: unknown) {
      toast(errorMessage(error), "danger");
    } finally {
      setBusy(false);
    }
  }

  return (
    <div className="mt-3 rounded-lg border border-border px-4 py-4">
      <div className="flex items-center gap-4">
        <span aria-hidden className="flex size-10 shrink-0 items-center justify-center rounded-md border border-border bg-sidebar text-fg-muted">
          <StoreIcon store="itch" />
        </span>
        <div className="min-w-0 flex-1">
          <p className="text-sm font-medium text-fg">itch.io</p>
          <p className="truncate text-sm text-fg-muted">
            {status.status === "loading" && t.settings.checking}
            {status.status === "error" && status.error}
            {status.status === "success" &&
              (status.data.connected ? t.accounts.connectedAs(status.data.userName ?? t.common.unknown) : t.accounts.itchDisconnected)}
          </p>
        </div>
        {status.status === "success" && status.data.connected && (
          <Button variant="ghost" disabled={busy} onClick={() => void run(api.downloads.clearItchApiKey, t.accounts.itchRemoved)}>
            {t.accounts.disconnect}
          </Button>
        )}
      </div>
      {status.status === "success" && !status.data.connected && (
        <form
          className="mt-3 flex flex-wrap items-center gap-2"
          onSubmit={(e) => {
            e.preventDefault();
            void run(() => api.downloads.setItchApiKey(key), t.accounts.connected("itch.io"));
          }}
        >
          <label htmlFor={keyId} className="sr-only">
            {t.accounts.itchKey}
          </label>
          <Input
            id={keyId}
            type="password"
            autoComplete="off"
            placeholder={t.accounts.apiKey}
            className="w-72"
            value={key}
            onChange={(e) => {
              setKey(e.target.value);
            }}
          />
          <Button type="submit" variant="primary" disabled={busy || key.trim() === ""}>
            {t.accounts.connect}
          </Button>
          <Button
            type="button"
            variant="ghost"
            icon={ExternalLink}
            onClick={() => {
              void api.system.openExternal(ITCH_KEYS_URL);
            }}
          >
            {t.accounts.createKey}
          </Button>
        </form>
      )}
    </div>
  );
}

function LocalGames() {
  const t = useMessages();
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
      toast(t.accounts.addedFrom(summary.added, SOURCE_LABELS[store]));
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
    <section aria-label={t.accounts.thisComputer} className="mt-10">
      <div className="flex items-center justify-between gap-4">
        <div>
          <h3 className="text-base font-semibold text-fg">{t.accounts.thisComputer}</h3>
          <p className="mt-1 text-sm text-fg-muted">{t.accounts.thisComputerHint}</p>
        </div>
        <Button icon={HardDrive} disabled={state.status === "loading"} onClick={() => void scan()}>
          {t.itch.search}
        </Button>
      </div>
      <div className="mt-4">
        {state.status === "loading" && (
          <LoadingRegion label={t.accounts.searchingLocal} className="flex flex-col gap-2">
            {[0, 1, 2].map((i) => (
              <Skeleton key={i} className="h-10" />
            ))}
          </LoadingRegion>
        )}
        {state.status === "error" && <ErrorState message={state.error} onRetry={() => void scan()} />}
        {state.status === "done" && state.games.length === 0 && (
          <div className="rounded-lg border border-border">
            <EmptyState compact icon={HardDrive} title={t.accounts.noLocal} description={t.accounts.noLocalHint} />
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
                  <span className="w-28 shrink-0 text-right text-xs text-fg-muted">{game.inLibrary ? t.accounts.inLibrary : t.accounts.new}</span>
                </li>
              ))}
            </ul>
            {stores.length > 0 && (
              <div className="mt-4 flex flex-wrap gap-2">
                {stores.map((store) => (
                  <Button key={store} variant="primary" disabled={importing !== null} onClick={() => void importStore(store)}>
                    {t.accounts.addFrom(missing.filter((g) => g.store === store).length, SOURCE_LABELS[store])}
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
  const t = useMessages();
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
        if (event.account) toast(messages().accounts.connectedImporting(name));
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
    <Panel title={t.settings.tabs.accounts} description={t.accounts.description}>
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
      <ItchAccount />
      <p className="mt-4 text-xs text-fg-subtle">
        {t.accounts.privacyNote}
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
