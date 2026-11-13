import type { GameId, GamePrice, PriceAlert, WishlistItem } from "@cove/shared";
import { call, subscribe } from "./transport.ts";

export function list(): Promise<WishlistItem[]> {
  return call("wishlist_list", {});
}

/** Adds a catalogue game (with its prices when they arrive quickly). */
export function add(gameId: GameId): Promise<WishlistItem> {
  return call("wishlist_add", { gameId });
}

export async function remove(gameId: GameId): Promise<void> {
  await call("wishlist_remove", { gameId });
}

export function update(gameId: GameId, targetPrice: number | null, notifyOnSale: boolean): Promise<WishlistItem> {
  return call("wishlist_update", { gameId, targetPrice, notifyOnSale });
}

/** Checks every price now. */
export function refresh(): Promise<WishlistItem[]> {
  return call("wishlist_refresh", {});
}

/** Prices of a catalogue game (IsThereAnyDeal); null = not found. */
export function gamePrice(gameId: GameId): Promise<GamePrice | null> {
  return call("game_price", { gameId });
}

export function subscribePriceAlert(handler: (alert: PriceAlert) => void): () => void {
  return subscribe("price-alert", handler);
}
