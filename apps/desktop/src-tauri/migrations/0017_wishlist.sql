-- Wishlist: catalogue games the user wants, with price alerts. Prices come
-- from IsThereAnyDeal through the Cove proxy; the last answer is kept here.
CREATE TABLE wishlist (
    game_id        TEXT PRIMARY KEY REFERENCES games (id) ON DELETE CASCADE,
    added_at       TEXT NOT NULL,
    -- Notify at or below this price (in the price's currency); NULL = off.
    target_price   REAL,
    notify_on_sale INTEGER NOT NULL DEFAULT 1,
    -- Last GamePrice as JSON.
    price          TEXT,
    -- Best price and discount of the last alert, so each deal is announced once.
    alerted_price  REAL,
    alerted_cut    INTEGER
);
