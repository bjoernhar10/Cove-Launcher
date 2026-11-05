import { messages, useMessages } from "../i18n/index.ts";
import type { LibraryItem } from "@cove/shared";
import { Compass, Library, Medal, Plus, Trophy } from "lucide-react";
import { Link } from "react-router";
import { api } from "../api/index.ts";
import { GameCard, GameCardSkeleton } from "../components/game/GameCard.tsx";
import { GameCover } from "../components/game/GameCover.tsx";
import { PlayButton } from "../components/game/PlayButton.tsx";
import { ScrollItem, ScrollRow } from "../components/game/ScrollRow.tsx";
import { Button, ButtonLink } from "../components/ui/Button.tsx";
import { EmptyState, ErrorState, LoadingRegion, ProgressBar, Skeleton } from "../components/ui/Feedback.tsx";
import { Page, Section } from "../components/ui/Page.tsx";
import { useQuery } from "../hooks/useQuery.ts";
import { formatPlaytime, formatRelative } from "../lib/format.ts";
import { useUi } from "../stores/ui.ts";

function greeting(): string {
  const hour = new Date().getHours();
  const h = messages().home;
  if (hour < 11) return h.morning;
  if (hour < 18) return h.hello;
  return h.evening;
}

function ContinueCard({ item }: { item: LibraryItem }) {
  const t = useMessages();
  const { game, entry } = item;
  return (
    <article className="group overflow-hidden rounded-lg border border-border bg-surface transition-colors duration-150 ease-out hover:border-border-strong">
      <Link to={`/library/${game.id}`} className="block focus-visible:-outline-offset-2" aria-label={t.home.openGame(game.title)}>
        <GameCover game={game} kind="header" aspect="landscape" titleSize="xl" className="rounded-none" />
      </Link>
      <div className="flex items-center gap-4 p-4">
        <div className="min-w-0 flex-1">
          <h3 className="truncate text-base font-medium text-fg">{game.title}</h3>
          <p className="mt-1 truncate text-sm text-fg-muted">
            <span className="num">{formatPlaytime(entry.playtimeMinutes)}</span>
            {entry.lastPlayedAt && <> · {formatRelative(entry.lastPlayedAt)}</>}
          </p>
        </div>
        <PlayButton item={item} />
      </div>
    </article>
  );
}

function ContinuePlaying() {
  const t = useMessages();
  const query = useQuery("home/continue", api.home.getContinuePlaying);
  const openAddGame = useUi((s) => s.openAddGame);

  return (
    <Section title={t.home.continueTitle}>
      {query.status === "loading" && (
        <LoadingRegion label={t.home.continueLoading} className="grid grid-cols-1 gap-5 md:grid-cols-2 xl:grid-cols-3">
          {[0, 1, 2].map((i) => (
            <div key={i} className="overflow-hidden rounded-lg border border-border">
              <Skeleton className="aspect-video rounded-none" />
              <div className="flex items-center gap-4 p-4">
                <div className="flex-1">
                  <Skeleton className="h-5 w-2/3" />
                  <Skeleton className="mt-2 h-4 w-1/2" />
                </div>
                <Skeleton className="h-9 w-24" />
              </div>
            </div>
          ))}
        </LoadingRegion>
      )}
      {query.status === "error" && <ErrorState message={query.error} onRetry={query.retry} />}
      {query.status === "success" && query.data.length === 0 && (
        <div className="rounded-lg border border-border">
          <EmptyState
            icon={Library}
            title={t.home.welcomeTitle}
            description={t.home.welcomeText}
            actions={
              <>
                <ButtonLink to="/settings/accounts">{t.home.connectStore}</ButtonLink>
                <Button
                  variant="primary"
                  icon={Plus}
                  onClick={() => {
                    openAddGame();
                  }}
                >
                  {t.palette.addGame}
                </Button>
              </>
            }
          />
        </div>
      )}
      {query.status === "success" && query.data.length > 0 && (
        <div className="grid grid-cols-1 gap-5 md:grid-cols-2 xl:grid-cols-3">
          {query.data.map((item) => (
            <ContinueCard key={item.game.id} item={item} />
          ))}
        </div>
      )}
    </Section>
  );
}

function RowSkeleton({ label }: { label: string }) {
  return (
    <LoadingRegion label={label} className="flex gap-5 overflow-hidden">
      {Array.from({ length: 8 }, (_, i) => (
        <div key={i} className="w-40 shrink-0">
          <GameCardSkeleton />
        </div>
      ))}
    </LoadingRegion>
  );
}

function Recommended() {
  const t = useMessages();
  const query = useQuery("home/recommended", api.home.getRecommendations);
  return (
    <ScrollRow title={t.home.recommendedTitle} description={t.home.recommendedText}>
      {query.status === "loading" && <RowSkeleton label={t.home.recommendedLoading} />}
      {query.status === "error" && <ErrorState message={query.error} onRetry={query.retry} />}
      {query.status === "success" && query.data.length === 0 && (
        <div className="w-full rounded-lg border border-border">
          <EmptyState
            compact
            icon={Compass}
            title={t.home.recommendedEmpty}
            description={t.home.recommendedEmptyText}
            actions={<ButtonLink to="/catalogue">{t.home.browseCatalogue}</ButtonLink>}
          />
        </div>
      )}
      {query.status === "success" &&
        query.data.map((rec) => (
          <ScrollItem key={rec.game.id}>
            <GameCard game={rec.game} to={`/catalogue/${rec.game.id}`} meta={rec.reason} />
          </ScrollItem>
        ))}
    </ScrollRow>
  );
}

