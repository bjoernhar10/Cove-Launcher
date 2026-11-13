import type { LibraryItem } from "@cove/shared";
import { Link } from "react-router";
import { GameCover } from "../components/game/GameCover.tsx";

/** A large cover that opens the TV game page. */
export function TvCard({ item, className, isDefault = false }: { item: LibraryItem; className?: string; isDefault?: boolean }) {
  return (
    <Link
      to={`/tv/game/${item.game.id}`}
      className={className ?? "block w-40 shrink-0 rounded-lg"}
      {...(isDefault ? { "data-nav-default": "" } : {})}
    >
      <GameCover game={item.game} titleSize="base" className="rounded-lg border border-border" />
      <p className="mt-2 truncate text-sm text-fg">{item.game.title}</p>
    </Link>
  );
}

/** A titled row of covers. */
export function TvRow({ title, items, first = false }: { title: string; items: LibraryItem[]; first?: boolean }) {
  if (items.length === 0) return null;
  return (
    <section aria-label={title} className="mb-8">
      <h2 className="mb-3 text-base font-semibold text-fg">{title}</h2>
      <div className="-m-1 flex gap-4 overflow-x-auto p-1 [scrollbar-width:none]">
        {items.map((item, i) => (
          <TvCard key={item.game.id} item={item} isDefault={first && i === 0} />
        ))}
      </div>
    </section>
  );
}
