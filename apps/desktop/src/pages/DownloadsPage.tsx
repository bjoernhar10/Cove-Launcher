import { localized, useMessages, type Messages } from "../i18n/index.ts";
import {
  closestCenter,
  DndContext,
  KeyboardSensor,
  PointerSensor,
  useSensor,
  useSensors,
  type DragEndEvent,
} from "@dnd-kit/core";
import {
  arrayMove,
  SortableContext,
  sortableKeyboardCoordinates,
  useSortable,
  verticalListSortingStrategy,
} from "@dnd-kit/sortable";
import { CSS } from "@dnd-kit/utilities";
import type { DownloadItem } from "@cove/shared";
import { CircleAlert, CircleCheck, CircleX, Download, ExternalLink, Gamepad2, GripVertical, Link2, Pause, Play, RotateCcw, ShieldCheck, X } from "lucide-react";
import { useState } from "react";
import { Link } from "react-router";
import { api } from "../api/index.ts";
import { GameCover } from "../components/game/GameCover.tsx";
import { Button, ButtonLink } from "../components/ui/Button.tsx";
import { EmptyState, ErrorState, LoadingRegion, ProgressBar, Skeleton } from "../components/ui/Feedback.tsx";
import { Page, PageHeader, Section } from "../components/ui/Page.tsx";
import { cx } from "../lib/cx.ts";
import { formatBytes, formatEta, formatPercent, formatRelative, formatSpeed } from "../lib/format.ts";
import { AddDownloadDialog } from "../features/AddDownloadDialog.tsx";
import { ItchDialog } from "../features/ItchDialog.tsx";
import { selectActive, selectHistory, selectQueued, useDownloads } from "../stores/downloads.ts";
import { errorMessage, toast } from "../stores/toasts.ts";

async function act(action: () => Promise<void>): Promise<void> {
  try {
    await action();
  } catch (error: unknown) {
    toast(errorMessage(error), "danger");
  }
  await useDownloads.getState().refresh();
}

/** The game's title; extras say which file they are. */
function downloadTitle(item: DownloadItem, t: Messages): string {
  return item.download.extra ? t.downloads.extraOf(item.game.title, item.download.extra) : item.game.title;
}

const ORIGIN_LABELS: Record<DownloadItem["download"]["origin"], string> = {
  direct: "Link",
  itch: "itch.io",
  steam: "Steam",
  gog: "GOG",
  epic: "Epic",
  amazon: "Amazon",
};

/** Store install running in the store's own client. */
function HandedOff({ item }: { item: DownloadItem }) {
  const t = useMessages();
  const { download, game } = item;
  const title = downloadTitle(item, t);
  const store = ORIGIN_LABELS[download.origin];
  return (
    <article className="flex items-center gap-5 rounded-lg border border-border p-4" aria-label={t.downloads.installation(title)}>
      <GameCover game={game} kind="header" aspect="landscape" showTitle={false} className="w-36 shrink-0 rounded-md" />
      <div className="min-w-0 flex-1">
        <Link to={`/library/${game.id}`} className="block truncate text-base font-medium text-fg hover:underline">
          {title}
        </Link>
        <p className="mt-1 flex items-center gap-2 text-sm text-fg-muted">
          <ExternalLink aria-hidden />
          {t.downloads.installingIn(download.origin === "steam" || navigator.userAgent.includes("Windows") ? store : `Heroic (${store})`)}
        </p>
        <p className="mt-1 text-xs text-fg-subtle">{t.downloads.detectsInstall}</p>
      </div>
      <Button
        iconOnly
        icon={X}
        label={t.downloads.stopTracking(title)}
        onClick={() => void act(() => api.downloads.cancelDownload(download.id))}
      />
    </article>
  );
}

