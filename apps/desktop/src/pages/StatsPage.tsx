import type { PlayStats } from "@cove/shared";
import { ChartColumn } from "lucide-react";
import type { ReactNode } from "react";
import { Link } from "react-router";
import { api } from "../api/index.ts";
import { GameCover } from "../components/game/GameCover.tsx";
import { BarChart, ShareRow } from "../components/ui/BarChart.tsx";
import { EmptyState, ErrorState, LoadingRegion, Skeleton } from "../components/ui/Feedback.tsx";
import { Select } from "../components/ui/Field.tsx";
import { Page, PageHeader, Section } from "../components/ui/Page.tsx";
import { useQuery } from "../hooks/useQuery.ts";
import { useMessages } from "../i18n/index.ts";
import { GENRE_LABELS, SOURCE_LABELS } from "../lib/labels.ts";
import {
  formatDate,
  formatMinutesShort,
  formatMonthLong,
  formatMonthShort,
  formatNumber,
  formatPlaytime,
  formatWeekdayIndex,
} from "../lib/format.ts";
import { usePreferences } from "../stores/preferences.ts";

const ALL = "all";

function Tile({ label, children }: { label: string; children: ReactNode }) {
  return (
    <div className="rounded-lg border border-border px-4 py-3">
      <p className="text-xs text-fg-subtle">{label}</p>
      <p className="num mt-1 text-base font-medium text-fg">{children}</p>
    </div>
  );
}

function Overview({ stats }: { stats: PlayStats }) {
  const t = useMessages();
  const longest = stats.longestSession;
  const topMax = stats.topGames[0]?.minutes ?? 0;
  const genreMax = stats.genres[0]?.minutes ?? 0;
  const storeMax = stats.stores[0]?.minutes ?? 0;

  return (
    <>
      <div className="grid grid-cols-2 gap-3 md:grid-cols-3 xl:grid-cols-5">
        <Tile label={t.stats.playtime}>
          {formatPlaytime(stats.totalMinutes)}
        </Tile>
        <Tile label={t.stats.sessions}>{formatNumber(stats.sessionCount)}</Tile>
        <Tile label={t.stats.gamesPlayed}>{formatNumber(stats.gamesPlayed)}</Tile>
        <Tile label={t.stats.streak}>{t.stats.days(stats.longestStreakDays)}</Tile>
        <Tile label={t.stats.achievements}>{formatNumber(stats.achievementsUnlocked)}</Tile>
      </div>
      <p className="mt-3 text-sm text-fg-muted">
        {stats.daysPlayed > 0 && t.stats.daysPlayed(stats.daysPlayed)}{" "}
        {longest && t.stats.longestSession(formatPlaytime(longest.minutes), longest.title, formatDate(longest.startedAt))}
      </p>

      <Section title={t.stats.months} description={stats.year === null ? t.stats.monthsAllTime : undefined}>
        <BarChart
          title={t.stats.months}
          labelHeader={t.stats.month}
          valueHeader={t.stats.playtime}
          format={formatMinutesShort}
          bars={stats.months.map((m) => ({
            key: m.month,
            label: formatMonthShort(m.month),
            fullLabel: formatMonthLong(m.month),
            value: m.minutes,
          }))}
        />
      </Section>

      <div className="mt-12 grid gap-12 lg:grid-cols-[3fr_2fr]">
        <Section title={t.stats.topGames} className="mt-0">
          <ol className="flex flex-col">
            {stats.topGames.map((entry, index) => (
              <li key={entry.game.id} className="flex items-center gap-4 border-b border-border py-3 last:border-b-0">
                <span className="num w-6 shrink-0 text-right text-sm text-fg-subtle">{index + 1}</span>
                <GameCover game={entry.game} showTitle={false} className="w-10 shrink-0 rounded-md" />
                <div className="flex min-w-0 flex-1 flex-col gap-2">
                  <div className="flex items-baseline justify-between gap-4">
                    <Link to={`/library/${entry.game.id}`} className="min-w-0 truncate text-sm font-medium text-fg hover:underline">
                      {entry.game.title}
                    </Link>
                    <span className="num shrink-0 text-sm text-fg-muted">{formatPlaytime(entry.minutes)}</span>
                  </div>
                  <div className="h-1 rounded-full bg-border" aria-hidden>
                    <div className="h-1 rounded-full bg-accent" style={{ width: `${String(topMax > 0 ? Math.max((entry.minutes / topMax) * 100, 1) : 0)}%` }} />
                  </div>
                  {entry.sessionCount > 0 && <p className="text-xs text-fg-subtle">{t.stats.sessionsOf(entry.sessionCount)}</p>}
                </div>
              </li>
            ))}
          </ol>
        </Section>

        <div className="flex flex-col gap-12">
          <Section title={t.stats.weekdays} className="mt-0">
            <BarChart
              title={t.stats.weekdays}
              labelHeader={t.stats.weekday}
              valueHeader={t.stats.playtime}
              format={formatMinutesShort}
              bars={stats.weekdays.map((minutes, i) => ({
                key: String(i),
                label: formatWeekdayIndex(i),
                fullLabel: formatWeekdayIndex(i, true),
                value: minutes,
              }))}
            />
          </Section>
          {stats.genres.length > 0 && (
            <Section title={t.stats.genres} className="mt-0">
              <ul>
                {stats.genres.slice(0, 6).map((g) => (
                  <ShareRow key={g.genre} label={GENRE_LABELS[g.genre]} value={formatPlaytime(g.minutes)} amount={g.minutes} max={genreMax} />
                ))}
              </ul>
            </Section>
          )}
          {stats.stores.length > 0 && (
            <Section title={t.stats.stores} className="mt-0">
              <ul>
                {stats.stores.map((s) => (
                  <ShareRow key={s.store} label={SOURCE_LABELS[s.store]} value={formatPlaytime(s.minutes)} amount={s.minutes} max={storeMax} />
                ))}
              </ul>
            </Section>
          )}
        </div>
      </div>

      {stats.year === null && <p className="mt-12 text-xs text-fg-subtle">{t.stats.importedHint}</p>}
    </>
  );
}

