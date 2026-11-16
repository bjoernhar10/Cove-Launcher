import type { DriveUsage, LibraryItem, StorageGame } from "@cove/shared";
import { HardDrive, PackageMinus } from "lucide-react";
import { useState } from "react";
import { Link } from "react-router";
import { api } from "../api/index.ts";
import { GameCover } from "../components/game/GameCover.tsx";
import { Button, ButtonLink } from "../components/ui/Button.tsx";
import { Dialog } from "../components/ui/Dialog.tsx";
import { EmptyState, ErrorState, LoadingRegion, Skeleton } from "../components/ui/Feedback.tsx";
import { Select } from "../components/ui/Field.tsx";
import { Page, PageHeader, Section } from "../components/ui/Page.tsx";
import { useQuery } from "../hooks/useQuery.ts";
import { useMessages } from "../i18n/index.ts";
import { formatBytes, formatRelative } from "../lib/format.ts";
import { invalidateQueries } from "../stores/query.ts";
import { errorMessage, toast } from "../stores/toasts.ts";

type Sort = "size" | "played";

interface Row {
  item: LibraryItem;
  storage: StorageGame;
}

/** Share of `total` in percent, for widths. */
function percent(part: number, total: number): string {
  return `${String(total > 0 ? Math.max(0, Math.min(100, (part / total) * 100)) : 0)}%`;
}

/** One drive: games, everything else and free space in one thin bar. */
function Drive({ drive }: { drive: DriveUsage }) {
  const t = useMessages();
  const used = Math.max(0, drive.totalBytes - drive.freeBytes);
  const other = Math.max(0, used - drive.gamesBytes);
  return (
    <li className="flex flex-col gap-2 border-b border-border py-4 last:border-b-0">
      <div className="flex items-baseline justify-between gap-4">
        <span className="flex min-w-0 items-center gap-2 text-sm font-medium text-fg">
          <HardDrive aria-hidden className="shrink-0 text-fg-muted" />
          <span className="truncate">{drive.mountPoint}</span>
        </span>
        <span className="num shrink-0 text-sm text-fg-muted">
          {t.storage.freeOf(formatBytes(drive.freeBytes), formatBytes(drive.totalBytes))}
        </span>
      </div>
      <div
        role="img"
        aria-label={t.storage.driveLabel(drive.mountPoint, formatBytes(drive.gamesBytes), formatBytes(other), formatBytes(drive.freeBytes))}
        className="flex h-2 gap-[2px] overflow-hidden rounded-full bg-active"
      >
        {drive.gamesBytes > 0 && <div className="h-full rounded-full bg-accent" style={{ width: percent(drive.gamesBytes, drive.totalBytes) }} />}
        {other > 0 && <div className="h-full rounded-full bg-fg-subtle" style={{ width: percent(other, drive.totalBytes) }} />}
      </div>
      <ul aria-hidden className="flex flex-wrap gap-x-4 gap-y-1 text-xs text-fg-muted">
        <li className="flex items-center gap-2">
          <span className="size-2 rounded-full bg-accent" />
          <span className="num">{t.storage.games(formatBytes(drive.gamesBytes))}</span>
        </li>
        <li className="flex items-center gap-2">
          <span className="size-2 rounded-full bg-fg-subtle" />
          <span className="num">{t.storage.other(formatBytes(other))}</span>
        </li>
        <li className="flex items-center gap-2">
          <span className="size-2 rounded-full bg-active" />
          <span className="num">{t.storage.free(formatBytes(drive.freeBytes))}</span>
        </li>
      </ul>
    </li>
  );
}

function GameRow({ row, showDrive, onUninstall }: { row: Row; showDrive: boolean; onUninstall: (item: LibraryItem) => void }) {
  const t = useMessages();
  const { item, storage } = row;
  const removable = item.entry.source === "steam" || item.entry.installedByCove;
  return (
    <li className="flex items-center gap-3 border-b border-border px-2 py-2 last:border-b-0">
      <Link to={`/library/${item.game.id}`} className="group flex min-w-0 flex-1 items-center gap-3 rounded-md">
        <GameCover game={item.game} aspect="square" showTitle={false} className="w-9 shrink-0 rounded-md" />
        <span className="min-w-0 flex-1">
          <span className="block truncate text-sm font-medium text-fg group-hover:underline">{item.game.title}</span>
          <span className="block truncate text-xs text-fg-muted">
            {item.entry.lastPlayedAt ? t.storage.lastPlayed(formatRelative(item.entry.lastPlayedAt)) : t.storage.neverPlayed}
            {showDrive && storage.mountPoint ? ` · ${storage.mountPoint}` : ""}
          </span>
        </span>
      </Link>
      <span className="num w-20 shrink-0 text-right text-sm text-fg">
        {storage.sizeBytes === null ? <span className="text-fg-subtle">{t.storage.unknownSize}</span> : formatBytes(storage.sizeBytes)}
      </span>
      <div className="w-10 shrink-0">
        {removable && (
          <Button
            variant="ghost"
            size="sm"
            iconOnly
            icon={PackageMinus}
            label={t.storage.uninstall(item.game.title)}
            onClick={() => {
              onUninstall(item);
            }}
          />
        )}
      </div>
    </li>
  );
}

