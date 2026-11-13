import type { WishlistItem } from "@cove/shared";
import { Bookmark, ExternalLink, RefreshCw, Trash2 } from "lucide-react";
import { useState } from "react";
import { Link } from "react-router";
import { api } from "../api/index.ts";
import { GameCover } from "../components/game/GameCover.tsx";
import { Button, ButtonLink } from "../components/ui/Button.tsx";
import { EmptyState, ErrorState, LoadingRegion, Skeleton } from "../components/ui/Feedback.tsx";
import { Input, Select, Switch } from "../components/ui/Field.tsx";
import { Page, PageHeader } from "../components/ui/Page.tsx";
import { BestPrice, PriceAttribution } from "../features/GamePrices.tsx";
import { useQuery } from "../hooks/useQuery.ts";
import { useMessages } from "../i18n/index.ts";
import { formatCountry, formatDate } from "../lib/format.ts";
import { invalidateQueries } from "../stores/query.ts";
import { errorMessage, toast } from "../stores/toasts.ts";
import { useSettings } from "./settings/useSettings.ts";

/** Countries IsThereAnyDeal has prices for (the common ones). */
const COUNTRIES = [
  "AT", "AU", "BE", "BR", "CA", "CH", "CZ", "DE", "DK", "ES", "FI", "FR", "GB", "HU", "IE",
  "IT", "JP", "NL", "NO", "NZ", "PL", "PT", "SE", "TR", "US",
];

/** "9,99" or "9.99" → 9.99; empty → null; anything else → undefined (invalid). */
function parsePrice(text: string): number | null | undefined {
  const trimmed = text.trim().replace(",", ".");
  if (trimmed === "") return null;
  if (!/^\d{1,6}(\.\d{1,2})?$/.test(trimmed)) return undefined;
  return Number(trimmed);
}

function Row({ item }: { item: WishlistItem }) {
  const t = useMessages();
  const { game, price } = item;
  const currency = price?.best?.price.currency ?? price?.historyLow?.currency ?? "";
  const [target, setTarget] = useState(item.targetPrice === null ? "" : String(item.targetPrice));
  const [busy, setBusy] = useState(false);

  async function save(targetPrice: number | null, notifyOnSale: boolean): Promise<void> {
    setBusy(true);
    try {
      await api.wishlist.update(game.id, targetPrice, notifyOnSale);
      invalidateQueries();
    } catch (error: unknown) {
      toast(errorMessage(error), "danger");
    } finally {
      setBusy(false);
    }
  }

  function saveTarget(): void {
    const parsed = parsePrice(target);
    if (parsed === undefined) {
      toast(t.wishlist.invalidTarget, "danger");
      return;
    }
    if (parsed !== item.targetPrice) void save(parsed, item.notifyOnSale);
  }

  async function remove(): Promise<void> {
    try {
      await api.wishlist.remove(game.id);
      toast(t.wishlist.removed(game.title));
      invalidateQueries();
    } catch (error: unknown) {
      toast(errorMessage(error), "danger");
    }
  }

  const best = price?.best ?? null;
  return (
    <li className="flex flex-wrap items-center gap-6 border-b border-border py-4 last:border-b-0">
      <Link to={`/catalogue/${game.id}`} aria-label={game.title} className="shrink-0 rounded-md">
        <GameCover game={game} showTitle={false} className="w-16 rounded-md" />
      </Link>
      <div className="flex min-w-48 flex-1 flex-col gap-2">
        <Link to={`/catalogue/${game.id}`} className="truncate text-base font-medium text-fg hover:underline">
          {game.title}
        </Link>
        {price ? (
          <BestPrice price={price} />
        ) : (
          <p className="text-sm text-fg-muted">{t.wishlist.notChecked}</p>
        )}
        {best?.expiry && best.cut > 0 && <p className="text-xs text-fg-subtle">{t.wishlist.expires(formatDate(best.expiry))}</p>}
      </div>
      <div className="flex flex-col gap-2">
        <label className="flex items-center gap-2 text-sm text-fg-muted">
          {t.wishlist.target}
          <Input
            aria-label={t.wishlist.targetFor(game.title)}
            inputMode="decimal"
            placeholder="–"
            className="num w-24"
            value={target}
            disabled={busy}
            onChange={(e) => {
              setTarget(e.target.value);
            }}
            onBlur={saveTarget}
            onKeyDown={(e) => {
              if (e.key === "Enter") saveTarget();
            }}
          />
          <span className="w-8 text-xs text-fg-subtle">{currency}</span>
        </label>
        <div className="flex items-center gap-3 text-sm text-fg-muted">
          <Switch
            label={t.wishlist.onSale}
            checked={item.notifyOnSale}
            disabled={busy}
            onChange={(notifyOnSale) => void save(item.targetPrice, notifyOnSale)}
          />
          <span aria-hidden>{t.wishlist.onSale}</span>
        </div>
      </div>
      <div className="flex items-center gap-2">
        {best && (
          <Button icon={ExternalLink} onClick={() => void api.system.openExternal(best.url)} aria-label={t.wishlist.toDealAt(best.shop, game.title)}>
            {t.wishlist.toDeal}
          </Button>
        )}
        <Button variant="ghost" icon={Trash2} iconOnly label={t.wishlist.remove(game.title)} onClick={() => void remove()} />
      </div>
    </li>
  );
}

