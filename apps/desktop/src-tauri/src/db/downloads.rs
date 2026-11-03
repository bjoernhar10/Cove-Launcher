//! Download queue persistence. The actual transfer is done by the store
//! connectors later; this module only manages queue state.

use sqlx::{FromRow, SqlitePool};

use crate::db::library::{GameRow, GAME_COLUMNS};
use crate::error::{AppError, AppResult};
use crate::models::{now_iso, Download, DownloadItem, DownloadStatus, StoreSource};

#[derive(Debug, FromRow)]
struct DownloadRow {
    download_id: String,
    source_id: String,
    status: String,
    total_bytes: i64,
    downloaded_bytes: i64,
    queue_position: Option<i64>,
    queued_at: String,
    finished_at: Option<String>,
    error: Option<String>,
    #[sqlx(flatten)]
    game: GameRow,
}

impl DownloadRow {
    fn into_item(self) -> AppResult<DownloadItem> {
        let game = self.game.into_game()?;
        Ok(DownloadItem {
            download: Download {
                id: self.download_id,
                game_id: game.id.clone(),
                source: StoreSource::parse(&self.source_id).unwrap_or(StoreSource::Manual),
                status: DownloadStatus::parse(&self.status),
                total_bytes: self.total_bytes,
                downloaded_bytes: self.downloaded_bytes,
                bytes_per_second: 0,
                queue_position: self.queue_position,
                queued_at: self.queued_at,
                finished_at: self.finished_at,
                error: self.error,
            },
            game,
        })
    }
}

pub async fn list(pool: &SqlitePool) -> AppResult<Vec<DownloadItem>> {
    let rows: Vec<DownloadRow> = sqlx::query_as(&format!(
        "SELECT d.id AS download_id, d.source_id, d.status, d.total_bytes, d.downloaded_bytes, d.queue_position, \
           d.queued_at, d.finished_at, d.error, {GAME_COLUMNS} \
         FROM downloads d JOIN games g ON g.id = d.game_id ORDER BY d.queue_position IS NULL, d.queue_position, d.finished_at DESC"
    ))
    .fetch_all(pool)
    .await?;
    rows.into_iter().map(DownloadRow::into_item).collect()
}

async fn renumber(pool: &SqlitePool) -> AppResult<()> {
    let ids: Vec<String> = sqlx::query_scalar(
        "SELECT id FROM downloads WHERE queue_position IS NOT NULL ORDER BY queue_position",
    )
    .fetch_all(pool)
    .await?;
    for (position, id) in ids.iter().enumerate() {
        sqlx::query("UPDATE downloads SET queue_position = ? WHERE id = ?")
            .bind(i64::try_from(position).unwrap_or(i64::MAX))
            .bind(id)
            .execute(pool)
            .await?;
    }
    Ok(())
}

/// Queues a library game for installation (no-op if it is already queued or running).
pub async fn enqueue(pool: &SqlitePool, game_id: &str) -> AppResult<()> {
    let source: Option<String> =
        sqlx::query_scalar("SELECT source_id FROM library_entries WHERE game_id = ?")
            .bind(game_id)
            .fetch_optional(pool)
            .await?;
    let source = source.ok_or_else(|| AppError::NotInLibrary(game_id.to_owned()))?;
    if source == "manual" {
        return Err(AppError::InvalidInput(
            "Manuell hinzugefügte Spiele können nicht heruntergeladen werden. Lege stattdessen den Pfad fest.".into(),
        ));
    }
    let active: Option<i64> = sqlx::query_scalar(
        "SELECT 1 FROM downloads WHERE game_id = ? AND queue_position IS NOT NULL",
    )
    .bind(game_id)
    .fetch_optional(pool)
    .await?;
    if active.is_some() {
        return Ok(());
    }
    let now = now_iso();
    sqlx::query(
        "INSERT INTO downloads (id, game_id, source_id, status, queue_position, queued_at) \
         VALUES (?, ?, ?, 'queued', (SELECT COALESCE(MAX(queue_position), -1) + 1 FROM downloads), ?)",
    )
    .bind(format!("dl-{game_id}-{}", chrono::Utc::now().timestamp_millis()))
    .bind(game_id)
    .bind(source)
    .bind(&now)
    .execute(pool)
    .await?;
    Ok(())
}

