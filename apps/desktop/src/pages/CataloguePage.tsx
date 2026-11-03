import type { Genre, Platform, YearRange } from "@cove/shared";
import { Compass, SearchX, X } from "lucide-react";
import { useEffect, useRef } from "react";
import { COVER_GRID, GameCard, GameCardSkeleton } from "../components/game/GameCard.tsx";
import { Button, ButtonLink } from "../components/ui/Button.tsx";
import { EmptyState, ErrorState, LoadingRegion, OfflineBadge } from "../components/ui/Feedback.tsx";
import { Input, Select } from "../components/ui/Field.tsx";
import { Page, PageHeader } from "../components/ui/Page.tsx";
import { CheckOption, Popover, RadioOption } from "../components/ui/Popover.tsx";
import { CoveError } from "../api/index.ts";
import { useCatalogBrowse } from "../hooks/useCatalog.ts";
import { useDebounced } from "../hooks/useDebounced.ts";
import { errorMessage } from "../stores/toasts.ts";
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
  const sort = useCatalogFilter((s) => s.sort);
  const setSort = useCatalogFilter((s) => s.setSort);
  return (
    <label className="flex items-center gap-2 text-sm text-fg-muted">
      Sortierung
      <Select
        className="w-36"
        value={sort}
        onChange={setSort}
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
  const liveFilter = useCatalogFilter((s) => s.filter);
  const sort = useCatalogFilter((s) => s.sort);
  const reset = useCatalogFilter((s) => s.reset);
  // Typing in the search box should not fire a request per keystroke.
  const filter = useDebounced(liveFilter, 350);
  const query = useCatalogBrowse(filter, sort);
  const sentinel = useRef<HTMLDivElement>(null);
  const { hasNextPage, isFetchingNextPage, fetchNextPage } = query;

  useEffect(() => {
    const el = sentinel.current;
    if (!el || !hasNextPage || typeof IntersectionObserver === "undefined") return;
    const observer = new IntersectionObserver(
      (entries) => {
        if (entries.some((e) => e.isIntersecting) && !isFetchingNextPage) void fetchNextPage();
      },
      { root: document.getElementById("main"), rootMargin: "600px" },
    );
    observer.observe(el);
    return () => {
      observer.disconnect();
    };
  }, [hasNextPage, isFetchingNextPage, fetchNextPage]);

  const pages = query.data?.pages ?? [];
  // Providers can repeat a game across pages when the ranking shifts; show each once.
  const seen = new Set<string>();
  const items = pages.flatMap((p) => p.items).filter((g) => !seen.has(g.id) && seen.add(g.id));
  const total = pages[0]?.total ?? null;
  const offline = pages.some((p) => p.offline);
  const notConfigured = query.error instanceof CoveError && query.error.kind === "catalogNotConfigured";

  return (
    <Page className="pt-8">
      <PageHeader
        title="Catalogue"
        description={
          <span className="inline-flex items-center gap-3">
            {total !== null ? (
              <span>
                <span className="num">{formatNumber(total)}</span> Spiele
              </span>
            ) : (
              "Spieldaten von IGDB"
            )}
            {offline && <OfflineBadge />}
          </span>
        }
        actions={notConfigured ? undefined : <SortSelect />}
      />

      {notConfigured ? (
        <CatalogSetupState />
      ) : (
        <>
          <FilterBar />

          {query.isPending && (
            <LoadingRegion label="Katalog wird geladen" className={COVER_GRID}>
              {Array.from({ length: 12 }, (_, i) => (
                <GameCardSkeleton key={i} />
              ))}
            </LoadingRegion>
          )}

          {query.isError && <ErrorState message={errorMessage(query.error)} onRetry={() => void query.refetch()} />}

          {query.isSuccess && items.length === 0 && (
            <EmptyState
              icon={SearchX}
              title="Keine Spiele gefunden"
              description="Zu diesen Filtern passt gerade nichts. Versuch es mit weniger Einschränkungen."
              actions={<Button onClick={reset}>Filter zurücksetzen</Button>}
            />
          )}

          {items.length > 0 && (
            <>
              <ul className={COVER_GRID} aria-label="Spiele">
                {items.map((game) => (
                  <li key={game.id}>
                    <GameCard game={game} to={`/catalogue/${game.id}`} />
                  </li>
                ))}
                {isFetchingNextPage &&
                  Array.from({ length: 6 }, (_, i) => (
                    <li key={`s${String(i)}`} aria-hidden>
                      <GameCardSkeleton />
                    </li>
                  ))}
              </ul>
              <div ref={sentinel} className="h-px" />
              {query.isFetchNextPageError && (
                <div className="mt-8 flex justify-center">
                  <Button onClick={() => void fetchNextPage()}>Weitere Spiele laden</Button>
                </div>
              )}
              {!hasNextPage && <p className="mt-12 text-center text-sm text-fg-subtle">Das war alles.</p>}
            </>
          )}
        </>
      )}
    </Page>
  );
}

/** Shown when no catalogue source is available (no Cove service in this build / switched off, no own keys). */
export function CatalogSetupState() {
  return (
    <EmptyState
      icon={Compass}
      title="Katalog nicht verfügbar"
      description="Der Cove-Katalogdienst ist in diesem Build nicht enthalten oder ausgeschaltet. Unter Einstellungen → Katalog kannst du ihn einschalten oder eigene Zugangsdaten für IGDB eintragen."
      actions={
        <ButtonLink to="/settings/catalog" variant="primary">
          Zu den Einstellungen
        </ButtonLink>
      }
    />
  );
}