function WishlistSkeleton() {
  const t = useMessages();
  return (
    <LoadingRegion label={t.wishlist.loading}>
      {Array.from({ length: 4 }, (_, i) => (
        <div key={i} className="flex items-center gap-6 border-b border-border py-4">
          <Skeleton className="aspect-[3/4] w-16 rounded-md" />
          <div className="flex flex-1 flex-col gap-2">
            <Skeleton className="h-5 w-48" />
            <Skeleton className="h-4 w-32" />
          </div>
        </div>
      ))}
    </LoadingRegion>
  );
}

/** Country for prices and whether to check in the background. */
function PriceSettings() {
  const t = useMessages();
  const { settings, save } = useSettings();
  if (!settings) return null;
  const { country, priceAlerts } = settings.wishlist;
  const codes = COUNTRIES.includes(country) ? COUNTRIES : [...COUNTRIES, country].sort();
  return (
    <div className="flex flex-wrap items-center gap-6">
      <label className="flex items-center gap-2 text-sm text-fg-muted">
        {t.wishlistSettings.country}
        <Select
          className="w-48"
          value={country}
          onChange={(value) => {
            save((s) => ({ ...s, wishlist: { ...s.wishlist, country: value } }));
          }}
          options={codes.map((code) => ({ value: code, label: formatCountry(code) }))}
        />
      </label>
      <div className="flex items-center gap-3 text-sm text-fg-muted">
        <Switch
          label={t.wishlistSettings.alerts}
          checked={priceAlerts}
          onChange={(value) => {
            save((s) => ({ ...s, wishlist: { ...s.wishlist, priceAlerts: value } }));
          }}
        />
        <span aria-hidden>{t.wishlistSettings.alerts}</span>
      </div>
    </div>
  );
}

export function WishlistPage() {
  const t = useMessages();
  const query = useQuery("wishlist", api.wishlist.list);
  const [checking, setChecking] = useState(false);

  async function check(): Promise<void> {
    setChecking(true);
    try {
      await api.wishlist.refresh();
      toast(t.wishlist.checked);
      invalidateQueries();
    } catch (error: unknown) {
      toast(errorMessage(error), "danger");
    } finally {
      setChecking(false);
    }
  }

  const items = query.data ?? [];
  return (
    <Page>
      <PageHeader
        title={t.wishlist.title}
        description={t.wishlist.description}
        actions={
          items.length > 0 && (
            <Button icon={RefreshCw} disabled={checking} onClick={() => void check()}>
              {t.wishlist.check}
            </Button>
          )
        }
      />
      <div className="mb-6">
        <PriceSettings />
      </div>
      {query.status === "loading" && <WishlistSkeleton />}
      {query.status === "error" && <ErrorState message={query.error} onRetry={query.retry} />}
      {query.status === "success" && items.length === 0 && (
        <EmptyState
          icon={Bookmark}
          title={t.wishlist.emptyTitle}
          description={t.wishlist.emptyDescription}
          actions={<ButtonLink to="/catalogue">{t.wishlist.toCatalogue}</ButtonLink>}
        />
      )}
      {items.length > 0 && (
        <>
          <ul className="border-t border-border">
            {items.map((item) => (
              <Row key={item.game.id} item={item} />
            ))}
          </ul>
          <div className="mt-6">
            <PriceAttribution />
          </div>
        </>
      )}
    </Page>
  );
}
