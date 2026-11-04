//! Play sessions and playtime statistics.

use chrono::{Duration, NaiveDate};
use sqlx::SqlitePool;

use crate::error::AppResult;
use crate::models::{PlaytimeDay, PlaytimeStats};

/// Opens a session; returns its id.
pub async fn start(pool: &SqlitePool, game_id: &str, started_at: &str) -> AppResult<i64> {
    let id = sqlx::query("INSERT INTO play_sessions (game_id, started_at) VALUES (?, ?)")
        .bind(game_id)
        .bind(started_at)
        .execute(pool)
        .await?
        .last_insert_rowid();
    Ok(id)
}

/// Heartbeat / finish: stores the current end time and duration.
pub async fn update(
    pool: &SqlitePool,
    session_id: i64,
    ended_at: &str,
    duration_seconds: i64,
) -> AppResult<()> {
    sqlx::query("UPDATE play_sessions SET ended_at = ?, duration_seconds = ? WHERE id = ?")
        .bind(ended_at)
        .bind(duration_seconds.max(0))
        .bind(session_id)
        .execute(pool)
        .await?;
    Ok(())
}

/// Number of real (non-empty) sessions per game.
pub async fn counts(pool: &SqlitePool) -> AppResult<std::collections::HashMap<String, i64>> {
    let rows: Vec<(String, i64)> = sqlx::query_as(
        "SELECT game_id, COUNT(*) FROM play_sessions WHERE duration_seconds > 0 GROUP BY game_id",
    )
    .fetch_all(pool)
    .await?;
    Ok(rows.into_iter().collect())
}

/// After a crash, sessions without end keep their last heartbeat duration.
pub async fn close_dangling(pool: &SqlitePool) -> AppResult<u64> {
    let result =
        sqlx::query("UPDATE play_sessions SET ended_at = started_at WHERE ended_at IS NULL")
            .execute(pool)
            .await?;
    Ok(result.rows_affected())
}

/// Totals plus one bucket per day for the 14 days ending at `today` (UTC dates).
pub async fn stats(pool: &SqlitePool, game_id: &str, today: NaiveDate) -> AppResult<PlaytimeStats> {
    let first_day = today - Duration::days(13);
    let (total_seconds, session_count): (i64, i64) = sqlx::query_as(
        "SELECT COALESCE(SUM(duration_seconds), 0), COUNT(*) FROM play_sessions WHERE game_id = ? AND duration_seconds > 0",
    )
    .bind(game_id)
    .fetch_one(pool)
    .await?;
    let offset: i64 =
        sqlx::query_scalar("SELECT COALESCE((SELECT playtime_offset_minutes FROM library_entries WHERE game_id = ?), 0)")
            .bind(game_id)
            .fetch_one(pool)
            .await?;
    let per_day: Vec<(String, i64)> = sqlx::query_as(
        "SELECT substr(started_at, 1, 10) AS day, SUM(duration_seconds) FROM play_sessions \
         WHERE game_id = ? AND substr(started_at, 1, 10) >= ? GROUP BY day",
    )
    .bind(game_id)
    .bind(first_day.to_string())
    .fetch_all(pool)
    .await?;

    let days: Vec<PlaytimeDay> = (0..14)
        .map(|i| {
            let date = (first_day + Duration::days(i)).to_string();
            let seconds = per_day
                .iter()
                .find(|(d, _)| *d == date)
                .map_or(0, |(_, s)| *s);
            PlaytimeDay {
                date,
                minutes: seconds / 60,
            }
        })
        .collect();
    let last14 = days.iter().map(|d| d.minutes).sum();

    Ok(PlaytimeStats {
        game_id: game_id.to_owned(),
        total_minutes: offset + total_seconds / 60,
        last14_days_minutes: last14,
        session_count,
        days,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::library::add_manual;
    use crate::db::open_in_memory;
    use crate::models::ManualGameInput;

    #[tokio::test]
    async fn stats_buckets_by_day() {
        let pool = open_in_memory().await.unwrap();
        let input = ManualGameInput {
            title: "G".into(),
            executable_path: "/g".into(),
            size_bytes: None,
        };
        let id = add_manual(&pool, &input).await.unwrap().game.id;

        for (start, end, secs) in [
            ("2026-11-03T08:00:00Z", "2026-11-03T09:00:00Z", 3600),
            ("2026-11-03T20:00:00Z", "2026-11-03T20:30:00Z", 1800),
            ("2026-11-01T10:00:00Z", "2026-11-01T10:20:00Z", 1200),
            ("2026-09-01T10:00:00Z", "2026-09-01T12:00:00Z", 7200), // outside the window
        ] {
            let s = start_and_finish(&pool, &id, start, end, secs).await;
            assert!(s > 0);
        }

        let today = NaiveDate::from_ymd_opt(2026, 11, 3).unwrap();
        let stats = stats(&pool, &id, today).await.unwrap();
        assert_eq!(stats.days.len(), 14);
        assert_eq!(stats.days.first().unwrap().date, "2026-10-21");
        assert_eq!(stats.days.last().unwrap().minutes, 90);
        assert_eq!(stats.days[11].minutes, 20);
        assert_eq!(stats.last14_days_minutes, 110);
        assert_eq!(stats.total_minutes, 230);
        assert_eq!(stats.session_count, 4);
    }

    #[tokio::test]
    async fn dangling_sessions_are_closed() {
        let pool = open_in_memory().await.unwrap();
        let input = ManualGameInput {
            title: "G".into(),
            executable_path: "/g".into(),
            size_bytes: None,
        };
        let id = add_manual(&pool, &input).await.unwrap().game.id;
        start(&pool, &id, "2026-11-03T08:00:00Z").await.unwrap();
        assert_eq!(close_dangling(&pool).await.unwrap(), 1);
        assert_eq!(close_dangling(&pool).await.unwrap(), 0);
    }

    async fn start_and_finish(
        pool: &SqlitePool,
        id: &str,
        start_at: &str,
        end: &str,
        secs: i64,
    ) -> i64 {
        let session = start(pool, id, start_at).await.unwrap();
        update(pool, session, end, secs).await.unwrap();
        session
    }
}
