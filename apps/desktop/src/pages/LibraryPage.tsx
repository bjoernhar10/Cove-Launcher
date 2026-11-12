import { localized, useMessages } from "../i18n/index.ts";
import type { Collection, LibraryItem, TimeToBeat } from "@cove/shared";
import { Folder, Hourglass, LayoutGrid, Library, List, Pencil, Plus, Star, Trash2 } from "lucide-react";
import { memo, useCallback, useMemo, useRef, useState, type MouseEvent } from "react";
import { Link } from "react-router";
import { api } from "../api/index.ts";
import { AchievementMeter } from "../components/game/AchievementMeter.tsx";
import { COVER_GRID, GameCardSkeleton } from "../components/game/GameCard.tsx";
import { GameCover } from "../components/game/GameCover.tsx";
import { PlayButton } from "../components/game/PlayButton.tsx";
import { VirtualGrid } from "../components/game/VirtualGrid.tsx";
import { Button, ButtonLink } from "../components/ui/Button.tsx";
import { EmptyState, ErrorState, LoadingRegion, Skeleton } from "../components/ui/Feedback.tsx";
import { Page, PageHeader } from "../components/ui/Page.tsx";
import { Popover, RadioOption } from "../components/ui/Popover.tsx";
import { Segmented } from "../components/ui/Segmented.tsx";
import { CollectionNameDialog, DeleteCollectionDialog } from "../features/CollectionDialogs.tsx";
import { CollectionMenu, type MenuAnchor } from "../features/CollectionMenu.tsx";
import { typicalLength } from "../features/TimeToBeat.tsx";
import { useMainVirtualizer, VIRTUALIZE_FROM } from "../hooks/useMainVirtualizer.ts";
import { useQuery } from "../hooks/useQuery.ts";
import { cx } from "../lib/cx.ts";
import { formatNumber, formatPlaytime, formatRelative } from "../lib/format.ts";
import { SOURCE_LABELS } from "../lib/labels.ts";
import {
  collectionGroup,
  usePreferences,
  type BuiltinGroup,
  type LengthFilter,
  type LibraryGroup,
} from "../stores/preferences.ts";
import { invalidateQueries } from "../stores/query.ts";
import { errorMessage, toast } from "../stores/toasts.ts";
import { useUi } from "../stores/ui.ts";

const GROUPS: BuiltinGroup[] = ["all", "installed", "favorites", "steam", "gog", "epic", "manual"];

function isBuiltin(group: LibraryGroup): group is BuiltinGroup {
  return !group.startsWith("collection:");
}

function groupLabel(group: BuiltinGroup): string {
  return group === "all" || group === "installed" || group === "favorites" ? GROUP_LABELS[group] : SOURCE_LABELS[group];
}

const GROUP_LABELS = localized((m) => m.library.groups);

function matchesGroup(item: LibraryItem, group: BuiltinGroup): boolean {
  if (group === "all") return true;
  if (group === "installed") return item.entry.installed;
  if (group === "favorites") return item.entry.favorite;
  return item.entry.source === group;
}

/** Upper bounds in hours (the last band is open). */
const LENGTHS: { value: Exclude<LengthFilter, "any">; min: number; max: number }[] = [
  { value: "short", min: 0, max: 5 },
  { value: "medium", min: 5, max: 15 },
  { value: "long", min: 15, max: 40 },
  { value: "veryLong", min: 40, max: Number.POSITIVE_INFINITY },
];

const LENGTH_LABELS: Record<LengthFilter, string> = localized((m) => m.timeToBeat.lengths);

function inBand(times: TimeToBeat | undefined, filter: LengthFilter): boolean {
  if (filter === "any") return true;
  const band = LENGTHS.find((l) => l.value === filter);
  const seconds = times ? typicalLength(times) : null;
  if (!band || seconds === null) return false;
  const hours = seconds / 3600;
  return hours >= band.min && hours < band.max;
}

function LengthPopover({ value, onChange }: { value: LengthFilter; onChange: (value: LengthFilter) => void }) {
  const t = useMessages();
  return (
    <Popover label={t.timeToBeat.filter} value={value === "any" ? undefined : LENGTH_LABELS[value]} active={value !== "any"} align="end">
      <div className="flex flex-col gap-1 p-1">
        {(["any", ...LENGTHS.map((l) => l.value)] as const).map((option) => (
          <RadioOption
            key={option}
            name="library-length"
            label={LENGTH_LABELS[option]}
            checked={value === option}
            onSelect={() => {
              onChange(option);
            }}
          />
        ))}
        <p className="max-w-56 px-2 pt-2 pb-1 text-xs text-fg-muted">{t.timeToBeat.filterHint}</p>
      </div>
    </Popover>
  );
}

