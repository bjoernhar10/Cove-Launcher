import { useMessages } from "../i18n/index.ts";
import type { Game } from "@cove/shared";
import { ArrowLeft, Check, ChevronLeft, ChevronRight, Layers, Plus, SearchX } from "lucide-react";
import { useRef, useState, type ReactNode } from "react";
import { useParams } from "react-router";
import { api, CoveError } from "../api/index.ts";
import { GameCard, GameCardSkeleton } from "../components/game/GameCard.tsx";
import { GameCover } from "../components/game/GameCover.tsx";
import { ScrollItem, ScrollRow } from "../components/game/ScrollRow.tsx";
import { Button, ButtonLink } from "../components/ui/Button.tsx";
import { Dialog } from "../components/ui/Dialog.tsx";
import { Badge, EmptyState, ErrorState, LoadingRegion, OfflineBadge, Skeleton } from "../components/ui/Feedback.tsx";
import { CatalogSetupState } from "./CataloguePage.tsx";
import { Page } from "../components/ui/Page.tsx";
import { useCatalogGame, useSimilarGames } from "../hooks/useCatalog.ts";
import { formatDate } from "../lib/format.ts";
import { GENRE_LABELS, PLATFORM_LABELS } from "../lib/labels.ts";
import { invalidateQueries } from "../stores/query.ts";
import { errorMessage, toast } from "../stores/toasts.ts";

function Fact({ label, children }: { label: string; children: ReactNode }) {
  return (
    <div className="flex flex-col gap-1 border-b border-border py-3 last:border-b-0">
      <dt className="text-xs text-fg-subtle">{label}</dt>
      <dd className="m-0 text-sm text-fg">{children}</dd>
    </div>
  );
}

function Screenshots({ game }: { game: Game }) {
  const t = useMessages();
  const [index, setIndex] = useState<number | null>(null);
  const shots = game.artwork.screenshots;
  if (shots.length === 0) return null;
  const current = index === null ? null : shots[index];

  return (
    <section aria-label="Screenshots" className="mt-10">
      <h2 className="mb-4 text-base font-semibold text-fg">Screenshots</h2>
      <ul className="grid grid-cols-2 gap-3 xl:grid-cols-3">
        {shots.map((shot, i) => (
          <li key={shot.url ?? i}>
            <button
              type="button"
              className="block w-full rounded-lg"
              aria-label={t.detail.enlarge(t.cover.screenshot(i + 1, game.title))}
              onClick={() => {
                setIndex(i);
              }}
            >
              <GameCover
                game={{ ...game, artwork: { ...game.artwork, header: shot } }}
                kind="header"
                aspect="landscape"
                showTitle={false}
                variant={i + 1}
                alt={t.cover.screenshot(i + 1, game.title)}
                className="border border-border transition-colors duration-150 ease-out hover:border-border-strong"
              />
            </button>
          </li>
        ))}
      </ul>
      <Dialog
        open={current !== null && current !== undefined}
        onClose={() => {
          setIndex(null);
        }}
        title={index === null ? "Screenshot" : t.cover.screenshot(index + 1, game.title)}
        className="w-[960px]"
        footer={
          <>
            <Button
              icon={ChevronLeft}
              disabled={index === 0}
              onClick={() => {
                setIndex((i) => (i === null ? i : Math.max(0, i - 1)));
              }}
            >
              {t.topbar.back}
            </Button>
            <Button
              icon={ChevronRight}
              disabled={index === shots.length - 1}
              onClick={() => {
                setIndex((i) => (i === null ? i : Math.min(shots.length - 1, i + 1)));
              }}
            >
              {t.detail.next}
            </Button>
          </>
        }
      >
        {current && index !== null && (
          <GameCover
            game={{ ...game, artwork: { ...game.artwork, header: current } }}
            kind="header"
            aspect="landscape"
            showTitle={false}
            variant={index + 1}
            alt={t.cover.screenshot(index + 1, game.title)}
          />
        )}
      </Dialog>
    </section>
  );
}

function DetailSkeleton() {
  const t = useMessages();
  return (
    <LoadingRegion label={t.detail.loading}>
      <Skeleton className="aspect-[3/1] rounded-lg" />
      <div className="mt-8 grid grid-cols-1 gap-12 lg:grid-cols-[1fr_280px]">
        <div>
          <Skeleton className="h-9 w-1/2" />
          <Skeleton className="mt-3 h-4 w-1/3" />
          <Skeleton className="mt-8 h-4 w-full" />
          <Skeleton className="mt-2 h-4 w-11/12" />
          <Skeleton className="mt-2 h-4 w-3/4" />
        </div>
        <Skeleton className="h-64" />
      </div>
    </LoadingRegion>
  );
}

