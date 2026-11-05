import { messages } from "../../i18n/index.ts";
import type { Game } from "@cove/shared";
import type { ReactNode } from "react";
import { Link } from "react-router";
import { GENRE_LABELS } from "../../lib/labels.ts";
import { formatYear } from "../../lib/format.ts";
import { Skeleton } from "../ui/Feedback.tsx";
import { GameCover } from "./GameCover.tsx";

interface GameCardProps {
  game: Game;
  to: string;
  /** Replaces the default "Genre · Jahr" line (e.g. a recommendation reason). */
  meta?: ReactNode | undefined;
}

/** Portrait cover + title. The whole card is one link. */
export function GameCard({ game, to, meta }: GameCardProps) {
  const genre = game.genres[0];
  return (
    <Link to={to} className="group block rounded-lg focus-visible:outline-offset-4">
      <GameCover game={game} className="border border-border transition-colors duration-150 ease-out group-hover:border-border-strong" />
      <div className="mt-3 min-w-0">
        <p className="truncate text-sm font-medium text-fg">{game.title}</p>
        <p className="mt-1 truncate text-xs text-fg-muted">
          {meta ?? (
            <>
              {genre ? GENRE_LABELS[genre] : messages().library.colGame}
              {game.releaseDate && (
                <>
                  {" · "}
                  <span className="num">{formatYear(game.releaseDate)}</span>
                </>
              )}
            </>
          )}
        </p>
      </div>
    </Link>
  );
}

export function GameCardSkeleton() {
  return (
    <div>
      <Skeleton className="aspect-[3/4] rounded-lg" />
      <Skeleton className="mt-3 h-4 w-3/4" />
      <Skeleton className="mt-2 h-3 w-1/2" />
    </div>
  );
}

/** Responsive cover grid used by Catalogue and Library (keep in sync with the constants below). */
export const COVER_GRID = "grid grid-cols-[repeat(auto-fill,minmax(160px,1fr))] gap-x-5 gap-y-8";
/** Minimum column width of `COVER_GRID` in px. */
export const COVER_GRID_MIN = 160;
/** Gaps of `COVER_GRID` in px (`gap-x-5`, `gap-y-8`). */
export const COVER_GRID_GAP = { x: 20, y: 32 } as const;