function StatsSkeleton() {
  const t = useMessages();
  return (
    <LoadingRegion label={t.stats.loading}>
      <div className="grid grid-cols-2 gap-3 md:grid-cols-3 xl:grid-cols-5">
        {Array.from({ length: 5 }, (_, i) => (
          <Skeleton key={i} className="h-16 rounded-lg" />
        ))}
      </div>
      <Skeleton className="mt-12 h-48 rounded-lg" />
      <div className="mt-12 grid gap-12 lg:grid-cols-[3fr_2fr]">
        <Skeleton className="h-80 rounded-lg" />
        <Skeleton className="h-80 rounded-lg" />
      </div>
    </LoadingRegion>
  );
}

export function StatsPage() {
  const t = useMessages();
  const range = usePreferences((s) => s.statsRange);
  const setRange = usePreferences((s) => s.setStatsRange);
  const year = range === ALL ? null : Number(range);
  const query = useQuery(`stats/${range}`, () => api.stats.overview(year));
  const years = query.data?.years ?? [];
  // A remembered year without sessions any more still shows up, so it can be left.
  const choices = [...new Set([...years, ...(year === null ? [] : [year])])].sort((a, b) => b - a);
  const empty = query.status === "success" && query.data.totalMinutes === 0;

  return (
    <Page>
      <PageHeader
        title={t.stats.title}
        description={t.stats.description}
        actions={
          <label className="flex items-center gap-2 text-sm text-fg-muted">
            {t.stats.range}
            <Select
              className="w-36"
              value={range}
              onChange={setRange}
              options={[{ value: ALL, label: t.stats.allTime }, ...choices.map((y) => ({ value: String(y), label: String(y) }))]}
            />
          </label>
        }
      />
      {query.status === "loading" && <StatsSkeleton />}
      {query.status === "error" && <ErrorState message={query.error} onRetry={query.retry} />}
      {empty && (
        <EmptyState
          icon={ChartColumn}
          title={t.stats.emptyTitle}
          description={year === null ? t.stats.emptyDescription : t.stats.emptyYear(year)}
        />
      )}
      {query.status === "success" && !empty && <Overview stats={query.data} />}
    </Page>
  );
}
