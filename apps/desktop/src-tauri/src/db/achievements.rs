//! Store achievements (definitions per game + user unlocks), filled by
//! `achievements_sync` from the C# connectors.

use sqlx::{FromRow, SqlitePool};

use crate::db::library::{GameRow, GAME_COLUMNS};
use crate::error::AppResult;
use crate::models::{Achievement, AchievementUnlock, ImageRef};

#[derive(Debug, FromRow)]
struct AchievementRow {
    achievement_id: String,
    achievement_game_id: String,
    achievement_title: String,
    achievement_description: String,
    hidden: bool,
    rarity_percent: Option<f64>,
    icon_url: Option<String>,
    icon_locked_url: Option<String>,
    unlocked_at: Option<String>,
}

impl From<AchievementRow> for Achievement {
    fn from(row: AchievementRow) -> Self {
        let icon = if row.unlocked_at.is_some() {
            row.icon_url.or(row.icon_locked_url)
        } else {
            row.icon_locked_url.or(row.icon_url)
        };
        Self {
            icon: ImageRef {
                url: icon,
                alt: row.achievement_title.clone(),
            },
            id: row.achievement_id,
            game_id: row.achievement_game_id,
            title: row.achievement_title,
            description: row.achievement_description,
            unlocked_at: row.unlocked_at,
            rarity_percent: row.rarity_percent,
            hidden: row.hidden,
        }
    }
}

const ACHIEVEMENT_COLUMNS: &str = "a.id AS achievement_id, a.game_id AS achievement_game_id, \
     a.title AS achievement_title, a.description AS achievement_description, a.hidden, a.rarity_percent, a.icon_url, a.icon_locked_url, u.unlocked_at";

/// All achievements of a game: unlocked first (newest first), then locked in their defined order.
pub async fn list(pool: &SqlitePool, game_id: &str) -> AppResult<Vec<Achievement>> {
    let rows: Vec<AchievementRow> = sqlx::query_as(&format!(
        "SELECT {ACHIEVEMENT_COLUMNS} FROM achievements a \
         LEFT JOIN user_achievements u ON u.game_id = a.game_id AND u.achievement_id = a.id \
         WHERE a.game_id = ? ORDER BY u.unlocked_at IS NULL, u.unlocked_at DESC, a.sort_order"
    ))
    .bind(game_id)
    .fetch_all(pool)
    .await?;
    Ok(rows.into_iter().map(Achievement::from).collect())
}

#[derive(Debug, FromRow)]
struct UnlockRow {
    #[sqlx(flatten)]
    achievement: AchievementRow,
    #[sqlx(flatten)]
    game: GameRow,
}

/// Most recent unlocks across all games in the library.
pub async fn recent(pool: &SqlitePool, limit: i64) -> AppResult<Vec<AchievementUnlock>> {
    let rows: Vec<UnlockRow> = sqlx::query_as(&format!(
        "SELECT {ACHIEVEMENT_COLUMNS}, {GAME_COLUMNS} FROM user_achievements u \
         JOIN achievements a ON a.game_id = u.game_id AND a.id = u.achievement_id \
         JOIN games g ON g.id = u.game_id \
         JOIN library_entries e ON e.game_id = u.game_id \
         ORDER BY u.unlocked_at DESC LIMIT ?"
    ))
    .bind(limit)
    .fetch_all(pool)
    .await?;
    rows.into_iter()
        .map(|row| {
            Ok(AchievementUnlock {
                achievement: row.achievement.into(),
                game: row.game.into_game()?,
            })
        })
        .collect()
}

/// One achievement as the store reports it.
#[derive(Debug, Clone, PartialEq)]
pub struct SyncedAchievement {
    pub id: String,
    pub title: String,
    pub description: String,
    pub icon_url: Option<String>,
    pub icon_locked_url: Option<String>,
    pub hidden: bool,
    pub unlocked_at: Option<String>,
    pub rarity_percent: Option<f64>,
}

