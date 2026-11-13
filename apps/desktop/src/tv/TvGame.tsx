import { Star, SearchX } from "lucide-react";
import { useEffect } from "react";
import { useParams } from "react-router";
import { api } from "../api/index.ts";
import { GameCover } from "../components/game/GameCover.tsx";
import { PlayButton } from "../components/game/PlayButton.tsx";
import { Button } from "../components/ui/Button.tsx";
import { EmptyState, ErrorState, LoadingRegion, Skeleton } from "../components/ui/Feedback.tsx";
import { PadGlyph } from "../components/ui/PadGlyph.tsx";
import { onPad } from "../hooks/useGamepadNavigation.ts";
import { useQuery } from "../hooks/useQuery.ts";
import { useMessages } from "../i18n/index.ts";
import { formatPlaytime, formatRelative } from "../lib/format.ts";
import { invalidateQueries } from "../stores/query.ts";
import { errorMessage, toast } from "../stores/toasts.ts";

function Fact({ label, value }: { label: string; value: string }) {
  return (
    <div className="rounded-lg border border-border px-4 py-3">
      <p className="text-xs text-fg-subtle">{label}</p>
      <p className="num mt-1 text-base font-medium text-fg">{value}</p>
    </div>
  );
}

/** A game on the TV: big artwork, one big button. */
export function TvGame() {
  const t = useMessages();
  const { gameId = "" } = useParams();
  const query = useQuery(`library/${gameId}`, () => api.library.getLibraryItem(gameId));
  const item = query.data ?? null;

  async function toggleFavorite(): Promise<void> {
    if (!item) return;
    try {
      await api.library.setFavorite(item.game.id, !item.entry.favorite);
      invalidateQueries();
    } catch (error: unknown) {
      toast(errorMessage(error), "danger");
    }
  }

  useEffect(
    () =>
      onPad((action) => {
        if (action === "secondary") void toggleFavorite();
      }),
  );

  // The big button gets the focus first.
  const shownId = item?.game.id;
  useEffect(() => {
    if (!shownId) return;
    document.querySelector<HTMLElement>("[data-tv-play] button")?.focus({ preventScroll: true });
  }, [shownId]);

  if (query.status === "loading") {
    return (
      <LoadingRegion label={t.common.loading}>
        <Skeleton className="aspect-[3/1]" />
      </LoadingRegion>
    );
  }
  if (query.status === "error") return <ErrorState message={query.error} onRetry={query.retry} />;
  if (!item) return <EmptyState icon={SearchX} title={t.tv.library} description={t.tv.emptyLibrary} />;
  const { game, entry } = item;

  return (
    <div className="flex flex-col gap-8">
      <GameCover game={game} kind="header" aspect="wide" showTitle={false} className="border border-border" />
      <div className="flex flex-wrap items-end justify-between gap-6">
        <div className="min-w-0">
          <h1 className="text-xl font-semibold tracking-tight text-fg">{game.title}</h1>
          {game.summary && <p className="mt-2 max-w-3xl text-base text-fg-muted">{game.summary}</p>}
        </div>
        <div className="flex items-center gap-3">
          <div data-tv-play>
            <PlayButton item={item} />
          </div>
          <Button icon={Star} aria-pressed={entry.favorite} onClick={() => void toggleFavorite()}>
            <span className="flex items-center gap-2">
              {t.tv.favorite}
              <PadGlyph action="secondary" />
            </span>
          </Button>
        </div>
      </div>
      <div className="grid grid-cols-3 gap-4">
        <Fact label={t.tv.playtime} value={formatPlaytime(entry.playtimeMinutes)} />
        <Fact label={t.tv.lastPlayed} value={entry.lastPlayedAt ? formatRelative(entry.lastPlayedAt) : "–"} />
        <Fact label={t.tv.achievements} value={entry.achievements.total > 0 ? `${String(entry.achievements.unlocked)} / ${String(entry.achievements.total)}` : "–"} />
      </div>
    </div>
  );
}
