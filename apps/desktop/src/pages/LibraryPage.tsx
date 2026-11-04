import type { LibraryItem } from "@cove/shared";
import { LayoutGrid, Library, List, Plus, Star } from "lucide-react";
import { memo, useCallback, useMemo, useRef } from "react";
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
import { Segmented } from "../components/ui/Segmented.tsx";
import { useMainVirtualizer, VIRTUALIZE_FROM } from "../hooks/useMainVirtualizer.ts";
import { useQuery } from "../hooks/useQuery.ts";
import { cx } from "../lib/cx.ts";
import { formatNumber, formatPlaytime, formatRelative } from "../lib/format.ts";
import { SOURCE_LABELS } from "../lib/labels.ts";
import { usePreferences, type LibraryGroup } from "../stores/preferences.ts";
import { invalidateQueries } from "../stores/query.ts";
import { errorMessage, toast } from "../stores/toasts.ts";
import { useUi } from "../stores/ui.ts";

const GROUPS: { value: LibraryGroup; label: string }[] = [
  { value: "all", label: "Alle" },
  { value: "installed", label: "Installiert" },
  { value: "favorites", label: "Favoriten" },
  { value: "steam", label: SOURCE_LABELS.steam },
  { value: "gog", label: SOURCE_LABELS.gog },
  { value: "epic", label: SOURCE_LABELS.epic },
  { value: "manual", label: SOURCE_LABELS.manual },
];

function matchesGroup(item: LibraryItem, group: LibraryGroup): boolean {
  if (group === "all") return true;
  if (group === "installed") return item.entry.installed;
  if (group === "favorites") return item.entry.favorite;
  return item.entry.source === group;
}

const EMPTY_GROUP: Record<LibraryGroup, { title: string; description: string }> = {
  all: { title: "", description: "" },
  installed: { title: "Nichts installiert", description: "Installierte Spiele erscheinen hier. Starte einen Download über „Installieren“." },
  favorites: { title: "Noch keine Favoriten", description: "Markiere Spiele mit dem Stern, um sie hier schnell wiederzufinden." },
  steam: { title: "Keine Steam-Spiele", description: "Verbinde dein Steam-Konto in den Einstellungen, um deine Spiele zu importieren." },
  gog: { title: "Keine GOG-Spiele", description: "Verbinde dein GOG-Konto in den Einstellungen, um deine Spiele zu importieren." },
  epic: { title: "Keine Epic-Spiele", description: "Verbinde dein Epic-Konto in den Einstellungen, um deine Spiele zu importieren." },
  manual: { title: "Keine manuellen Spiele", description: "Spiele, die du selbst hinzufügst, landen hier." },
};

type FavoriteHandler = (item: LibraryItem, favorite: boolean) => void;

