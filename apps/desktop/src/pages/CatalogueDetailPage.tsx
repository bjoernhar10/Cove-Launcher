import type { Game } from "@cove/shared";
import { ArrowLeft, Check, ChevronLeft, ChevronRight, Layers, Plus, SearchX } from "lucide-react";
import { useRef, useState, type ReactNode } from "react";
import { useParams } from "react-router";
import { api } from "../api/index.ts";
import { GameCard, GameCardSkeleton } from "../components/game/GameCard.tsx";
import { GameCover } from "../components/game/GameCover.tsx";
import { ScrollItem, ScrollRow } from "../components/game/ScrollRow.tsx";
import { Button, ButtonLink } from "../components/ui/Button.tsx";
import { Dialog } from "../components/ui/Dialog.tsx";
import { Badge, EmptyState, ErrorState, LoadingRegion, Skeleton } from "../components/ui/Feedback.tsx";
import { Page } from "../components/ui/Page.tsx";
import { useQuery } from "../hooks/useQuery.ts";
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
  const [index, setIndex] = useState<number | null>(null);
  const shots = game.artwork.screenshots;
  if (shots.length === 0) return null;
  const current = index === null ? null : shots[index];

  return (
    <section aria-label="Screenshots" className="mt-10">
      <h2 className="mb-4 text-base font-semibold text-fg">Screenshots</h2>
      <ul className="grid grid-cols-2 gap-3 xl:grid-cols-3">
        {shots.map((shot, i) => (
          <li key={shot.alt}>
            <button
              type="button"
              className="block w-full rounded-lg"
              aria-label={`${shot.alt} vergrößern`}
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
        title={current?.alt ?? "Screenshot"}
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
              Zurück
            </Button>
            <Button
              icon={ChevronRight}
              disabled={index === shots.length - 1}
              onClick={() => {
                setIndex((i) => (i === null ? i : Math.min(shots.length - 1, i + 1)));
              }}
            >
              Weiter
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
          />
        )}
      </Dialog>
    </section>
  );
}

function DetailSkeleton() {
  return (
    <LoadingRegion label="Spiel wird geladen">
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
  const { gameId = "" } = useParams();
  const game = useQuery(`catalog/${gameId}`, () => api.catalog.getGame(gameId));
  const owned = useQuery(`library/${gameId}`, () => api.library.getLibraryItem(gameId));
  const similar = useQuery(`catalog/${gameId}/similar`, () => api.catalog.getSimilarGames(gameId));
  const [adding, setAdding] = useState(false);
  const similarRef = useRef<HTMLDivElement>(null);

  async function addToLibrary(): Promise<void> {
    if (game.status !== "success") return;
    setAdding(true);
    try {
      await api.library.addGameFromCatalog(game.data);
      toast(`„${game.data.title}“ wurde zur Bibliothek hinzugefügt`);
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

  return (
    <Page>
      <ButtonLink to="/catalogue" variant="ghost" size="sm" icon={ArrowLeft} className="-ml-3 mb-4">
        Catalogue
      </ButtonLink>

      {game.status === "loading" && <DetailSkeleton />}
      {game.status === "error" && (
        <EmptyState
          icon={SearchX}
          title="Spiel nicht gefunden"
          description="Dieses Spiel gibt es im Katalog nicht (mehr)."
          actions={<ButtonLink to="/catalogue">Zum Katalog</ButtonLink>}
        />
      )}
      {game.status === "success" && (
        <>
          <GameCover game={game.data} kind="header" aspect="wide" showTitle={false} className="border border-border" />

          <div className="mt-8 grid grid-cols-1 gap-12 lg:grid-cols-[1fr_280px]">
            <div className="min-w-0">
              <h1 className="text-xl font-semibold tracking-tight text-fg">{game.data.title}</h1>
              <p className="mt-1 text-sm text-fg-muted">
                {game.data.developer} · <span className="num">{game.data.releaseDate.slice(0, 4)}</span>
              </p>

              <div className="mt-6 flex flex-wrap gap-2">
                {owned.status === "success" && owned.data ? (
                  <ButtonLink to={`/library/${gameId}`} variant="secondary" icon={Check}>
                    In deiner Bibliothek
                  </ButtonLink>
                ) : (
                  <Button variant="primary" icon={Plus} disabled={adding || owned.status === "loading"} onClick={() => void addToLibrary()}>
                    Zur Bibliothek hinzufügen
                  </Button>
                )}
                <Button icon={Layers} onClick={showSimilar}>
                  Ähnliche Spiele
                </Button>
              </div>

              <div className="mt-8 max-w-2xl space-y-4 text-base leading-relaxed text-fg">
                {game.data.description.split("\n\n").map((paragraph) => (
                  <p key={paragraph}>{paragraph}</p>
                ))}
              </div>

              <Screenshots game={game.data} />
            </div>

            <aside aria-label="Details">
              <dl className="m-0 rounded-lg border border-border px-4">
                <Fact label="Genres">
                  <span className="flex flex-wrap gap-1.5 pt-0.5">
                    {game.data.genres.map((g) => (
                      <Badge key={g}>{GENRE_LABELS[g]}</Badge>
                    ))}
                  </span>
                </Fact>
                <Fact label="Release">
                  <time dateTime={game.data.releaseDate}>{formatDate(game.data.releaseDate)}</time>
                </Fact>
                <Fact label="Entwickler">{game.data.developer}</Fact>
                <Fact label="Publisher">{game.data.publisher}</Fact>
                <Fact label="Plattformen">{game.data.platforms.map((p) => PLATFORM_LABELS[p]).join(", ")}</Fact>
                <Fact label="Bewertung">
                  {game.data.rating === null ? (
                    <span className="text-fg-muted">Noch keine Bewertung</span>
                  ) : (
                    <span className="num">{game.data.rating} / 100</span>
                  )}
                </Fact>
              </dl>
            </aside>
          </div>

          <div ref={similarRef} className="scroll-mt-4">
            <ScrollRow title="Ähnliche Spiele">
              {similar.status === "loading" &&
                Array.from({ length: 6 }, (_, i) => (
                  <ScrollItem key={i}>
                    <GameCardSkeleton />
                  </ScrollItem>
                ))}
              {similar.status === "error" && <ErrorState message={similar.error} onRetry={similar.retry} />}
              {similar.status === "success" && similar.data.length === 0 && (
                <p className="py-6 text-sm text-fg-muted">Keine ähnlichen Spiele gefunden.</p>
              )}
              {similar.status === "success" &&
                similar.data.map((g) => (
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
