//! Store sources (Steam, GOG, Epic, manual) and their connection state.

use sqlx::{FromRow, SqlitePool};

use crate::error::AppResult;
use crate::models::{StoreAccount, StoreSource};

#[derive(Debug, FromRow)]
struct SourceRow {
    id: String,
    connected: bool,
    account_name: Option<String>,
    last_synced_at: Option<String>,
    game_count: i64,
}

/// Connectable stores (manual excluded) with the number of library games from each.
pub async fn list(pool: &SqlitePool) -> AppResult<Vec<StoreAccount>> {
    let rows: Vec<SourceRow> = sqlx::query_as(
        "SELECT s.id, s.connected, s.account_name, s.last_synced_at, \
           (SELECT COUNT(*) FROM library_entries e WHERE e.source_id = s.id) AS game_count \
         FROM sources s WHERE s.id <> 'manual' \
         ORDER BY CASE s.id WHEN 'steam' THEN 0 WHEN 'gog' THEN 1 ELSE 2 END",
    )
    .fetch_all(pool)
    .await?;
    Ok(rows
        .into_iter()
        .filter_map(|r| {
            Some(StoreAccount {
                store: StoreSource::parse(&r.id)?,
                connected: r.connected,
                account_name: r.account_name,
                game_count: r.game_count,
                last_synced_at: r.last_synced_at,
            })
        })
        .collect())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::open_in_memory;

    #[tokio::test]
    async fn lists_three_stores_disconnected() {
        let pool = open_in_memory().await.unwrap();
        let accounts = list(&pool).await.unwrap();
        let stores: Vec<StoreSource> = accounts.iter().map(|a| a.store).collect();
        assert_eq!(
            stores,
            [StoreSource::Steam, StoreSource::Gog, StoreSource::Epic]
        );
        assert!(accounts.iter().all(|a| !a.connected && a.game_count == 0));
    }
}