function ActiveDownload({ item }: { item: DownloadItem }) {
  const t = useMessages();
  const { download, game } = item;
  const title = downloadTitle(item, t);
  if (download.status === "handedOff") return <HandedOff item={item} />;
  const paused = download.status === "paused";
  const busy = download.status === "verifying" || download.status === "extracting";
  const known = download.totalBytes > 0;
  const remaining = download.totalBytes - download.downloadedBytes;
  const eta = download.bytesPerSecond > 0 && known ? remaining / download.bytesPerSecond : Number.POSITIVE_INFINITY;

  return (
    <article className="flex items-center gap-5 rounded-lg border border-border p-4" aria-label={t.downloads.download(title)}>
      <GameCover game={game} kind="header" aspect="landscape" showTitle={false} className="w-36 shrink-0 rounded-md" />
      <div className="min-w-0 flex-1">
        <div className="flex items-baseline justify-between gap-4">
          <Link to={`/library/${game.id}`} className="truncate text-base font-medium text-fg hover:underline">
            {title}
          </Link>
          <span className="num shrink-0 text-sm font-medium text-fg">
            {busy ? formatPercent(1) : known ? formatPercent(download.downloadedBytes / download.totalBytes) : ""}
          </span>
        </div>
        <ProgressBar
          value={busy ? 1 : download.downloadedBytes}
          max={busy ? 1 : Math.max(download.totalBytes, 1)}
          label={t.home.progressOf(title)}
          tone={paused ? "muted" : "accent"}
          className="mt-3"
        />
        <p className="num mt-3 flex flex-wrap gap-x-4 gap-y-1 text-xs text-fg-muted">
          <span>
            {formatBytes(download.downloadedBytes)}
            {known && <> {t.downloads.of} {formatBytes(download.totalBytes)}</>}
          </span>
          {download.status === "verifying" && <span>{t.downloads.verifying}</span>}
          {download.status === "extracting" && <span>{t.downloads.extracting}</span>}
          {paused && <span>{t.downloads.paused}</span>}
          {download.status === "downloading" && (
            <>
              <span>{formatSpeed(download.bytesPerSecond)}</span>
              {known && <span>{t.downloads.remaining(formatEta(eta))}</span>}
            </>
          )}
          <span>{ORIGIN_LABELS[download.origin]}</span>
        </p>
      </div>
      <div className="flex shrink-0 gap-1">
        {busy ? null : paused ? (
          <Button iconOnly icon={Play} label={t.downloads.resume(title)} onClick={() => void act(() => api.downloads.resumeDownload(download.id))} />
        ) : (
          <Button iconOnly icon={Pause} label={t.downloads.pause(title)} onClick={() => void act(() => api.downloads.pauseDownload(download.id))} />
        )}
        <Button iconOnly icon={X} label={t.downloads.cancel(title)} onClick={() => void act(() => api.downloads.cancelDownload(download.id))} />
      </div>
    </article>
  );
}

function QueueRow({ item, position }: { item: DownloadItem; position: number }) {
  const t = useMessages();
  const { download, game } = item;
  const title = downloadTitle(item, t);
  const { attributes, listeners, setNodeRef, setActivatorNodeRef, transform, transition, isDragging } = useSortable({ id: download.id });
  return (
    <li
      ref={setNodeRef}
      style={{ transform: CSS.Translate.toString(transform), transition }}
      className={cx(
        "relative flex items-center gap-3 border-b border-border px-2 py-2 last:border-b-0",
        isDragging && "z-10 rounded-md border border-border-strong bg-raised",
      )}
    >
      <button
        ref={setActivatorNodeRef}
        type="button"
        aria-label={t.downloads.move(title)}
        className="inline-flex size-8 cursor-grab items-center justify-center rounded-md text-fg-subtle hover:bg-hover hover:text-fg active:cursor-grabbing"
        {...attributes}
        {...listeners}
      >
        <GripVertical aria-hidden />
      </button>
      <span className="num w-5 text-xs text-fg-subtle">{position}</span>
      <GameCover game={game} aspect="square" showTitle={false} className="w-9 shrink-0 rounded-md" />
      <span className="min-w-0 flex-1 truncate text-sm font-medium text-fg">{title}</span>
      <span className="num text-xs text-fg-muted">{formatBytes(download.totalBytes)}</span>
      <span className="w-14 text-xs text-fg-subtle">{ORIGIN_LABELS[download.origin]}</span>
      <Button
        variant="ghost"
        size="sm"
        iconOnly
        icon={X}
        label={t.downloads.removeFromQueue(title)}
        onClick={() => void act(() => api.downloads.cancelDownload(download.id))}
      />
    </li>
  );
}

