import type { Genre, Platform, YearRange } from "@cove/shared";
import { SearchX, X } from "lucide-react";
import { useEffect, useRef } from "react";
import { COVER_GRID, GameCard, GameCardSkeleton } from "../components/game/GameCard.tsx";
import { Button } from "../components/ui/Button.tsx";
import { EmptyState, ErrorState, LoadingRegion } from "../components/ui/Feedback.tsx";
import { Input, Select } from "../components/ui/Field.tsx";
import { Page, PageHeader } from "../components/ui/Page.tsx";
import { CheckOption, Popover, RadioOption } from "../components/ui/Popover.tsx";
import { useInfiniteCatalog } from "../hooks/useInfiniteCatalog.ts";
import { formatNumber } from "../lib/format.ts";
import { GENRE_LABELS, PLATFORM_LABELS, SORT_LABELS } from "../lib/labels.ts";
import { activeFilterCount, useCatalogFilter } from "../stores/catalog.ts";

const GENRES = Object.keys(GENRE_LABELS) as Genre[];
const PLATFORMS = Object.keys(PLATFORM_LABELS) as Platform[];

const YEAR_OPTIONS: { label: string; range: YearRange | null }[] = [
  { label: "Alle Jahre", range: null },
  { label: "2025–2026", range: { from: 2025, to: 2026 } },
  { label: "2022–2024", range: { from: 2022, to: 2024 } },
  { label: "2018–2021", range: { from: 2018, to: 2021 } },
  { label: "Vor 2018", range: { from: 1970, to: 2017 } },
];

const RATING_OPTIONS: { label: string; value: number | null }[] = [
  { label: "Alle Bewertungen", value: null },
  { label: "Ab 90", value: 90 },
  { label: "Ab 80", value: 80 },
  { label: "Ab 70", value: 70 },
];

function toggle<T>(list: T[], value: T, on: boolean): T[] {
  return on ? [...list, value] : list.filter((v) => v !== value);
}

function summarize(labels: string[]): string | undefined {
  if (labels.length === 0) return undefined;
  if (labels.length === 1) return labels[0];
  return `${labels[0] ?? ""} +${String(labels.length - 1)}`;
}

function SortSelect() {
  const sort = useCatalogFilter((s) => s.filter.sort);
  const setFilter = useCatalogFilter((s) => s.setFilter);
  return (
    <label className="flex items-center gap-2 text-sm text-fg-muted">
      Sortierung
      <Select
        className="w-36"
        value={sort}
        onChange={(next) => {
          setFilter({ sort: next });
        }}
        options={(Object.keys(SORT_LABELS) as (keyof typeof SORT_LABELS)[]).map((value) => ({ value, label: SORT_LABELS[value] }))}
      />
    </label>
  );
}

function FilterBar() {
  const filter = useCatalogFilter((s) => s.filter);
  const setFilter = useCatalogFilter((s) => s.setFilter);
  const reset = useCatalogFilter((s) => s.reset);
  const yearLabel = YEAR_OPTIONS.find((o) => o.range?.from === filter.releaseYears?.from)?.label;
  const ratingLabel = RATING_OPTIONS.find((o) => o.value === filter.minRating)?.label;
  const count = activeFilterCount(filter);

  return (
    <div role="toolbar" aria-label="Filter" className="sticky top-0 z-20 -mx-8 mb-8 flex flex-wrap items-center gap-2 border-b border-border bg-bg px-8 py-3">
      <Input
        type="search"
        aria-label="Im Katalog suchen"
        placeholder="Titel oder Studio …"
        className="h-8 w-52"
        value={filter.query}
        onChange={(e) => {
          setFilter({ query: e.target.value });
        }}
      />
      <Popover label="Genre" value={summarize(filter.genres.map((g) => GENRE_LABELS[g]))} active={filter.genres.length > 0} panelClassName="w-80">
        <div className="grid grid-cols-2 gap-x-1">
          {GENRES.map((g) => (
            <CheckOption
              key={g}
              label={GENRE_LABELS[g]}
              checked={filter.genres.includes(g)}
              onChange={(on) => {
                setFilter({ genres: toggle(filter.genres, g, on) });
              }}
            />
          ))}
        </div>
      </Popover>
      <Popover label="Plattform" value={summarize(filter.platforms.map((p) => PLATFORM_LABELS[p]))} active={filter.platforms.length > 0}>
        {PLATFORMS.map((p) => (
          <CheckOption
            key={p}
            label={PLATFORM_LABELS[p]}
            checked={filter.platforms.includes(p)}
            onChange={(on) => {
              setFilter({ platforms: toggle(filter.platforms, p, on) });
            }}
          />
        ))}
      </Popover>
      <Popover label="Erscheinungsjahr" value={filter.releaseYears ? yearLabel : undefined} active={filter.releaseYears !== null}>
        {YEAR_OPTIONS.map((o) => (
          <RadioOption
            key={o.label}
            name="year"
            label={o.label}
            checked={o.range?.from === filter.releaseYears?.from}
            onSelect={() => {
              setFilter({ releaseYears: o.range });
            }}
          />
        ))}
      </Popover>
      <Popover label="Bewertung" value={filter.minRating !== null ? ratingLabel : undefined} active={filter.minRating !== null}>
        {RATING_OPTIONS.map((o) => (
          <RadioOption
            key={o.label}
            name="rating"
            label={o.label}
            checked={o.value === filter.minRating}
            onSelect={() => {
              setFilter({ minRating: o.value });
            }}
          />
        ))}
      </Popover>
      {count > 0 && (
        <Button variant="ghost" size="sm" icon={X} onClick={reset}>
          Zurücksetzen
        </Button>
      )}
    </div>
  );
}

