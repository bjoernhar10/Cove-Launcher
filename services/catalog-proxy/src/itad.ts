/**
 * Prices from the IsThereAnyDeal API (https://docs.isthereanydeal.com/).
 * The API key is a Worker secret (ITAD_API_KEY); clients send titles and a
 * country only. Deal links are passed on unchanged, as ITAD's terms require.
 */
import type { GamePrice, Money, PriceDeal } from "@cove/shared";
import type { Env } from "./env.ts";
import { UpstreamError } from "./igdb-client.ts";

export const ITAD_API = "https://api.isthereanydeal.com";
/** Titles looked up one by one when the exact title lookup misses (rate limit: 1000 per 5 minutes). */
const FUZZY_LOOKUPS = 10;
const DEALS_SHOWN = 5;

interface ItadPrice {
  amount: number;
  currency: string;
}

interface ItadDeal {
  shop: { id: number; name: string };
  price: ItadPrice;
  regular: ItadPrice;
  cut: number;
  expiry: string | null;
  url: string;
}

interface ItadPrices {
  id: string;
  historyLow: { all: ItadPrice | null } | null;
  deals: ItadDeal[];
}

interface ItadOverview {
  prices: { id: string; urls: { game: string } }[];
}

export class ItadClient {
  private readonly fetchFn: typeof fetch;

  constructor(
    private readonly env: Env,
    fetchFn: typeof fetch = (input, init) => fetch(input, init),
  ) {
    this.fetchFn = (input, init) => fetchFn(input, init);
  }

  private async call<T>(path: string, query: Record<string, string>, body?: unknown): Promise<T> {
    if (!this.env.ITAD_API_KEY) throw new UpstreamError(500, "ITAD_API_KEY is not configured (wrangler secret put).");
    const url = new URL(`${ITAD_API}${path}`);
    for (const [k, v] of Object.entries(query)) url.searchParams.set(k, v);
    const init: RequestInit = {
      method: body === undefined ? "GET" : "POST",
      headers: { "ITAD-API-Key": this.env.ITAD_API_KEY, Accept: "application/json", "Content-Type": "application/json" },
    };
    if (body !== undefined) init.body = JSON.stringify(body);
    const response = await this.fetchFn(url, init);
    if (response.status === 429 || response.status >= 500) {
      throw new UpstreamError(response.status === 429 ? 503 : 502, `IsThereAnyDeal answered HTTP ${String(response.status)}`);
    }
    if (!response.ok) throw new UpstreamError(502, `IsThereAnyDeal answered HTTP ${String(response.status)}`);
    return response.json<T>();
  }

  /** Title → ITAD game id: an exact lookup for all, then a few one-by-one lookups for the rest. */
  async ids(titles: string[]): Promise<Map<string, string>> {
    const found = new Map<string, string>();
    const exact = await this.call<Record<string, string | null>>("/lookup/id/title/v1", {}, titles);
    for (const title of titles) {
      const id = exact[title];
      if (id) found.set(title, id);
    }
    for (const title of titles.filter((t) => !found.has(t)).slice(0, FUZZY_LOOKUPS)) {
      const result = await this.call<{ found: boolean; game?: { id: string; type?: string | null } }>("/games/lookup/v1", { title });
      if (result.found && result.game && (result.game.type ?? "game") === "game") found.set(title, result.game.id);
    }
    return found;
  }

  /** Prices of ITAD games in a country (at most 200 ids). */
  async prices(ids: string[], country: string, now: Date): Promise<Map<string, GamePrice>> {
    const out = new Map<string, GamePrice>();
    if (ids.length === 0) return out;
    const [prices, overview] = await Promise.all([
      this.call<ItadPrices[]>("/games/prices/v3", { country }, ids),
      this.call<ItadOverview>("/games/overview/v2", { country }, ids),
    ]);
    const urls = new Map(overview.prices.map((p) => [p.id, p.urls.game]));
    for (const entry of prices) {
      const deals = entry.deals.map(toDeal).sort((a, b) => a.price.amount - b.price.amount);
      out.set(entry.id, {
        url: urls.get(entry.id) ?? null,
        best: deals[0] ?? null,
        deals: deals.slice(0, DEALS_SHOWN),
        historyLow: entry.historyLow?.all ? money(entry.historyLow.all) : null,
        checkedAt: now.toISOString(),
      });
    }
    return out;
  }
}

function money(price: ItadPrice): Money {
  return { amount: price.amount, currency: price.currency };
}

function toDeal(deal: ItadDeal): PriceDeal {
  return {
    shop: deal.shop.name,
    price: money(deal.price),
    regular: money(deal.regular),
    cut: Math.max(0, Math.min(100, Math.round(deal.cut))),
    url: deal.url,
    expiry: deal.expiry,
  };
}