function FavoriteButton({ item, onToggle }: { item: LibraryItem; onToggle: FavoriteHandler }) {
  const { favorite } = item.entry;
  return (
    <button
      type="button"
      aria-pressed={favorite}
      aria-label={favorite ? `${item.game.title} aus Favoriten entfernen` : `${item.game.title} zu Favoriten hinzufügen`}
      title={favorite ? "Aus Favoriten entfernen" : "Zu Favoriten"}
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

const GridCard = memo(function GridCard({ item, onFavorite }: { item: LibraryItem; onFavorite: FavoriteHandler }) {
  const { game, entry } = item;
  return (
    <div className="group relative">
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
            {!entry.installed && " · Nicht installiert"}
            {entry.matchStatus === "pending" && " · Zuordnung prüfen"}
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

const ListRow = memo(function ListRow({ item, onFavorite }: { item: LibraryItem; onFavorite: FavoriteHandler }) {
  const { game, entry } = item;
  return (
    <tr style={{ height: LIST_ROW_HEIGHT }} className="border-b border-border transition-colors duration-150 ease-out last:border-b-0 hover:bg-hover">
      <td className="py-2 pr-4 pl-4">
        <Link to={`/library/${game.id}`} className="flex items-center gap-3 rounded-md">
          <GameCover game={game} aspect="square" showTitle={false} className="w-10 shrink-0 rounded-md" />
          <span className="min-w-0">
            <span className="block truncate text-sm font-medium text-fg">{game.title}</span>
            <span className="block truncate text-xs text-fg-subtle">{entry.installed ? "Installiert" : "Nicht installiert"}</span>
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

function LibraryTable({ items, onFavorite }: { items: LibraryItem[]; onFavorite: FavoriteHandler }) {
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
            <th scope="col" className="py-3 pl-4 font-medium">Spiel</th>
            <th scope="col" className="px-4 font-medium">Quelle</th>
            <th scope="col" className="px-4 font-medium">Spielzeit</th>
            <th scope="col" className="px-4 font-medium">Zuletzt gespielt</th>
            <th scope="col" className="px-4 font-medium">Fortschritt</th>
            <th scope="col" className="pr-4">
              <span className="sr-only">Aktionen</span>
            </th>
          </tr>
        </thead>
        <tbody ref={bodyRef}>
          {before > 0 && <tr aria-hidden style={{ height: before }} />}
          {visible.map((item) => (
            <ListRow key={item.game.id} item={item} onFavorite={onFavorite} />
          ))}
          {after > 0 && <tr aria-hidden style={{ height: after }} />}
        </tbody>
      </table>
    </div>
  );
}

export function LibraryPage() {
  const query = useQuery("library", api.library.listLibrary);
  const view = usePreferences((s) => s.libraryView);
  const setView = usePreferences((s) => s.setLibraryView);
  const group = usePreferences((s) => s.libraryGroup);
  const setGroup = usePreferences((s) => s.setLibraryGroup);
  const openAddGame = useUi((s) => s.openAddGame);

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
  const items = useMemo(() => all.filter((i) => matchesGroup(i, group)), [all, group]);
  const counts = useMemo(() => {
    const result = {} as Record<LibraryGroup, number>;
    for (const g of GROUPS) result[g.value] = all.filter((i) => matchesGroup(i, g.value)).length;
    return result;
  }, [all]);
  const cells = useMemo(
    () => items.map((item) => ({ key: item.game.id, node: <GridCard item={item} onFavorite={setFavorite} /> })),
    [items, setFavorite],
  );
  const addButton = (
    <Button
      variant="primary"
      icon={Plus}
      onClick={() => {
        openAddGame();
      }}
    >
      Spiel hinzufügen
    </Button>
  );

  return (
    <Page>
      <PageHeader
        title="My Library"
        description={query.status === "success" ? <><span className="num">{formatNumber(all.length)}</span> Spiele</> : "Deine Spiele aus allen Quellen"}
        actions={addButton}
      />

      {query.status === "success" && all.length === 0 ? (
        <EmptyState
          icon={Library}
          title="Deine Bibliothek ist noch leer"
          description="Verbinde Steam, GOG oder Epic, um deine Spiele automatisch zu importieren – oder füge ein Spiel von Hand hinzu."
          actions={
            <>
              <ButtonLink to="/settings/accounts">Store verbinden</ButtonLink>
              {addButton}
            </>
          }
        />
      ) : (
        <>
          <div className="mb-8 flex flex-wrap items-center justify-between gap-4">
            <nav aria-label="Gruppierung" className="flex flex-wrap gap-1">
              {GROUPS.map((g) => {
                const selected = g.value === group;
                return (
                  <button
                    key={g.value}
                    type="button"
                    aria-pressed={selected}
                    onClick={() => {
                      setGroup(g.value);
                    }}
                    className={cx(
                      "inline-flex h-8 items-center gap-2 rounded-md px-3 text-sm transition-colors duration-150 ease-out",
                      selected ? "bg-active font-medium text-fg" : "text-fg-muted hover:bg-hover hover:text-fg",
                      g.value === "steam" && "ml-3",
                    )}
                  >
                    {g.label}
                    {query.status === "success" && <span className="num text-xs text-fg-subtle">{counts[g.value]}</span>}
                  </button>
                );
              })}
            </nav>
            <Segmented
              label="Ansicht"
              value={view}
              onChange={setView}
              options={[
                { value: "grid", label: "Raster", icon: LayoutGrid, iconOnly: true },
                { value: "list", label: "Liste", icon: List, iconOnly: true },
              ]}
            />
          </div>

          {query.status === "loading" &&
            (view === "grid" ? (
              <LoadingRegion label="Bibliothek wird geladen" className={COVER_GRID}>
                {Array.from({ length: 12 }, (_, i) => (
                  <GameCardSkeleton key={i} />
                ))}
              </LoadingRegion>
            ) : (
              <LoadingRegion label="Bibliothek wird geladen" className="rounded-lg border border-border">
                {Array.from({ length: 8 }, (_, i) => (
                  <div key={i} className="flex items-center gap-3 border-b border-border px-4 py-2 last:border-b-0">
                    <Skeleton className="size-10" />
                    <Skeleton className="h-4 w-48" />
                  </div>
                ))}
              </LoadingRegion>
            ))}

          {query.status === "error" && <ErrorState message={query.error} onRetry={query.retry} />}

          {query.status === "success" && items.length === 0 && (
            <EmptyState compact icon={Library} title={EMPTY_GROUP[group].title} description={EMPTY_GROUP[group].description} />
          )}

          {query.status === "success" && items.length > 0 && view === "grid" && <VirtualGrid cells={cells} label="Spiele" />}

          {query.status === "success" && items.length > 0 && view === "list" && <LibraryTable items={items} onFavorite={setFavorite} />}
        </>
      )}
    </Page>
  );
}