function Queue({ items }: { items: DownloadItem[] }) {
  const t = useMessages();
  const [order, setOrder] = useState<string[] | null>(null);
  const sensors = useSensors(
    useSensor(PointerSensor, { activationConstraint: { distance: 4 } }),
    useSensor(KeyboardSensor, { coordinateGetter: sortableKeyboardCoordinates }),
  );
  // Optimistic order while the backend confirms; falls back to server order.
  const ids = items.map((i) => i.download.id);
  const effective = order?.length === ids.length && order.every((id) => ids.includes(id)) ? order : ids;
  const byId = new Map(items.map((i) => [i.download.id, i]));

  function onDragEnd(event: DragEndEvent): void {
    const { active, over } = event;
    if (!over || active.id === over.id) return;
    const next = arrayMove(effective, effective.indexOf(String(active.id)), effective.indexOf(String(over.id)));
    setOrder(next);
    void act(() => api.downloads.reorderQueue(next));
  }

  return (
    <DndContext
      sensors={sensors}
      collisionDetection={closestCenter}
      onDragEnd={onDragEnd}
      accessibility={{
        screenReaderInstructions: {
          draggable: t.downloads.dragInstructions,
        },
      }}
    >
      <SortableContext items={effective} strategy={verticalListSortingStrategy}>
        <ul className="rounded-lg border border-border" aria-label={t.downloads.queue}>
          {effective.map((id, index) => {
            const item = byId.get(id);
            return item ? <QueueRow key={id} item={item} position={index + 1} /> : null;
          })}
        </ul>
      </SortableContext>
    </DndContext>
  );
}

const HISTORY_STATUS = {
  completed: { icon: CircleCheck, className: "text-fg-muted" },
  failed: { icon: CircleAlert, className: "text-danger" },
  cancelled: { icon: CircleX, className: "text-fg-subtle" },
} as const;

const HISTORY_LABELS = localized((m) => m.downloads.history);

function HistoryRow({ item }: { item: DownloadItem }) {
  const t = useMessages();
  const { download, game } = item;
  const title = downloadTitle(item, t);
  const status = download.status === "failed" ? "failed" : download.status === "cancelled" ? "cancelled" : "completed";
  const meta = HISTORY_STATUS[status];
  const Icon = meta.icon;
  return (
    <li className="flex items-center gap-3 border-b border-border px-4 py-3 last:border-b-0">
      <GameCover game={game} aspect="square" showTitle={false} className="w-9 shrink-0 rounded-md" />
      <div className="min-w-0 flex-1">
        <Link to={`/library/${game.id}`} className="block truncate text-sm font-medium text-fg hover:underline">
          {title}
        </Link>
        <p className={cx("truncate text-xs", download.error ? "text-danger" : "text-fg-subtle")}>
          {download.error ?? (
            <>
              <span className="num">{formatBytes(download.totalBytes)}</span> · {ORIGIN_LABELS[download.origin]}
              {download.checksumVerified && (
                <span className="ml-2 inline-flex items-center gap-1 align-middle">
                  <ShieldCheck aria-hidden />
                  {t.downloads.checksumOk}
                </span>
              )}
            </>
          )}
        </p>
      </div>
      <span className={cx("flex items-center gap-2 text-xs", meta.className)}>
        <Icon aria-hidden />
        {HISTORY_LABELS[status]}
      </span>
      <span className="w-28 text-right text-xs text-fg-subtle">{download.finishedAt ? formatRelative(download.finishedAt) : ""}</span>
      {status === "failed" ? (
        <Button size="sm" icon={RotateCcw} onClick={() => void act(() => api.downloads.retryDownload(download.id))}>
          {t.downloads.retry}
        </Button>
      ) : (
        <span className="w-22" aria-hidden />
      )}
    </li>
  );
}