pub async fn set_status(pool: &SqlitePool, id: &str, status: DownloadStatus) -> AppResult<()> {
    let finished = matches!(
        status,
        DownloadStatus::Completed | DownloadStatus::Failed | DownloadStatus::Cancelled
    );
    let result = if finished {
        sqlx::query(
            "UPDATE downloads SET status = ?, queue_position = NULL, finished_at = ? WHERE id = ?",
        )
        .bind(status.as_str())
        .bind(now_iso())
        .bind(id)
        .execute(pool)
        .await?
    } else {
        sqlx::query(
            "UPDATE downloads SET status = ?, error = NULL, finished_at = NULL, \
               queue_position = COALESCE(queue_position, (SELECT COALESCE(MAX(queue_position), -1) + 1 FROM downloads)) \
             WHERE id = ?",
        )
        .bind(status.as_str())
        .bind(id)
        .execute(pool)
        .await?
    };
    if result.rows_affected() == 0 {
        return Err(AppError::InvalidInput(format!(
            "Download nicht gefunden: {id}"
        )));
    }
    renumber(pool).await
}

/// `ordered_ids`: queued downloads in their new order; they follow the active ones.
pub async fn reorder(pool: &SqlitePool, ordered_ids: &[String]) -> AppResult<()> {
    for (i, id) in ordered_ids.iter().enumerate() {
        sqlx::query("UPDATE downloads SET queue_position = ? WHERE id = ? AND status = 'queued'")
            .bind(1000 + i64::try_from(i).unwrap_or(0))
            .bind(id)
            .execute(pool)
            .await?;
    }
    renumber(pool).await
}

pub async fn clear_history(pool: &SqlitePool) -> AppResult<()> {
    sqlx::query("DELETE FROM downloads WHERE queue_position IS NULL")
        .execute(pool)
        .await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::library::{insert_store_game, tests::sample_game};
    use crate::db::open_in_memory;

    #[tokio::test]
    async fn queue_lifecycle() {
        let pool = open_in_memory().await.unwrap();
        for id in ["a", "b", "c"] {
            insert_store_game(
                &pool,
                &sample_game(id, id),
                StoreSource::Steam,
                Some("1"),
                None,
            )
            .await
            .unwrap();
            enqueue(&pool, id).await.unwrap();
        }
        enqueue(&pool, "a").await.unwrap(); // idempotent
        let ids =
            |items: &[DownloadItem]| items.iter().map(|i| i.game.id.clone()).collect::<Vec<_>>();
        assert_eq!(ids(&list(&pool).await.unwrap()), ["a", "b", "c"]);

        let all = list(&pool).await.unwrap();
        reorder(
            &pool,
            &[
                all[2].download.id.clone(),
                all[1].download.id.clone(),
                all[0].download.id.clone(),
            ],
        )
        .await
        .unwrap();
        assert_eq!(ids(&list(&pool).await.unwrap()), ["c", "b", "a"]);

        set_status(&pool, &all[1].download.id, DownloadStatus::Cancelled)
            .await
            .unwrap();
        let after = list(&pool).await.unwrap();
        assert_eq!(
            after
                .iter()
                .filter(|i| i.download.queue_position.is_some())
                .count(),
            2
        );
        clear_history(&pool).await.unwrap();
        assert_eq!(list(&pool).await.unwrap().len(), 2);
    }

    #[tokio::test]
    async fn manual_games_cannot_be_downloaded() {
        let pool = open_in_memory().await.unwrap();
        crate::db::library::add_from_catalog(&pool, &sample_game("m", "M"))
            .await
            .unwrap();
        assert!(matches!(
            enqueue(&pool, "m").await,
            Err(AppError::InvalidInput(_))
        ));
    }
}
