//! Games imported from store accounts and local store installs.
//! Each store game is identified by `(source_id, store_game_id)`.

use std::collections::HashSet;

use sqlx::SqlitePool;

use crate::db::slugify;
use crate::error::AppResult;
use crate::models::{now_iso, GameId, Platform, StoreSource};

/// Owned game (from the store account) and/or local install.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct StoreGameInput {
    pub store_game_id: String,
    pub title: String,
    /// Playtime according to the store (replaces the previous import).
    pub playtime_minutes: Option<i64>,
    pub last_played_at: Option<String>,
    pub image_url: Option<String>,
    pub install: Option<LocalInstall>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct LocalInstall {
    pub install_path: String,
    pub executable_path: Option<String>,
    pub size_bytes: Option<i64>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Upsert {
    Added(GameId),
    Updated(GameId),
}

async fn unique_id(
    pool: &SqlitePool,
    store: StoreSource,
    store_game_id: &str,
) -> AppResult<GameId> {
    let base = format!("{}-{}", store.as_str(), slugify(store_game_id));
    let mut candidate = base.clone();
    let mut n = 2;
    while sqlx::query_scalar::<_, i64>("SELECT 1 FROM games WHERE id = ?")
        .bind(&candidate)
        .fetch_optional(pool)
        .await?
        .is_some()
    {
        candidate = format!("{base}-{n}");
        n += 1;
    }
    Ok(candidate)
}

pub async fn find(
    pool: &SqlitePool,
    store: StoreSource,
    store_game_id: &str,
) -> AppResult<Option<GameId>> {
    Ok(sqlx::query_scalar(
        "SELECT game_id FROM library_entries WHERE source_id = ? AND store_game_id = ?",
    )
    .bind(store.as_str())
    .bind(store_game_id)
    .fetch_optional(pool)
    .await?)
}

/// Adds the game or refreshes playtime/install data of the existing entry.
/// User settings (favourite, launch options, catalogue link) are kept.
pub async fn upsert(
    pool: &SqlitePool,
    store: StoreSource,
    input: &StoreGameInput,
) -> AppResult<Upsert> {
    // Lookups first: the test pool has a single connection, held by the transaction.
    let existing = find(pool, store, &input.store_game_id).await?;
    let new_id = match existing {
        Some(_) => None,
        None => Some(unique_id(pool, store, &input.store_game_id).await?),
    };
    let mut tx = pool.begin().await?;
    if let Some(game_id) = existing {
        sqlx::query(
            "UPDATE library_entries SET \
               playtime_offset_minutes = COALESCE(?, playtime_offset_minutes), \
               store_last_played_at = COALESCE(?, store_last_played_at) \
             WHERE game_id = ?",
        )
        .bind(input.playtime_minutes)
        .bind(&input.last_played_at)
        .bind(&game_id)
        .execute(&mut *tx)
        .await?;
        if let Some(install) = &input.install {
            sqlx::query(
                "UPDATE library_entries SET installed = 1, install_path = ?, \
                   executable_path = COALESCE(?, executable_path), \
                   install_size_bytes = COALESCE(?, install_size_bytes) WHERE game_id = ?",
            )
            .bind(&install.install_path)
            .bind(&install.executable_path)
            .bind(install.size_bytes)
            .bind(&game_id)
            .execute(&mut *tx)
            .await?;
        }
        tx.commit().await?;
        return Ok(Upsert::Updated(game_id));
    }

    let id = new_id.unwrap_or_default();
    let now = now_iso();
    sqlx::query(
        "INSERT INTO games (id, title, summary, platforms, release_date, cover_url, created_at) \
         VALUES (?, ?, '', ?, '', ?, ?)",
    )
    .bind(&id)
    .bind(input.title.trim())
    .bind(serde_json::to_string(&[Platform::current()])?)
    .bind(&input.image_url)
    .bind(&now)
    .execute(&mut *tx)
    .await?;
    let install = input.install.as_ref();
    sqlx::query(
        "INSERT INTO library_entries (game_id, source_id, store_game_id, installed, install_path, executable_path, \
           install_size_bytes, playtime_offset_minutes, store_last_played_at, added_at) \
         VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
    )
    .bind(&id)
    .bind(store.as_str())
    .bind(&input.store_game_id)
    .bind(install.is_some())
    .bind(install.map(|i| &i.install_path))
    .bind(install.and_then(|i| i.executable_path.as_ref()))
    .bind(install.and_then(|i| i.size_bytes))
    .bind(input.playtime_minutes.unwrap_or(0))
    .bind(&input.last_played_at)
    .bind(&now)
    .execute(&mut *tx)
    .await?;
    tx.commit().await?;
    Ok(Upsert::Added(id))
}

/// `(store, store_game_id)` pairs already in the library.
pub async fn known(pool: &SqlitePool) -> AppResult<HashSet<(String, String)>> {
    let rows: Vec<(String, String)> = sqlx::query_as(
        "SELECT source_id, store_game_id FROM library_entries WHERE store_game_id IS NOT NULL",
    )
    .fetch_all(pool)
    .await?;
    Ok(rows.into_iter().collect())
}

/// Library games of a store as `(game_id, store_game_id)`; `played_only` skips never-played games.
pub async fn of_store(
    pool: &SqlitePool,
    store: StoreSource,
    played_only: bool,
) -> AppResult<Vec<(GameId, String)>> {
    let filter = if played_only {
        " AND (e.playtime_offset_minutes > 0 OR EXISTS (SELECT 1 FROM play_sessions s WHERE s.game_id = e.game_id))"
    } else {
        ""
    };
    Ok(sqlx::query_as(&format!(
        "SELECT e.game_id, e.store_game_id FROM library_entries e \
         WHERE e.source_id = ? AND e.store_game_id IS NOT NULL{filter} \
         ORDER BY e.playtime_offset_minutes DESC"
    ))
    .bind(store.as_str())
    .fetch_all(pool)
    .await?)
}

/// Logout without keeping games: removes the store's library entries
/// (game metadata and play sessions stay, like `library_remove`).
pub async fn remove_store(pool: &SqlitePool, store: StoreSource) -> AppResult<u64> {
    Ok(
        sqlx::query("DELETE FROM library_entries WHERE source_id = ?")
            .bind(store.as_str())
            .execute(pool)
            .await?
            .rows_affected(),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::{library, open_in_memory};

    fn portal(playtime: Option<i64>) -> StoreGameInput {
        StoreGameInput {
            store_game_id: "620".into(),
            title: "Portal 2".into(),
            playtime_minutes: playtime,
            last_played_at: Some("2026-10-01T10:00:00Z".into()),
            ..StoreGameInput::default()
        }
    }

    #[tokio::test]
    async fn adds_then_updates_without_duplicates() {
        let pool = open_in_memory().await.unwrap();
        let Upsert::Added(id) = upsert(&pool, StoreSource::Steam, &portal(Some(754)))
            .await
            .unwrap()
        else {
            panic!("expected a new game");
        };
        assert_eq!(id, "steam-620");
        let item = library::require(&pool, &id).await.unwrap();
        assert_eq!(item.entry.source, StoreSource::Steam);
        assert_eq!(item.entry.store_game_id.as_deref(), Some("620"));
        assert_eq!(item.entry.playtime_minutes, 754);
        assert_eq!(
            item.entry.last_played_at.as_deref(),
            Some("2026-10-01T10:00:00Z")
        );
        assert!(!item.entry.installed);

        // The user favourites it; a later import must keep that and only refresh store data.
        sqlx::query("UPDATE library_entries SET favorite = 1 WHERE game_id = ?")
            .bind(&id)
            .execute(&pool)
            .await
            .unwrap();
        let mut again = portal(Some(800));
        again.install = Some(LocalInstall {
            install_path: "/games/Portal 2".into(),
            executable_path: None,
            size_bytes: Some(12_000),
        });
        assert_eq!(
            upsert(&pool, StoreSource::Steam, &again).await.unwrap(),
            Upsert::Updated(id.clone())
        );
        let item = library::require(&pool, &id).await.unwrap();
        assert!(item.entry.favorite && item.entry.installed);
        assert_eq!(item.entry.playtime_minutes, 800);
        assert_eq!(item.entry.install_size_bytes, Some(12_000));

        // Missing playtime (local detection) keeps the imported value.
        upsert(&pool, StoreSource::Steam, &portal(None))
            .await
            .unwrap();
        assert_eq!(
            library::require(&pool, &id)
                .await
                .unwrap()
                .entry
                .playtime_minutes,
            800
        );
        assert_eq!(library::list(&pool).await.unwrap().len(), 1);
    }

    #[tokio::test]
    async fn same_id_in_different_stores_and_logout_cleanup() {
        let pool = open_in_memory().await.unwrap();
        upsert(&pool, StoreSource::Steam, &portal(Some(1)))
            .await
            .unwrap();
        upsert(&pool, StoreSource::Gog, &portal(None))
            .await
            .unwrap();
        assert!(known(&pool)
            .await
            .unwrap()
            .contains(&("gog".into(), "620".into())));
        assert_eq!(
            of_store(&pool, StoreSource::Steam, true)
                .await
                .unwrap()
                .len(),
            1
        );
        assert_eq!(of_store(&pool, StoreSource::Gog, true).await.unwrap(), []);

        assert_eq!(remove_store(&pool, StoreSource::Steam).await.unwrap(), 1);
        assert_eq!(library::list(&pool).await.unwrap().len(), 1);
        // Re-importing after a logout re-uses nothing stale.
        assert!(
            matches!(upsert(&pool, StoreSource::Steam, &portal(Some(1))).await.unwrap(), Upsert::Added(id) if id == "steam-620-2")
        );
    }
}