/// Stores definitions and unlocks of one game; returns how many unlocks are new.
/// Unlocks are never removed (stores do not take achievements back).
pub async fn store_synced(
    pool: &SqlitePool,
    game_id: &str,
    items: &[SyncedAchievement],
) -> AppResult<i64> {
    let mut tx = pool.begin().await?;
    let mut new_unlocks = 0;
    for (order, item) in items.iter().enumerate() {
        sqlx::query(
            "INSERT INTO achievements (game_id, id, title, description, hidden, rarity_percent, sort_order, icon_url, icon_locked_url) \
             VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?) \
             ON CONFLICT (game_id, id) DO UPDATE SET title = excluded.title, description = excluded.description, \
               hidden = excluded.hidden, rarity_percent = excluded.rarity_percent, sort_order = excluded.sort_order, \
               icon_url = excluded.icon_url, icon_locked_url = excluded.icon_locked_url",
        )
        .bind(game_id)
        .bind(&item.id)
        .bind(&item.title)
        .bind(&item.description)
        .bind(item.hidden)
        .bind(item.rarity_percent)
        .bind(i64::try_from(order).unwrap_or(i64::MAX))
        .bind(&item.icon_url)
        .bind(&item.icon_locked_url)
        .execute(&mut *tx)
        .await?;
        if let Some(unlocked_at) = &item.unlocked_at {
            let inserted = sqlx::query("INSERT OR IGNORE INTO user_achievements VALUES (?, ?, ?)")
                .bind(game_id)
                .bind(&item.id)
                .bind(unlocked_at)
                .execute(&mut *tx)
                .await?;
            new_unlocks += i64::try_from(inserted.rows_affected()).unwrap_or(0);
        }
    }
    tx.commit().await?;
    Ok(new_unlocks)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::library::{add_from_catalog, require, tests::sample_game};
    use crate::db::open_in_memory;

    #[tokio::test]
    async fn lists_unlocked_first_and_counts_progress() {
        let pool = open_in_memory().await.unwrap();
        add_from_catalog(&pool, &sample_game("g", "Game"))
            .await
            .unwrap();
        for (i, id) in ["a", "b", "c"].iter().enumerate() {
            sqlx::query(
                "INSERT INTO achievements (game_id, id, title, sort_order) VALUES ('g', ?, ?, ?)",
            )
            .bind(id)
            .bind(id.to_uppercase())
            .bind(i64::try_from(i).unwrap())
            .execute(&pool)
            .await
            .unwrap();
        }
        sqlx::query("INSERT INTO user_achievements VALUES ('g', 'c', '2026-11-02T10:00:00Z')")
            .execute(&pool)
            .await
            .unwrap();

        let list = list(&pool, "g").await.unwrap();
        let ids: Vec<&str> = list.iter().map(|a| a.id.as_str()).collect();
        assert_eq!(ids, ["c", "a", "b"]);
        assert!(list[0].unlocked_at.is_some());

        let progress = require(&pool, "g").await.unwrap().entry.achievements;
        assert_eq!((progress.unlocked, progress.total), (1, 3));

        let recent = recent(&pool, 5).await.unwrap();
        assert_eq!(recent.len(), 1);
        assert_eq!(recent[0].game.title, "Game");
    }

    #[tokio::test]
    async fn stores_synced_achievements_idempotently() {
        let pool = open_in_memory().await.unwrap();
        add_from_catalog(&pool, &sample_game("g", "Game"))
            .await
            .unwrap();
        let mut items = vec![
            SyncedAchievement {
                id: "ACH_A".into(),
                title: "Erster Schritt".into(),
                description: "Fang an".into(),
                icon_url: Some("https://cdn.akamai.steamstatic.com/a.jpg".into()),
                icon_locked_url: Some("https://cdn.akamai.steamstatic.com/a_g.jpg".into()),
                hidden: false,
                unlocked_at: Some("2026-09-01T10:00:00Z".into()),
                rarity_percent: Some(81.5),
            },
            SyncedAchievement {
                id: "ACH_B".into(),
                title: "Geheim".into(),
                description: String::new(),
                icon_url: Some("https://cdn.akamai.steamstatic.com/b.jpg".into()),
                icon_locked_url: Some("https://cdn.akamai.steamstatic.com/b_g.jpg".into()),
                hidden: true,
                unlocked_at: None,
                rarity_percent: Some(3.2),
            },
        ];
        assert_eq!(store_synced(&pool, "g", &items).await.unwrap(), 1);
        assert_eq!(
            store_synced(&pool, "g", &items).await.unwrap(),
            0,
            "no double counting"
        );
        items[1].unlocked_at = Some("2026-11-01T22:00:00Z".into());
        assert_eq!(store_synced(&pool, "g", &items).await.unwrap(), 1);

        let list = list(&pool, "g").await.unwrap();
        assert_eq!(list[0].id, "ACH_B");
        assert_eq!(list[1].rarity_percent, Some(81.5));
        // Unlocked → colour icon (raw URL kept in Rust; proxied only on the wire).
        assert_eq!(
            list[1].icon.url.as_deref(),
            Some("https://cdn.akamai.steamstatic.com/a.jpg")
        );
    }
}