const EMPTY_GROUP: Record<BuiltinGroup, { title: string; description: string }> = localized((m) => m.library.emptyGroup);

const CHIP = "inline-flex h-8 items-center gap-2 rounded-md px-3 text-sm transition-colors duration-150 ease-out";
const CHIP_IDLE = "text-fg-muted hover:bg-hover hover:text-fg";

type FavoriteHandler = (item: LibraryItem, favorite: boolean) => void;
/** Opens the collections menu of a game (right-click or the context-menu key). */
type MenuHandler = (item: LibraryItem, anchor: MenuAnchor) => void;

/** Where a context menu opens: at the pointer, or at the element when the keyboard asked for it. */
function menuAnchor(event: MouseEvent<HTMLElement>): MenuAnchor {
  if (event.clientX !== 0 || event.clientY !== 0) return { x: event.clientX, y: event.clientY };
  const target = event.target instanceof HTMLElement ? event.target : event.currentTarget;
  const rect = target.getBoundingClientRect();
  return { x: rect.left + 16, y: rect.top + 16 };
}

function FavoriteButton({ item, onToggle }: { item: LibraryItem; onToggle: FavoriteHandler }) {
  const t = useMessages();
  const { favorite } = item.entry;
  return (
    <button
      type="button"
      aria-pressed={favorite}
      aria-label={favorite ? t.library.unfavoriteTitle(item.game.title) : t.library.favoriteTitle(item.game.title)}
      title={favorite ? t.library.unfavorite : t.library.favorite}
      onClick={() => {
        onToggle(item, !favorite);
      }}
      className={cx(
        "inline-flex size-8 items-center justify-center rounded-md transition-colors duration-150 ease-out hover:bg-hover",
        favorite ? "text-accent-text" : "text-fg-subtle hover:text-fg",
      )}
    >
      <Star aria-hidden className={favorite ? "fill-current" : undefined} />
    </button>
  );
}

interface GameRowProps {
  item: LibraryItem;
  onFavorite: FavoriteHandler;
  onMenu: MenuHandler;
}

const GridCard = memo(function GridCard({ item, onFavorite, onMenu }: GameRowProps) {
  const t = useMessages();
  const { game, entry } = item;
  return (
    <div
      className="group relative"
      onContextMenu={(event) => {
        event.preventDefault();
        onMenu(item, menuAnchor(event));
      }}
    >
      <Link to={`/library/${game.id}`} aria-label={game.title} className="block rounded-lg focus-visible:outline-offset-4">
        <GameCover game={game} className="border border-border transition-colors duration-150 ease-out group-hover:border-border-strong" />
      </Link>
      <div className="absolute top-2 right-2 opacity-0 transition-opacity duration-150 ease-out group-focus-within:opacity-100 group-hover:opacity-100">
        <PlayButton item={item} size="sm" iconOnly />
      </div>
      <div className="mt-3 flex items-start gap-1">
        <div className="min-w-0 flex-1">
          <p className="truncate text-sm font-medium text-fg">{game.title}</p>
          <p className="num mt-1 truncate text-xs text-fg-muted">
            {formatPlaytime(entry.playtimeMinutes)}
            {!entry.installed && ` · ${t.library.notInstalled}`}
            {entry.matchStatus === "pending" && ` · ${t.library.checkMatch}`}
          </p>
        </div>
        <FavoriteButton item={item} onToggle={onFavorite} />
      </div>
      <AchievementMeter progress={entry.achievements} className="mt-2" />
    </div>
  );
});

/** Height of one list row in px (40px thumbnail + 2 × 8px padding + 1px border). */
const LIST_ROW_HEIGHT = 57;

