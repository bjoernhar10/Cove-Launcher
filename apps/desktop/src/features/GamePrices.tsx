import type { GamePrice, PriceDeal } from "@cove/shared";
import { Bookmark, BookmarkCheck, ExternalLink } from "lucide-react";
import { useState } from "react";
import { api } from "../api/index.ts";
import { Button } from "../components/ui/Button.tsx";
import { ErrorState, LoadingRegion, Skeleton } from "../components/ui/Feedback.tsx";
import { useQuery } from "../hooks/useQuery.ts";
import { useMessages } from "../i18n/index.ts";
import { formatMoney } from "../lib/format.ts";
import { invalidateQueries } from "../stores/query.ts";
import { errorMessage, toast } from "../stores/toasts.ts";

export const ITAD_URL = "https://isthereanydeal.com/";

/** "Preise von IsThereAnyDeal" (their terms ask for the mention). */
export function PriceAttribution() {
  const t = useMessages();
  return (
    <button
      type="button"
      className="text-xs text-fg-subtle underline-offset-2 hover:text-fg-muted hover:underline"
      onClick={() => void api.system.openExternal(ITAD_URL)}
    >
      {t.wishlist.attribution}
    </button>
  );
}

/** "−60 %" next to a price. */
export function Cut({ cut }: { cut: number }) {
  if (cut <= 0) return null;
  return <span className="num rounded-md bg-accent px-2 py-1 text-xs font-medium text-on-accent">−{cut} %</span>;
}

/** Best price, regular price and shop; the line under it shows the lowest price ever. */
export function BestPrice({ price }: { price: GamePrice }) {
  const t = useMessages();
  const best = price.best;
  if (!best) return <p className="text-sm text-fg-muted">{t.wishlist.noPrice}</p>;
  return (
    <div className="flex flex-col gap-1">
      <p className="flex flex-wrap items-center gap-2">
        <span className="num text-base font-semibold text-fg">{formatMoney(best.price.amount, best.price.currency)}</span>
        <Cut cut={best.cut} />
        <span className="text-sm text-fg-muted">{t.wishlist.at(best.shop)}</span>
      </p>
      <p className="num text-xs text-fg-subtle">
        {best.cut > 0 && `${t.wishlist.regular(formatMoney(best.regular.amount, best.regular.currency))} · `}
        {price.historyLow && t.wishlist.historyLow(formatMoney(price.historyLow.amount, price.historyLow.currency))}
      </p>
    </div>
  );
}

function DealRow({ deal, title }: { deal: PriceDeal; title: string }) {
  const t = useMessages();
  return (
    <li className="flex items-center justify-between gap-3 border-b border-border py-2 last:border-b-0">
      <span className="min-w-0 truncate text-sm text-fg">{deal.shop}</span>
      <span className="flex shrink-0 items-center gap-2">
        <Cut cut={deal.cut} />
        <span className="num text-sm text-fg">{formatMoney(deal.price.amount, deal.price.currency)}</span>
        <Button
          variant="ghost"
          size="sm"
          icon={ExternalLink}
          iconOnly
          label={t.wishlist.toDealAt(deal.shop, title)}
          onClick={() => void api.system.openExternal(deal.url)}
        />
      </span>
    </li>
  );
}

/** "Preise" on a catalogue game page: current deals from IsThereAnyDeal. */
export function GamePrices({ gameId, title }: { gameId: string; title: string }) {
  const t = useMessages();
  const query = useQuery(`price/${gameId}`, () => api.wishlist.gamePrice(gameId));
  return (
    <section aria-labelledby={`prices-${gameId}`}>
      <h2 id={`prices-${gameId}`} className="mb-3 text-base font-semibold text-fg">
        {t.wishlist.prices}
      </h2>
      {query.status === "loading" && (
        <LoadingRegion label={t.wishlist.pricesLoading}>
          <Skeleton className="h-28" />
        </LoadingRegion>
      )}
      {query.status === "error" && <ErrorState message={query.error} onRetry={query.retry} />}
      {query.status === "success" && (
        <div className="rounded-lg border border-border px-4 py-3">
          {query.data ? (
            <>
              <BestPrice price={query.data} />
              {query.data.deals.length > 1 && (
                <ul className="mt-3 border-t border-border">
                  {query.data.deals.map((deal) => (
                    <DealRow key={`${deal.shop}-${deal.url}`} deal={deal} title={title} />
                  ))}
                </ul>
              )}
              {query.data.deals.length === 1 && query.data.best && (
                <Button
                  size="sm"
                  icon={ExternalLink}
                  className="mt-3"
                  onClick={() => {
                    if (query.data?.best) void api.system.openExternal(query.data.best.url);
                  }}
                >
                  {t.wishlist.toDeal}
                </Button>
              )}
            </>
          ) : (
            <p className="text-sm text-fg-muted">{t.wishlist.noPriceHint}</p>
          )}
        </div>
      )}
      <div className="mt-2">
        <PriceAttribution />
      </div>
    </section>
  );
}

/** "Auf die Wunschliste" / "Auf der Wunschliste" on a catalogue game page. */
export function WishlistButton({ gameId, title }: { gameId: string; title: string }) {
  const t = useMessages();
  const list = useQuery("wishlist", api.wishlist.list);
  const [busy, setBusy] = useState(false);
  const onList = list.status === "success" && list.data.some((i) => i.game.id === gameId);

  async function toggle(): Promise<void> {
    setBusy(true);
    try {
      if (onList) {
        await api.wishlist.remove(gameId);
        toast(t.wishlist.removed(title));
      } else {
        await api.wishlist.add(gameId);
        toast(t.wishlist.added(title), "neutral", { action: { label: t.wishlist.open, to: "/wishlist" } });
      }
      invalidateQueries();
    } catch (error: unknown) {
      toast(errorMessage(error), "danger");
    } finally {
      setBusy(false);
    }
  }

  return (
    <Button icon={onList ? BookmarkCheck : Bookmark} aria-pressed={onList} disabled={busy || list.status !== "success"} onClick={() => void toggle()}>
      {onList ? t.wishlist.onList : t.wishlist.add}
    </Button>
  );
}
