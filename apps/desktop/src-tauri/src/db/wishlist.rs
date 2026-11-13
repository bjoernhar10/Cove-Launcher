//! The wishlist: catalogue games the user wants, with their last prices.

use sqlx::{FromRow, SqlitePool};

use crate::db::library::{upsert_game, GameRow, GAME_COLUMNS};
use crate::error::{AppError, AppResult};
use crate::models::{now_iso, Game, GamePrice, WishlistItem};

#[derive(FromRow)]
struct Row {
    #[sqlx(flatten)]
    game: GameRow,
    added_at: String,
    target_price: Option<f64>,
    notify_on_sale: bool,
    price: Option<String>,
}

impl Row {
    fn into_item(self) -> AppResult<WishlistItem> {
        Ok(WishlistItem {
            game: self.game.into_game()?,
            added_at: self.added_at,
            target_price: self.target_price,
            notify_on_sale: self.notify_on_sale,
            // An unreadable old answer is the same as none.
            price: self.price.and_then(|p| serde_json::from_str(&p).ok()),
        })
    }
}

/// Games now in the library (also as a catalogue match) leave the list by themselves.
const NOT_OWNED: &str = "NOT EXISTS (SELECT 1 FROM library_entries e \
     WHERE e.game_id = w.game_id OR e.catalog_id = w.game_id)";

/// The wishlist, newest first.
pub async fn list(pool: &SqlitePool) -> AppResult<Vec<WishlistItem>> {
    let rows: Vec<Row> = sqlx::query_as(&format!(
        "SELECT {GAME_COLUMNS}, w.added_at, w.target_price, w.notify_on_sale, w.price \
         FROM wishlist w JOIN games g ON g.id = w.game_id WHERE {NOT_OWNED} \
         ORDER BY w.added_at DESC, g.title"
    ))
    .fetch_all(pool)
    .await?;
    rows.into_iter().map(Row::into_item).collect()
}

pub async fn get(pool: &SqlitePool, game_id: &str) -> AppResult<Option<WishlistItem>> {
    let row: Option<Row> = sqlx::query_as(&format!(
        "SELECT {GAME_COLUMNS}, w.added_at, w.target_price, w.notify_on_sale, w.price \
         FROM wishlist w JOIN games g ON g.id = w.game_id WHERE w.game_id = ?"
    ))
    .bind(game_id)
    .fetch_optional(pool)
    .await?;
    row.map(Row::into_item).transpose()
}

fn not_on_list() -> AppError {
    AppError::InvalidInput(tr!(
        "Dieses Spiel ist nicht auf deiner Wunschliste.",
        "This game isn't on your wishlist."
    ))
}

/// Adds a catalogue game (kept when it is there already).
pub async fn add(pool: &SqlitePool, game: &Game) -> AppResult<WishlistItem> {
    if !game.id.starts_with("igdb-") {
        return Err(AppError::InvalidInput(tr!(
            "Nur Spiele aus dem Katalog können auf die Wunschliste.",
            "Only catalogue games can go on the wishlist."
        )));
    }
    let mut tx = pool.begin().await?;
    upsert_game(&mut tx, game).await?;
    sqlx::query(
        "INSERT INTO wishlist (game_id, added_at) VALUES (?, ?) ON CONFLICT (game_id) DO NOTHING",
    )
    .bind(&game.id)
    .bind(now_iso())
    .execute(&mut *tx)
    .await?;
    tx.commit().await?;
    get(pool, &game.id).await?.ok_or_else(not_on_list)
}

pub async fn remove(pool: &SqlitePool, game_id: &str) -> AppResult<()> {
    sqlx::query("DELETE FROM wishlist WHERE game_id = ?")
        .bind(game_id)
        .execute(pool)
        .await?;
    Ok(())
}

/// A new target price or sale setting; the next check may alert again.
pub async fn update(
    pool: &SqlitePool,
    game_id: &str,
    target_price: Option<f64>,
    notify_on_sale: bool,
) -> AppResult<WishlistItem> {
    if target_price.is_some_and(|p| !p.is_finite() || p < 0.0 || p > 100_000.0) {
        return Err(AppError::InvalidInput(tr!(
            "Der Wunschpreis muss zwischen 0 und 100000 liegen.",
            "The target price must be between 0 and 100000."
        )));
    }
    let result = sqlx::query(
        "UPDATE wishlist SET target_price = ?, notify_on_sale = ?, alerted_price = NULL, alerted_cut = NULL \
         WHERE game_id = ?",
    )
    .bind(target_price)
    .bind(notify_on_sale)
    .bind(game_id)
    .execute(pool)
    .await?;
    if result.rows_affected() == 0 {
        return Err(not_on_list());
    }
    get(pool, game_id).await?.ok_or_else(not_on_list)
}

/// What the alert check needs about one entry.
#[derive(Debug, Clone, FromRow)]
pub struct AlertState {
    pub game_id: String,
    pub title: String,
    pub target_price: Option<f64>,
    pub notify_on_sale: bool,
    pub alerted_price: Option<f64>,
    pub alerted_cut: Option<i64>,
}

pub async fn alert_states(pool: &SqlitePool) -> AppResult<Vec<AlertState>> {
    Ok(sqlx::query_as(&format!(
        "SELECT w.game_id, g.title, w.target_price, w.notify_on_sale, w.alerted_price, w.alerted_cut \
         FROM wishlist w JOIN games g ON g.id = w.game_id WHERE {NOT_OWNED}"
    ))
    .fetch_all(pool)
    .await?)
}

pub async fn set_price(
    pool: &SqlitePool,
    game_id: &str,
    price: Option<&GamePrice>,
) -> AppResult<()> {
    sqlx::query("UPDATE wishlist SET price = ? WHERE game_id = ?")
        .bind(price.map(serde_json::to_string).transpose()?)
        .bind(game_id)
        .execute(pool)
        .await?;
    Ok(())
}

/// Remembers the deal an alert was sent for (None = the deal ended: the next one alerts again).
pub async fn set_alerted(
    pool: &SqlitePool,
    game_id: &str,
    alerted: Option<(f64, i64)>,
) -> AppResult<()> {
    sqlx::query("UPDATE wishlist SET alerted_price = ?, alerted_cut = ? WHERE game_id = ?")
        .bind(alerted.map(|a| a.0))
        .bind(alerted.map(|a| a.1))
        .bind(game_id)
        .execute(pool)
        .await?;
    Ok(())
}