export function CatalogueDetailPage() {
  const t = useMessages();
  const { gameId = "" } = useParams();
  const details = useCatalogGame(gameId);
  const similar = useSimilarGames(gameId);
  const [adding, setAdding] = useState(false);
  const similarRef = useRef<HTMLDivElement>(null);
  const game = details.data?.game;
  const libraryGameId = details.data?.libraryGameId ?? null;

  async function addToLibrary(): Promise<void> {
    if (!game) return;
    setAdding(true);
    try {
      await api.library.addGameFromCatalog(game);
      toast(t.detail.added(game.title));
      invalidateQueries();
    } catch (error: unknown) {
      toast(errorMessage(error), "danger");
    } finally {
      setAdding(false);
    }
  }

  function showSimilar(): void {
    const reduce = globalThis.matchMedia("(prefers-reduced-motion: reduce)").matches;
    similarRef.current?.scrollIntoView({ behavior: reduce ? "auto" : "smooth", block: "start" });
    similarRef.current?.querySelector<HTMLElement>("a")?.focus({ preventScroll: true });
  }

  const notConfigured = details.error instanceof CoveError && details.error.kind === "catalogNotConfigured";

  return (
    <Page>
      <ButtonLink to="/catalogue" variant="ghost" size="sm" icon={ArrowLeft} className="-ml-3 mb-4">
        Catalogue
      </ButtonLink>

      {details.isPending && <DetailSkeleton />}
      {notConfigured && <CatalogSetupState />}
      {details.isError && !notConfigured && (
        <EmptyState
          icon={SearchX}
          title={t.detail.unavailable}
          description={errorMessage(details.error)}
          actions={
            <>
              <Button onClick={() => void details.refetch()}>{t.common.retry}</Button>
              <ButtonLink to="/catalogue">{t.detail.toCatalogue}</ButtonLink>
            </>
          }
        />
      )}
      {game && (
        <>
          <GameCover game={game} kind="header" aspect="wide" showTitle={false} className="border border-border" />

          <div className="mt-8 grid grid-cols-1 gap-12 lg:grid-cols-[1fr_280px]">
            <div className="min-w-0">
              <div className="flex flex-wrap items-center gap-3">
                <h1 className="text-xl font-semibold tracking-tight text-fg">{game.title}</h1>
                {details.data?.offline && <OfflineBadge />}
              </div>
              <p className="mt-1 text-sm text-fg-muted">
                {[game.developer, game.releaseDate?.slice(0, 4)].filter(Boolean).join(" · ") || t.detail.noDetails}
              </p>

              <div className="mt-6 flex flex-wrap gap-2">
                {libraryGameId ? (
                  <ButtonLink to={`/library/${libraryGameId}`} variant="secondary" icon={Check}>
                    {t.palette.inLibrary}
                  </ButtonLink>
                ) : (
                  <Button variant="primary" icon={Plus} disabled={adding} onClick={() => void addToLibrary()}>
                    {t.detail.addToLibrary}
                  </Button>
                )}
                <Button icon={Layers} onClick={showSimilar}>
                  {t.detail.similar}
                </Button>
              </div>

              <div className="mt-8 max-w-2xl space-y-4 text-base leading-relaxed text-fg">
                {game.description ? (
                  game.description.split(/\n\s*\n/).map((paragraph) => <p key={paragraph}>{paragraph}</p>)
                ) : (
                  <p className="text-fg-muted">{t.detail.noDescription}</p>
                )}
              </div>

              <Screenshots game={game} />
            </div>

            <aside aria-label={t.detail.details}>
              <dl className="m-0 rounded-lg border border-border px-4">
                <Fact label="Genres">
                  {game.genres.length > 0 ? (
                    <span className="flex flex-wrap gap-2 pt-1">
                      {game.genres.map((g) => (
                        <Badge key={g}>{GENRE_LABELS[g]}</Badge>
                      ))}
                    </span>
                  ) : (
                    <span className="text-fg-muted">{t.detail.notSpecified}</span>
                  )}
                </Fact>
                <Fact label="Release">
                  {game.releaseDate ? (
                    <time dateTime={game.releaseDate}>{formatDate(game.releaseDate)}</time>
                  ) : (
                    <span className="text-fg-muted">{t.detail.unknown}</span>
                  )}
                </Fact>
                <Fact label={t.detail.developer}>{game.developer || <span className="text-fg-muted">{t.detail.unknown}</span>}</Fact>
                <Fact label="Publisher">{game.publisher || <span className="text-fg-muted">{t.detail.unknown}</span>}</Fact>
                <Fact label={t.detail.platforms}>
                  {game.platforms.length > 0 ? game.platforms.map((p) => PLATFORM_LABELS[p]).join(", ") : t.detail.notSpecified}
                </Fact>
                <Fact label={t.catalogue.rating}>
                  {game.rating === null ? (
                    <span className="text-fg-muted">{t.detail.noRating}</span>
                  ) : (
                    <span className="num">{game.rating} / 100</span>
                  )}
                </Fact>
              </dl>
            </aside>
          </div>

          <div ref={similarRef} className="scroll-mt-4">
            <ScrollRow title={t.detail.similar}>
              {similar.isPending &&
                Array.from({ length: 6 }, (_, i) => (
                  <ScrollItem key={i}>
                    <GameCardSkeleton />
                  </ScrollItem>
                ))}
              {similar.isError && (
                <ErrorState message={errorMessage(similar.error)} onRetry={() => void similar.refetch()} />
              )}
              {similar.isSuccess && similar.data.items.length === 0 && (
                <p className="py-6 text-sm text-fg-muted">{t.detail.noSimilar}</p>
              )}
              {similar.data?.items.map((g) => (
                <ScrollItem key={g.id}>
                  <GameCard game={g} to={`/catalogue/${g.id}`} />
                </ScrollItem>
              ))}
            </ScrollRow>
          </div>
        </>
      )}
    </Page>
  );
}
