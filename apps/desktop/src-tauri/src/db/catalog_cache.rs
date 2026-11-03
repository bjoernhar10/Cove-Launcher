//! Cache of catalogue provider responses (JSON). Freshness is decided by the
//! catalogue service; stale rows are still used when the provider is offline.

use chrono::{DateTime, Utc};
use sqlx::SqlitePool;

use crate::error::AppResult;
use crate::models::now_iso;

pub struct CachedValue {
    pub value: String,
    pub fetched_at: DateTime<Utc>,
}

pub async fn get(pool: &SqlitePool, key: &str) -> AppResult<Option<CachedValue>> {
    let row: Option<(String, String)> =
        sqlx::query_as("SELECT value, fetched_at FROM catalog_cache WHERE key = ?")
            .bind(key)
            .fetch_optional(pool)
            .await?;
    Ok(row.and_then(|(value, fetched_at)| {
        let fetched_at = DateTime::parse_from_rfc3339(&fetched_at)
            .ok()?
            .with_timezone(&Utc);
        Some(CachedValue { value, fetched_at })
    }))
}

pub async fn put(pool: &SqlitePool, provider: &str, key: &str, value: &str) -> AppResult<()> {
    sqlx::query(
        "INSERT INTO catalog_cache (key, provider, value, fetched_at) VALUES (?, ?, ?, ?) \
         ON CONFLICT (key) DO UPDATE SET value = excluded.value, fetched_at = excluded.fetched_at",
    )
    .bind(key)
    .bind(provider)
    .bind(value)
    .bind(now_iso())
    .execute(pool)
    .await?;
    Ok(())
}

/// Removes everything older than `max_age_days` (housekeeping on startup).
pub async fn prune(pool: &SqlitePool, max_age_days: i64) -> AppResult<u64> {
    let cutoff = (Utc::now() - chrono::Duration::days(max_age_days))
        .to_rfc3339_opts(chrono::SecondsFormat::Secs, true);
    let result = sqlx::query("DELETE FROM catalog_cache WHERE fetched_at < ?")
        .bind(cutoff)
        .execute(pool)
        .await?;
    Ok(result.rows_affected())
}

#[cfg(test)]
pub async fn set_fetched_at(pool: &SqlitePool, key: &str, fetched_at: DateTime<Utc>) {
    sqlx::query("UPDATE catalog_cache SET fetched_at = ? WHERE key = ?")
        .bind(fetched_at.to_rfc3339_opts(chrono::SecondsFormat::Secs, true))
        .bind(key)
        .execute(pool)
        .await
        .unwrap();
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::open_in_memory;

    #[tokio::test]
    async fn put_get_and_prune() {
        let pool = open_in_memory().await.unwrap();
        assert!(get(&pool, "k").await.unwrap().is_none());
        put(&pool, "igdb", "k", "[1]").await.unwrap();
        put(&pool, "igdb", "k", "[2]").await.unwrap();
        let cached = get(&pool, "k").await.unwrap().unwrap();
        assert_eq!(cached.value, "[2]");
        assert!((Utc::now() - cached.fetched_at).num_seconds() < 5);

        set_fetched_at(&pool, "k", Utc::now() - chrono::Duration::days(40)).await;
        assert_eq!(prune(&pool, 30).await.unwrap(), 1);
    }
}