function Discover() {
  const t = useMessages();
  const query = useQuery("home/discover", api.home.getDiscover);
  return (
    <ScrollRow title={t.home.discoverTitle} description={t.home.discoverText}>
      {query.status === "loading" && <RowSkeleton label={t.home.discoverLoading} />}
      {query.status === "error" && <ErrorState message={query.error} onRetry={query.retry} />}
      {query.status === "success" && query.data.length === 0 && (
        <p className="py-6 text-sm text-fg-muted">{t.home.discoverEmpty}</p>
      )}
      {query.status === "success" &&
        query.data.map((rec) => (
          <ScrollItem key={rec.game.id}>
            <GameCard game={rec.game} to={`/catalogue/${rec.game.id}`} meta={rec.reason} />
          </ScrollItem>
        ))}
    </ScrollRow>
  );
}

function RecentAchievements() {
  const t = useMessages();
  const query = useQuery("home/achievements", api.home.getRecentAchievements);
  return (
    <Section title={t.home.achievementsTitle}>
      {query.status === "loading" && (
        <LoadingRegion label={t.home.achievementsLoading} className="grid grid-cols-1 gap-x-8 md:grid-cols-2">
          {Array.from({ length: 6 }, (_, i) => (
            <div key={i} className="flex items-center gap-3 border-b border-border py-3">
              <Skeleton className="size-10" />
              <div className="flex-1">
                <Skeleton className="h-4 w-1/2" />
                <Skeleton className="mt-2 h-3 w-1/3" />
              </div>
            </div>
          ))}
        </LoadingRegion>
      )}
      {query.status === "error" && <ErrorState message={query.error} onRetry={query.retry} />}
      {query.status === "success" && query.data.length === 0 && (
        <div className="rounded-lg border border-border">
          <EmptyState compact icon={Trophy} title={t.home.achievementsEmpty} description={t.home.achievementsEmptyText} />
        </div>
      )}
      {query.status === "success" && query.data.length > 0 && (
        <ul className="grid grid-cols-1 gap-x-8 md:grid-cols-2">
          {query.data.map(({ achievement, game }) => (
            <li key={achievement.id}>
              <Link
                to={`/library/${game.id}?tab=achievements`}
                className="flex items-center gap-3 border-b border-border py-3 transition-colors duration-150 ease-out hover:bg-hover focus-visible:outline-offset-0 md:-mx-2 md:px-2"
              >
                <div className="flex size-10 shrink-0 items-center justify-center rounded-md border border-border bg-surface text-accent-text">
                  <Trophy aria-hidden />
                </div>
                <div className="min-w-0 flex-1">
                  <p className="truncate text-sm font-medium text-fg">{achievement.title}</p>
                  <p className="truncate text-xs text-fg-muted">{game.title}</p>
                </div>
                {achievement.unlockedAt && (
                  <time dateTime={achievement.unlockedAt} className="shrink-0 text-xs text-fg-subtle">
                    {formatRelative(achievement.unlockedAt)}
                  </time>
                )}
              </Link>
            </li>
          ))}
        </ul>
      )}
    </Section>
  );
}

function CoveAchievements() {
  const t = useMessages();
  const query = useQuery("home/cove-achievements", api.accounts.listCoveAchievements);
  return (
    <Section title={t.home.coveTitle}>
      {query.status === "loading" && (
        <LoadingRegion label={t.home.coveLoading} className="grid grid-cols-1 gap-4 md:grid-cols-3">
          {[0, 1, 2].map((i) => (
            <Skeleton key={i} className="h-24" />
          ))}
        </LoadingRegion>
      )}
      {query.status === "error" && <ErrorState message={query.error} onRetry={query.retry} />}
      {query.status === "success" && query.data.length === 0 && (
        <div className="rounded-lg border border-border">
          <EmptyState compact icon={Medal} title={t.home.coveEmpty} description={t.home.coveEmptyText} />
        </div>
      )}
      {query.status === "success" && query.data.length > 0 && (
        <ul className="grid grid-cols-1 gap-4 md:grid-cols-3">
          {query.data.map((a) => (
            <li key={a.id} className="rounded-xl border border-border bg-surface p-4">
              <div className="flex items-start gap-3">
                <Medal aria-hidden className={a.unlockedAt ? "shrink-0 text-accent-text" : "shrink-0 text-fg-subtle"} />
                <div className="min-w-0 flex-1">
                  <p className={a.unlockedAt ? "truncate text-sm font-medium text-fg" : "truncate text-sm font-medium text-fg-muted"}>{a.title}</p>
                  <p className="text-xs text-fg-subtle">{a.description}</p>
                </div>
              </div>
              {a.unlockedAt ? (
                <p className="mt-3 text-xs text-fg-muted">
                  {t.home.unlocked} <time dateTime={a.unlockedAt}>{formatRelative(a.unlockedAt)}</time>
                </p>
              ) : (
                <div className="mt-3">
                  <p className="num text-xs text-fg-muted">
                    {a.current} / {a.target}
                  </p>
                  <ProgressBar value={a.current} max={a.target} label={t.home.progressOf(a.title)} className="mt-2" />
                </div>
              )}
            </li>
          ))}
        </ul>
      )}
    </Section>
  );
}

export function HomePage() {
  const t = useMessages();
  return (
    <Page>
      <header className="mb-8">
        <h1 className="text-xl font-semibold tracking-tight text-fg">{greeting()}</h1>
        <p className="mt-1 text-sm text-fg-muted">{t.home.subtitle}</p>
      </header>
      <ContinuePlaying />
      <Recommended />
      <Discover />
      <RecentAchievements />
      <CoveAchievements />
    </Page>
  );
}