function StorageSkeleton() {
  const t = useMessages();
  return (
    <LoadingRegion label={t.storage.loading} className="flex flex-col gap-3">
      <Skeleton className="h-24 rounded-lg" />
      <Skeleton className="h-64 rounded-lg" />
    </LoadingRegion>
  );
}

/** "Speicherplatz": what installed games take, per drive, with uninstall at hand. */
export function StoragePage() {
  const t = useMessages();
  const overview = useQuery("storage", api.library.storageOverview);
  const library = useQuery("library", api.library.listLibrary);
  const [sort, setSort] = useState<Sort>("size");
  const [confirming, setConfirming] = useState<LibraryItem | null>(null);
  const [busy, setBusy] = useState(false);

  async function uninstall(item: LibraryItem): Promise<void> {
    setBusy(true);
    try {
      const method = await api.library.uninstallGame(item.game.id);
      toast(method === "steam" ? t.game.uninstallInSteam(item.game.title) : t.game.uninstalled(item.game.title));
      invalidateQueries();
    } catch (error: unknown) {
      toast(errorMessage(error), "danger");
    } finally {
      setBusy(false);
      setConfirming(null);
    }
  }

  const header = <PageHeader title={t.storage.title} description={t.storage.subtitle} />;
  if (overview.status === "error" || library.status === "error") {
    return (
      <Page className="max-w-5xl">
        {header}
        <ErrorState
          message={overview.status === "error" ? overview.error : library.status === "error" ? library.error : ""}
          onRetry={() => {
            overview.retry();
            library.retry();
          }}
        />
      </Page>
    );
  }
  if (overview.status !== "success" || library.status !== "success") {
    return (
      <Page className="max-w-5xl">
        {header}
        <StorageSkeleton />
      </Page>
    );
  }

  const byId = new Map(library.data.map((item) => [item.game.id, item]));
  const rows: Row[] = overview.data.games.flatMap((storage) => {
    const item = byId.get(storage.gameId);
    return item ? [{ item, storage }] : [];
  });
  if (sort === "played") {
    // Longest unplayed first: never played, then oldest.
    rows.sort((a, b) => (a.item.entry.lastPlayedAt ?? "").localeCompare(b.item.entry.lastPlayedAt ?? ""));
  }
  const drives = overview.data.drives;

  return (
    <Page className="max-w-5xl">
      {header}
      {drives.length > 0 && (
        <Section title={t.storage.drives}>
          <ul className="rounded-lg border border-border px-4">
            {drives.map((drive) => (
              <Drive key={drive.mountPoint} drive={drive} />
            ))}
          </ul>
        </Section>
      )}
      <Section
        title={t.storage.installed}
        actions={
          rows.length > 1 ? (
            <Select
              className="w-48"
              aria-label={t.storage.sortLabel}
              value={sort}
              onChange={setSort}
              options={[
                { value: "size", label: t.storage.sortSize },
                { value: "played", label: t.storage.sortPlayed },
              ]}
            />
          ) : undefined
        }
      >
        {rows.length === 0 ? (
          <EmptyState
            compact
            icon={HardDrive}
            title={t.storage.emptyTitle}
            description={t.storage.emptyText}
            actions={<ButtonLink to="/library">{t.downloads.toLibrary}</ButtonLink>}
          />
        ) : (
          <ul className="rounded-lg border border-border">
            {rows.map((row) => (
              <GameRow key={row.item.game.id} row={row} showDrive={drives.length > 1} onUninstall={(item) => {
                if (item.entry.source === "steam") void uninstall(item);
                else setConfirming(item);
              }} />
            ))}
          </ul>
        )}
      </Section>
      <Dialog
        open={confirming !== null}
        onClose={() => {
          setConfirming(null);
        }}
        title={confirming ? t.game.uninstallConfirm(confirming.game.title) : ""}
        description={confirming ? t.game.uninstallConfirmText(confirming.entry.installPath ?? "") : undefined}
        footer={
          <>
            <Button
              onClick={() => {
                setConfirming(null);
              }}
            >
              {t.common.cancel}
            </Button>
            <Button variant="danger" disabled={busy} onClick={() => void (confirming && uninstall(confirming))}>
              {t.game.uninstall}
            </Button>
          </>
        }
      >
        {null}
      </Dialog>
    </Page>
  );
}