const ListRow = memo(function ListRow({ item, onFavorite, onMenu }: GameRowProps) {
  const t = useMessages();
  const { game, entry } = item;
  return (
    <tr
      style={{ height: LIST_ROW_HEIGHT }}
      className="border-b border-border transition-colors duration-150 ease-out last:border-b-0 hover:bg-hover"
      onContextMenu={(event) => {
        event.preventDefault();
        onMenu(item, menuAnchor(event));
      }}
    >
      <td className="py-2 pr-4 pl-4">
        <Link to={`/library/${game.id}`} className="flex items-center gap-3 rounded-md">
          <GameCover game={game} aspect="square" showTitle={false} className="w-10 shrink-0 rounded-md" />
          <span className="min-w-0">
            <span className="block truncate text-sm font-medium text-fg">{game.title}</span>
            <span className="block truncate text-xs text-fg-subtle">{entry.installed ? t.library.installed : t.library.notInstalled}</span>
          </span>
        </Link>
      </td>
      <td className="px-4 text-sm whitespace-nowrap text-fg-muted">{SOURCE_LABELS[entry.source]}</td>
      <td className="num px-4 text-sm whitespace-nowrap text-fg-muted">{formatPlaytime(entry.playtimeMinutes)}</td>
      <td className="px-4 text-sm whitespace-nowrap text-fg-muted">{entry.lastPlayedAt ? formatRelative(entry.lastPlayedAt) : "—"}</td>
      <td className="w-44 px-4">
        <AchievementMeter progress={entry.achievements} />
      </td>
      <td className="py-2 pr-4 pl-2">
        <div className="flex items-center justify-end gap-1">
          <FavoriteButton item={item} onToggle={onFavorite} />
          <PlayButton item={item} size="sm" />
        </div>
      </td>
    </tr>
  );
});

function LibraryTable({ items, onFavorite, onMenu }: { items: LibraryItem[]; onFavorite: FavoriteHandler; onMenu: MenuHandler }) {
  const t = useMessages();
  const bodyRef = useRef<HTMLTableSectionElement>(null);
  const virtual = items.length > VIRTUALIZE_FROM;
  const virtualizer = useMainVirtualizer({ listRef: bodyRef, count: virtual ? items.length : 0, estimateSize: LIST_ROW_HEIGHT, overscan: 8 });
  const rows = virtualizer.getVirtualItems();
  const margin = virtualizer.options.scrollMargin;
  // Spacer rows keep the scroll height without rendering every game.
  const before = virtual ? (rows[0]?.start ?? margin) - margin : 0;
  const after = virtual ? virtualizer.getTotalSize() - ((rows.at(-1)?.end ?? margin) - margin) : 0;
  const visible = virtual ? rows.flatMap((row) => items[row.index] ?? []) : items;

  return (
    <div className="overflow-hidden rounded-lg border border-border">
      <table className="w-full border-collapse text-left" aria-rowcount={items.length + 1}>
        <thead>
          <tr className="border-b border-border text-xs text-fg-subtle">
            <th scope="col" className="py-3 pl-4 font-medium">{t.library.colGame}</th>
            <th scope="col" className="px-4 font-medium">{t.library.colSource}</th>
            <th scope="col" className="px-4 font-medium">{t.library.colPlaytime}</th>
            <th scope="col" className="px-4 font-medium">{t.library.colLastPlayed}</th>
            <th scope="col" className="px-4 font-medium">{t.library.colProgress}</th>
            <th scope="col" className="pr-4">
              <span className="sr-only">{t.palette.actions}</span>
            </th>
          </tr>
        </thead>
        <tbody ref={bodyRef}>
          {before > 0 && <tr aria-hidden style={{ height: before }} />}
          {visible.map((item) => (
            <ListRow key={item.game.id} item={item} onFavorite={onFavorite} onMenu={onMenu} />
          ))}
          {after > 0 && <tr aria-hidden style={{ height: after }} />}
        </tbody>
      </table>
    </div>
  );
}