export function DownloadsPage() {
  const t = useMessages();
  const items = useDownloads((s) => s.items);
  const loaded = useDownloads((s) => s.loaded);
  const error = useDownloads((s) => s.error);
  const refresh = useDownloads((s) => s.refresh);
  const active = selectActive(items);
  const queued = selectQueued(items);
  const history = selectHistory(items);
  const totalSpeed = active.reduce((sum, i) => sum + i.download.bytesPerSecond, 0);
  const [dialog, setDialog] = useState<"url" | "itch" | null>(null);
  const close = () => {
    setDialog(null);
  };
  const addActions = (
    <>
      <Button
        icon={Gamepad2}
        onClick={() => {
          setDialog("itch");
        }}
      >
        {t.downloads.fromItch}
      </Button>
      <Button
        variant="primary"
        icon={Link2}
        onClick={() => {
          setDialog("url");
        }}
      >
        {t.downloads.byLink}
      </Button>
    </>
  );

  return (
    <Page className="max-w-5xl">
      <PageHeader
        title="Downloads"
        description={
          active.some((i) => i.download.status === "downloading") ? (
            <span className="num">{t.downloads.total(formatSpeed(totalSpeed))}</span>
          ) : (
            t.downloads.subtitle
          )
        }
        actions={addActions}
      />
      <AddDownloadDialog open={dialog === "url"} onClose={close} />
      <ItchDialog open={dialog === "itch"} onClose={close} />

      {!loaded && (
        <LoadingRegion label={t.downloads.loading} className="flex flex-col gap-3">
          <Skeleton className="h-28 rounded-lg" />
          <Skeleton className="h-48 rounded-lg" />
        </LoadingRegion>
      )}

      {loaded && error && <ErrorState message={error} onRetry={() => void refresh()} />}

      {loaded && !error && items.length === 0 && (
        <EmptyState
          icon={Download}
          title={t.downloads.emptyTitle}
          description={t.downloads.emptyText}
          actions={<ButtonLink to="/library">{t.downloads.toLibrary}</ButtonLink>}
        />
      )}

      {loaded && items.length > 0 && (
        <>
          <Section title={t.downloads.active}>
            {active.length === 0 ? (
              <p className="rounded-lg border border-border px-4 py-6 text-center text-sm text-fg-muted">{t.downloads.noneActive}</p>
            ) : (
              <div className="flex flex-col gap-3">
                {active.map((item) => (
                  <ActiveDownload key={item.download.id} item={item} />
                ))}
              </div>
            )}
          </Section>

          <Section title={t.downloads.queue} description={queued.length > 1 ? t.downloads.reorderHint : undefined}>
            {queued.length === 0 ? (
              <p className="rounded-lg border border-border px-4 py-6 text-center text-sm text-fg-muted">{t.downloads.queueEmpty}</p>
            ) : (
              <Queue items={queued} />
            )}
          </Section>

          <Section
            title={t.downloads.historyTitle}
            actions={
              history.length > 0 ? (
                <Button variant="ghost" size="sm" onClick={() => void act(() => api.downloads.clearDownloadHistory())}>
                  {t.downloads.clearHistory}
                </Button>
              ) : undefined
            }
          >
            {history.length === 0 ? (
              <p className="rounded-lg border border-border px-4 py-6 text-center text-sm text-fg-muted">{t.downloads.historyEmpty}</p>
            ) : (
              <ul className="rounded-lg border border-border">
                {history.map((item) => (
                  <HistoryRow key={item.download.id} item={item} />
                ))}
              </ul>
            )}
          </Section>
        </>
      )}
    </Page>
  );
}
