//! Achievements (definitions per game + user unlocks). Filled by the C#
//! connectors later; Cove itself only reads them.

use sqlx::{FromRow, SqlitePool};

use crate::db::library::{GameRow, GAME_COLUMNS};
use crate::error::AppResult;
use crate::models::{Achievement, AchievementUnlock};

#[derive(Debug, FromRow)]
struct AchievementRow {
    achievement_id: String,
    achievement_game_id: String,
    achievement_title: String,
    achievement_description: String,
    hidden: bool,
    rarity_percent: Option<f64>,
    unlocked_at: Option<String>,
}

impl From<AchievementRow> for Achievement {
    fn from(row: AchievementRow) -> Self {
        Self {
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
     a.title AS achievement_title, a.description AS achievement_description, a.hidden, a.rarity_percent, u.unlocked_at";

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
}