export function LibraryPage() {
  const t = useMessages();
  const query = useQuery("library", api.library.listLibrary);
  const view = usePreferences((s) => s.libraryView);
  const setView = usePreferences((s) => s.setLibraryView);
  const collectionsQuery = useQuery("collections", api.collections.list);
  const storedGroup = usePreferences((s) => s.libraryGroup);
  const setGroup = usePreferences((s) => s.setLibraryGroup);
  const length = usePreferences((s) => s.libraryLength);
  const setLength = usePreferences((s) => s.setLibraryLength);
  const openAddGame = useUi((s) => s.openAddGame);
  const [menu, setMenu] = useState<{ item: LibraryItem; anchor: MenuAnchor } | null>(null);
  const [naming, setNaming] = useState<{ collection: Collection | null } | null>(null);
  const [deleting, setDeleting] = useState<Collection | null>(null);

  const collections = useMemo(
    () => (collectionsQuery.status === "success" ? collectionsQuery.data : []),
    [collectionsQuery.status, collectionsQuery.data],
  );
  const collection = collections.find((c) => collectionGroup(c.id) === storedGroup) ?? null;
  // A stored collection that is gone (or not loaded yet) shows the whole library.
  const group: BuiltinGroup = isBuiltin(storedGroup) ? storedGroup : "all";
  const openMenu = useCallback<MenuHandler>((item, anchor) => {
    setMenu({ item, anchor });
  }, []);

  const { update } = query;
  const setFavorite = useCallback<FavoriteHandler>(
    (item, favorite) => {
      update((items) => items.map((i) => (i.game.id === item.game.id ? { ...i, entry: { ...i.entry, favorite } } : i)));
      api.library.setFavorite(item.game.id, favorite).then(
        () => {
          invalidateQueries();
        },
        (error: unknown) => {
          toast(errorMessage(error), "danger");
          invalidateQueries();
        },
      );
    },
    [update],
  );

  const all = useMemo(() => (query.status === "success" ? query.data : []), [query.status, query.data]);
  const catalogIds = useMemo(() => all.flatMap((i) => (i.entry.catalogId ? [i.entry.catalogId] : [])), [all]);
  // Only loaded while the length filter is on (one request for the whole library, cached by the core).
  const times = useQuery(length === "any" ? "time-to-beat/off" : `time-to-beat/library/${String(catalogIds.length)}`, () =>
    length === "any" ? Promise.resolve<Record<string, TimeToBeat>>({}) : api.catalog.timeToBeat(catalogIds),
  );
  const grouped = useMemo(() => {
    if (!collection) return all.filter((i) => matchesGroup(i, group));
    const members = new Set(collection.gameIds);
    return all.filter((i) => members.has(i.game.id));
  }, [all, group, collection]);
  const items = useMemo(() => {
    if (length === "any") return grouped;
    const known = times.status === "success" ? times.data : {};
    return grouped.filter((i) => inBand(i.entry.catalogId ? known[i.entry.catalogId] : undefined, length));
  }, [grouped, length, times.status, times.data]);
  const lengthLoading = length !== "any" && times.status === "loading";
  const counts = useMemo(() => {
    const result = {} as Record<BuiltinGroup, number>;
    for (const g of GROUPS) result[g] = all.filter((i) => matchesGroup(i, g)).length;
    return result;
  }, [all]);
  const cells = useMemo(
    () => items.map((item) => ({ key: item.game.id, node: <GridCard item={item} onFavorite={setFavorite} onMenu={openMenu} /> })),
    [items, setFavorite, openMenu],
  );
  const addButton = (
    <Button
      variant="primary"
      icon={Plus}
      onClick={() => {
        openAddGame();
      }}
    >
      {t.palette.addGame}
    </Button>
  );

  return (
    <Page>
      <PageHeader
        title="My Library"
        description={query.status === "success" ? <><span className="num">{formatNumber(all.length)}</span> {t.library.games}</> : t.library.subtitle}
        actions={addButton}
      />

      {query.status === "success" && all.length === 0 ? (
        <EmptyState
          icon={Library}
          title={t.library.emptyTitle}
          description={t.library.emptyText}
          actions={
            <>
              <ButtonLink to="/settings/accounts">{t.home.connectStore}</ButtonLink>
              {addButton}
            </>
          }
        />
      ) : (
        <>
          <div className="mb-8 flex items-start justify-between gap-4">
            <nav aria-label={t.library.grouping} className="flex min-w-0 flex-1 flex-wrap gap-1">
              {GROUPS.map((g) => {
                const selected = !collection && g === group;
                return (
                  <button
                    key={g}
                    type="button"
                    aria-pressed={selected}
                    onClick={() => {
                      setGroup(g);
                    }}
                    className={cx(CHIP, selected ? "bg-active font-medium text-fg" : CHIP_IDLE, g === "steam" && "ml-3")}
                  >
                    {groupLabel(g)}
                    {query.status === "success" && <span className="num text-xs text-fg-subtle">{counts[g]}</span>}
                  </button>
                );
              })}
              {collections.map((c, index) => {
                const selected = c.id === collection?.id;
                return (
                  <button
                    key={c.id}
                    type="button"
                    aria-pressed={selected}
                    onClick={() => {
                      setGroup(collectionGroup(c.id));
                    }}
                    className={cx(CHIP, "max-w-56", selected ? "bg-active font-medium text-fg" : CHIP_IDLE, index === 0 && "ml-3")}
                  >
                    <span className="truncate">{c.name}</span>
                    <span className="num text-xs text-fg-subtle">{c.gameIds.length}</span>
                  </button>
                );
              })}
              {collectionsQuery.status === "success" && (
                <button
                  type="button"
                  onClick={() => {
                    setNaming({ collection: null });
                  }}
                  className={cx(CHIP, CHIP_IDLE, collections.length === 0 && "ml-3")}
                >
                  <Plus aria-hidden />
                  {t.collections.create}
                </button>
              )}
            </nav>
            <div className="flex shrink-0 items-center gap-1">
              {collection && (
                <>
                  <Button
                    variant="ghost"
                    size="sm"
                    iconOnly
                    icon={Pencil}
                    label={t.collections.rename}
                    onClick={() => {
                      setNaming({ collection });
                    }}
                  />
                  <Button
                    variant="ghost"
                    size="sm"
                    iconOnly
                    icon={Trash2}
                    label={t.collections.delete}
                    className="mr-2"
                    onClick={() => {
                      setDeleting(collection);
                    }}
                  />
                </>
              )}
              <div className="mr-2">
                <LengthPopover value={length} onChange={setLength} />
              </div>
              <Segmented
                label={t.library.view}
                value={view}
                onChange={setView}
                options={[
                  { value: "grid", label: t.library.grid, icon: LayoutGrid, iconOnly: true },
                  { value: "list", label: t.library.list, icon: List, iconOnly: true },
                ]}
              />
            </div>
          </div>

          {(query.status === "loading" || (query.status === "success" && lengthLoading)) &&
            (view === "grid" ? (
              <LoadingRegion label={t.library.loading} className={COVER_GRID}>
                {Array.from({ length: 12 }, (_, i) => (
                  <GameCardSkeleton key={i} />
                ))}
              </LoadingRegion>
            ) : (
              <LoadingRegion label={t.library.loading} className="rounded-lg border border-border">
                {Array.from({ length: 8 }, (_, i) => (
                  <div key={i} className="flex items-center gap-3 border-b border-border px-4 py-2 last:border-b-0">
                    <Skeleton className="size-10" />
                    <Skeleton className="h-4 w-48" />
                  </div>
                ))}
              </LoadingRegion>
            ))}

          {query.status === "error" && <ErrorState message={query.error} onRetry={query.retry} />}

          {length !== "any" && times.status === "error" && <ErrorState message={times.error} onRetry={times.retry} />}

          {query.status === "success" &&
            !lengthLoading &&
            items.length === 0 &&
            (length !== "any" && grouped.length > 0 ? (
              <EmptyState
                compact
                icon={Hourglass}
                title={t.timeToBeat.emptyTitle}
                description={t.timeToBeat.emptyText}
                actions={
                  <Button
                    onClick={() => {
                      setLength("any");
                    }}
                  >
                    {t.timeToBeat.showAll}
                  </Button>
                }
              />
            ) : collection ? (
              <EmptyState compact icon={Folder} title={t.collections.emptyTitle} description={t.collections.emptyText} />
            ) : (
              <EmptyState compact icon={Library} title={EMPTY_GROUP[group].title} description={EMPTY_GROUP[group].description} />
            ))}

          {query.status === "success" && !lengthLoading && items.length > 0 && view === "grid" && (
            <VirtualGrid cells={cells} label={t.palette.games} />
          )}

          {query.status === "success" && !lengthLoading && items.length > 0 && view === "list" && (
            <LibraryTable items={items} onFavorite={setFavorite} onMenu={openMenu} />
          )}
        </>
      )}

      {menu && (
        <CollectionMenu
          game={menu.item.game}
          anchor={menu.anchor}
          onClose={() => {
            setMenu(null);
          }}
        />
      )}
      <CollectionNameDialog
        open={naming !== null}
        collection={naming?.collection ?? null}
        onClose={() => {
          setNaming(null);
        }}
        onSaved={(saved) => {
          setGroup(collectionGroup(saved.id));
        }}
      />
      <DeleteCollectionDialog
        collection={deleting}
        onClose={() => {
          setDeleting(null);
        }}
        onDeleted={() => {
          setGroup("all");
        }}
      />
    </Page>
  );
}