export function CataloguePage() {
  const filter = useCatalogFilter((s) => s.filter);
  const reset = useCatalogFilter((s) => s.reset);
  const catalog = useInfiniteCatalog(filter);
  const sentinel = useRef<HTMLDivElement>(null);
  const { hasMore, loadMore } = catalog;

  useEffect(() => {
    const el = sentinel.current;
    if (!el || !hasMore || typeof IntersectionObserver === "undefined") return;
    const observer = new IntersectionObserver(
      (entries) => {
        if (entries.some((e) => e.isIntersecting)) loadMore();
      },
      { root: document.getElementById("main"), rootMargin: "600px" },
    );
    observer.observe(el);
    return () => {
      observer.disconnect();
    };
  }, [hasMore, loadMore]);

  return (
    <Page className="pt-8">
      <PageHeader
        title="Catalogue"
        description={
          catalog.status === "success" ? (
            <>
              <span className="num">{formatNumber(catalog.total)}</span> Spiele aus allen verbundenen Stores
            </>
          ) : (
            "Spiele aus allen verbundenen Stores"
          )
        }
        actions={<SortSelect />}
      />
      <FilterBar />

      {catalog.status === "loading" && (
        <LoadingRegion label="Katalog wird geladen" className={COVER_GRID}>
          {Array.from({ length: 12 }, (_, i) => (
            <GameCardSkeleton key={i} />
          ))}
        </LoadingRegion>
      )}

      {catalog.status === "error" && <ErrorState message={catalog.error ?? ""} onRetry={catalog.retry} />}

      {catalog.status === "success" && catalog.items.length === 0 && (
        <EmptyState
          icon={SearchX}
          title="Keine Spiele gefunden"
          description="Zu diesen Filtern passt gerade nichts. Versuch es mit weniger Einschränkungen."
          actions={<Button onClick={reset}>Filter zurücksetzen</Button>}
        />
      )}

      {catalog.status === "success" && catalog.items.length > 0 && (
        <>
          <ul className={COVER_GRID} aria-label="Spiele">
            {catalog.items.map((game) => (
              <li key={game.id}>
                <GameCard game={game} to={`/catalogue/${game.id}`} />
              </li>
            ))}
            {catalog.loadingMore &&
              Array.from({ length: 6 }, (_, i) => (
                <li key={`s${String(i)}`} aria-hidden>
                  <GameCardSkeleton />
                </li>
              ))}
          </ul>
          <div ref={sentinel} className="h-px" />
          {catalog.hasMore && !catalog.loadingMore && (
            <div className="mt-8 flex justify-center">
              <Button onClick={loadMore}>Mehr laden</Button>
            </div>
          )}
          {!catalog.hasMore && (
            <p className="mt-12 text-center text-sm text-fg-subtle">Das waren alle {formatNumber(catalog.total)} Spiele.</p>
          )}
        </>
      )}
    </Page>
  );
}
