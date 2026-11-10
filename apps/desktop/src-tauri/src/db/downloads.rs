//! Download queue persistence. The engine (`crate::downloads`) does the
//! transfers; this module only stores state, so downloads survive restarts.

use sqlx::{FromRow, SqlitePool};

use crate::db::library::{GameRow, GAME_COLUMNS};
use crate::db::slugify;
use crate::error::{AppError, AppResult};
use crate::models::{
    now_iso, CompatibilityTool, Download, DownloadItem, DownloadOrigin, DownloadStatus, Platform,
    StoreSource,
};

#[derive(Debug, FromRow)]
struct DownloadRow {
    download_id: String,
    source_id: String,
    origin: String,
    status: String,
    total_bytes: i64,
    downloaded_bytes: i64,
    queue_position: Option<i64>,
    queued_at: String,
    finished_at: Option<String>,
    error: Option<String>,
    file_name: Option<String>,
    checksum_verified: bool,
    install_path: Option<String>,
    executable_path: Option<String>,
    extra: Option<String>,
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
                origin: DownloadOrigin::parse(&self.origin),
                file_name: self.file_name,
                checksum_verified: self.checksum_verified,
                install_path: self.install_path,
                executable_path: self.executable_path,
                extra: self.extra,
            },
            game,
        })
    }
}

const SELECT: &str = "SELECT d.id AS download_id, d.source_id, d.origin, d.status, d.total_bytes, d.downloaded_bytes, \
       d.queue_position, d.queued_at, d.finished_at, d.error, d.file_name, d.checksum_verified, d.install_path, \
       d.executable_path, d.extra";

pub async fn list(pool: &SqlitePool) -> AppResult<Vec<DownloadItem>> {
    let rows: Vec<DownloadRow> = sqlx::query_as(&format!(
        "{SELECT}, {GAME_COLUMNS} FROM downloads d JOIN games g ON g.id = d.game_id \
         ORDER BY d.queue_position IS NULL, d.queue_position, d.finished_at DESC"
    ))
    .fetch_all(pool)
    .await?;
    rows.into_iter().map(DownloadRow::into_item).collect()
}

pub async fn get(pool: &SqlitePool, id: &str) -> AppResult<DownloadItem> {
    let row: Option<DownloadRow> = sqlx::query_as(&format!(
        "{SELECT}, {GAME_COLUMNS} FROM downloads d JOIN games g ON g.id = d.game_id WHERE d.id = ?"
    ))
    .bind(id)
    .fetch_optional(pool)
    .await?;
    row.ok_or_else(|| {
        AppError::InvalidInput(tr!(
            "Download nicht gefunden: {id}",
            "Download not found: {id}"
        ))
    })?
    .into_item()
}

/// Everything the engine needs to (re)start a transfer.
#[derive(Debug, Clone, FromRow, PartialEq, Eq)]
pub struct Record {
    pub id: String,
    pub game_id: String,
    pub origin: String,
    pub status: String,
    pub total_bytes: i64,
    pub downloaded_bytes: i64,
    pub url: Option<String>,
    pub file_name: Option<String>,
    pub checksum_sha256: Option<String>,
    pub etag: Option<String>,
    pub chunk_size: i64,
    pub chunks_done: String,
    pub target_dir: Option<String>,
    pub itch_game_id: Option<i64>,
    pub itch_upload_id: Option<i64>,
    pub itch_download_key_id: Option<i64>,
    pub title: String,
    /// Store installs: the game folder, fixed when queued (also for updates).
    pub install_path: Option<String>,
    /// itch.io extras: the file's name; it goes into the game's "Extras" folder.
    pub extra: Option<String>,
}

pub async fn record(pool: &SqlitePool, id: &str) -> AppResult<Record> {
    sqlx::query_as(
        "SELECT d.id, d.game_id, d.origin, d.status, d.total_bytes, d.downloaded_bytes, d.url, d.file_name, \
           d.checksum_sha256, d.etag, d.chunk_size, d.chunks_done, d.target_dir, d.itch_game_id, d.itch_upload_id, \
           d.itch_download_key_id, g.title, d.install_path, d.extra \
         FROM downloads d JOIN games g ON g.id = d.game_id WHERE d.id = ?",
    )
    .bind(id)
    .fetch_optional(pool)
    .await?
    .ok_or_else(|| AppError::InvalidInput(tr!("Download nicht gefunden: {id}", "Download not found: {id}")))
}

/// Queued transfers Cove runs itself (direct, itch, store installs) in queue order.
pub async fn queued(pool: &SqlitePool) -> AppResult<Vec<String>> {
    Ok(sqlx::query_scalar(
        "SELECT id FROM downloads WHERE status = 'queued' AND target_dir IS NOT NULL \
         ORDER BY queue_position",
    )
    .fetch_all(pool)
    .await?)
}

/// `(download id, store, store game id, game id)` of installs handed to a store client.
pub async fn handed_off(pool: &SqlitePool) -> AppResult<Vec<(String, String, String, String)>> {
    Ok(sqlx::query_as(
        "SELECT d.id, d.origin, e.store_game_id, d.game_id FROM downloads d \
         JOIN library_entries e ON e.game_id = d.game_id \
         WHERE d.status = 'handedOff' AND e.store_game_id IS NOT NULL",
    )
    .fetch_all(pool)
    .await?)
}

/// After a restart, transfers that were running continue from where they stopped.
pub async fn requeue_interrupted(pool: &SqlitePool) -> AppResult<u64> {
    Ok(sqlx::query(
        "UPDATE downloads SET status = 'queued' WHERE status IN ('downloading', 'verifying', 'extracting')",
    )
    .execute(pool)
    .await?
    .rows_affected())
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

fn new_id(base: &str) -> String {
    format!("dl-{base}-{}", chrono::Utc::now().timestamp_micros())
}

/// Store games: the installation is handed over to the store's client.
/// Returns the download id (an existing active one is reused).
pub async fn enqueue_handoff(
    pool: &SqlitePool,
    game_id: &str,
) -> AppResult<(String, StoreSource, String)> {
    let entry: Option<(String, Option<String>)> =
        sqlx::query_as("SELECT source_id, store_game_id FROM library_entries WHERE game_id = ?")
            .bind(game_id)
            .fetch_optional(pool)
            .await?;
    let (source, store_game_id) =
        entry.ok_or_else(|| AppError::NotInLibrary(game_id.to_owned()))?;
    let store = StoreSource::parse(&source).filter(|s| *s != StoreSource::Manual);
    let (Some(store), Some(store_game_id)) = (store, store_game_id) else {
        return Err(AppError::InvalidInput(
            tr!("Dieses Spiel stammt aus keinem Store. Lade es über einen Link oder itch.io herunter oder lege den Pfad fest.", "This game doesn't come from a store. Download it from a link or itch.io, or set its path."),
        ));
    };
    let active: Option<String> = sqlx::query_scalar(
        "SELECT id FROM downloads WHERE game_id = ? AND queue_position IS NOT NULL",
    )
    .bind(game_id)
    .fetch_optional(pool)
    .await?;
    if let Some(id) = active {
        return Ok((id, store, store_game_id));
    }
    let id = new_id(game_id);
    sqlx::query(
        "INSERT INTO downloads (id, game_id, source_id, origin, status, queue_position, queued_at) \
         VALUES (?, ?, ?, ?, 'handedOff', (SELECT COALESCE(MAX(queue_position), -1) + 1 FROM downloads), ?)",
    )
    .bind(&id)
    .bind(game_id)
    .bind(store.as_str())
    .bind(store.as_str())
    .bind(now_iso())
    .execute(pool)
    .await?;
    Ok((id, store, store_game_id))
}

/// A store game Cove installs itself (GOG): into `install_path` below `target_dir`.
/// An update uses the existing game folder.
pub async fn enqueue_store_install(
    pool: &SqlitePool,
    game_id: &str,
    store: StoreSource,
    target_dir: &str,
    install_path: &str,
) -> AppResult<String> {
    let active: Option<String> = sqlx::query_scalar(
        "SELECT id FROM downloads WHERE game_id = ? AND queue_position IS NOT NULL",
    )
    .bind(game_id)
    .fetch_optional(pool)
    .await?;
    if active.is_some() {
        return Err(AppError::InvalidInput(tr!(
            "Dieses Spiel wird bereits heruntergeladen.",
            "This game is already downloading."
        )));
    }
    let id = new_id(game_id);
    sqlx::query(
        "INSERT INTO downloads (id, game_id, source_id, origin, status, queue_position, queued_at, target_dir, install_path) \
         VALUES (?, ?, ?, ?, 'queued', (SELECT COALESCE(MAX(queue_position), -1) + 1 FROM downloads), ?, ?, ?)",
    )
    .bind(&id)
    .bind(game_id)
    .bind(store.as_str())
    .bind(store.as_str())
    .bind(now_iso())
    .bind(target_dir)
    .bind(install_path)
    .execute(pool)
    .await?;
    Ok(id)
}

/// A file transfer (direct link or itch.io).
#[derive(Debug, Clone, Default)]
pub struct NewTransfer {
    pub title: String,
    pub origin: Option<DownloadOrigin>,
    pub url: Option<String>,
    pub file_name: Option<String>,
    pub sha256: Option<String>,
    pub target_dir: String,
    pub itch_game_id: Option<i64>,
    pub itch_upload_id: Option<i64>,
    pub itch_download_key_id: Option<i64>,
    /// The game folder, fixed when queued (itch.io games with extras).
    pub install_path: Option<String>,
    /// An extra of an already queued game: its game id and the file's name.
    pub extra_of: Option<(String, String)>,
}

/// Creates the game (metadata only; it joins the library once installed) and queues the transfer.
/// What the itch.io dialog picked: the game's file and extra files to put next to it.
#[derive(Debug, Clone, Default)]
pub struct ItchChoice {
    pub game_id: i64,
    pub upload_id: i64,
    pub download_key_id: Option<i64>,
    /// Upload ids of extras (soundtrack …); they go into the game's "Extras" folder.
    pub extras: Vec<i64>,
}

pub async fn enqueue_transfer(pool: &SqlitePool, input: &NewTransfer) -> AppResult<String> {
    let title = input.title.trim();
    if title.is_empty() {
        return Err(AppError::InvalidInput(tr!(
            "Bitte gib einen Namen ein.",
            "Please enter a name."
        )));
    }
    let origin = input.origin.unwrap_or(DownloadOrigin::Direct);
    let game_id = match &input.extra_of {
        Some((game_id, _)) => game_id.clone(),
        None => new_game(pool, input, title, origin).await?,
    };
    let now = now_iso();
    let id = new_id(&game_id);
    sqlx::query(
        "INSERT INTO downloads (id, game_id, source_id, origin, status, queue_position, queued_at, url, file_name, \
           checksum_sha256, target_dir, itch_game_id, itch_upload_id, itch_download_key_id, install_path, extra) \
         VALUES (?, ?, 'manual', ?, 'queued', (SELECT COALESCE(MAX(queue_position), -1) + 1 FROM downloads), ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
    )
    .bind(&id)
    .bind(&game_id)
    .bind(origin.as_str())
    .bind(&now)
    .bind(&input.url)
    .bind(&input.file_name)
    .bind(input.sha256.as_ref().map(|s| s.trim().to_lowercase()))
    .bind(&input.target_dir)
    .bind(input.itch_game_id)
    .bind(input.itch_upload_id)
    .bind(input.itch_download_key_id)
    .bind(&input.install_path)
    .bind(input.extra_of.as_ref().map(|(_, name)| name))
    .execute(pool)
    .await?;
    Ok(id)
}

/// A new game for a link or itch.io download (id from the title or itch id).
async fn new_game(
    pool: &SqlitePool,
    input: &NewTransfer,
    title: &str,
    origin: DownloadOrigin,
) -> AppResult<String> {
    let base = match (origin, input.itch_game_id) {
        (DownloadOrigin::Itch, Some(id)) => format!("itch-{id}"),
        _ => format!("dl-{}", slugify(title)),
    };
    let mut game_id = base.clone();
    let mut n = 2;
    loop {
        let in_library: Option<i64> =
            sqlx::query_scalar("SELECT 1 FROM library_entries WHERE game_id = ?")
                .bind(&game_id)
                .fetch_optional(pool)
                .await?;
        let active: Option<i64> = sqlx::query_scalar(
            "SELECT 1 FROM downloads WHERE game_id = ? AND queue_position IS NOT NULL",
        )
        .bind(&game_id)
        .fetch_optional(pool)
        .await?;
        if active.is_some() && origin == DownloadOrigin::Itch {
            return Err(AppError::InvalidInput(tr!(
                "„{title}“ wird bereits heruntergeladen.",
                "“{title}” is already downloading."
            )));
        }
        if in_library.is_none() && active.is_none() {
            break;
        }
        game_id = format!("{base}-{n}");
        n += 1;
    }
    let now = now_iso();
    let note = if matches!(origin, DownloadOrigin::Itch) {
        tr!("Von itch.io heruntergeladen.", "Downloaded from itch.io.")
    } else {
        tr!(
            "Über einen Link heruntergeladen.",
            "Downloaded from a link."
        )
    };
    sqlx::query(
        "INSERT INTO games (id, title, summary, description, platforms, release_date, created_at) \
         VALUES (?, ?, ?, ?, ?, '', ?) \
         ON CONFLICT (id) DO UPDATE SET title = excluded.title",
    )
    .bind(&game_id)
    .bind(title)
    .bind(&note)
    .bind(input.url.as_deref().map_or_else(
        || note.clone(),
        |u| tr!("{note}\n\nQuelle: {u}", "{note}\n\nSource: {u}"),
    ))
    .bind(serde_json::to_string(&[Platform::current()])?)
    .bind(&now)
    .execute(pool)
    .await?;
    Ok(game_id)
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
        return Err(AppError::InvalidInput(tr!(
            "Download nicht gefunden: {id}",
            "Download not found: {id}"
        )));
    }
    renumber(pool).await
}

pub async fn fail(pool: &SqlitePool, id: &str, message: &str) -> AppResult<()> {
    // Message first: whoever sees `failed` also sees why.
    sqlx::query("UPDATE downloads SET error = ? WHERE id = ?")
        .bind(message)
        .bind(id)
        .execute(pool)
        .await?;
    set_status(pool, id, DownloadStatus::Failed).await
}

/// What the server told us about the file.
pub async fn set_remote(
    pool: &SqlitePool,
    id: &str,
    total_bytes: i64,
    etag: Option<&str>,
    file_name: &str,
) -> AppResult<()> {
    sqlx::query("UPDATE downloads SET total_bytes = ?, etag = ?, file_name = ? WHERE id = ?")
        .bind(total_bytes)
        .bind(etag)
        .bind(file_name)
        .bind(id)
        .execute(pool)
        .await?;
    Ok(())
}

pub async fn save_progress(
    pool: &SqlitePool,
    id: &str,
    downloaded: i64,
    chunks_done: &[u64],
) -> AppResult<()> {
    sqlx::query("UPDATE downloads SET downloaded_bytes = ?, chunks_done = ? WHERE id = ?")
        .bind(downloaded)
        .bind(serde_json::to_string(chunks_done)?)
        .bind(id)
        .execute(pool)
        .await?;
    Ok(())
}

pub async fn set_checksum_verified(pool: &SqlitePool, id: &str) -> AppResult<()> {
    sqlx::query("UPDATE downloads SET checksum_verified = 1 WHERE id = ?")
        .bind(id)
        .execute(pool)
        .await?;
    Ok(())
}

/// Puts the installed game into the library (or updates the existing entry) and completes the download.
/// An extra is in the game's "Extras" folder; the library stays as it is.
pub async fn complete_extra(pool: &SqlitePool, id: &str, path: &str) -> AppResult<()> {
    sqlx::query("UPDATE downloads SET install_path = ? WHERE id = ?")
        .bind(path)
        .bind(id)
        .execute(pool)
        .await?;
    set_status(pool, id, DownloadStatus::Completed).await
}

/// Queued or running extras of a game.
pub async fn active_extras(pool: &SqlitePool, game_id: &str) -> AppResult<Vec<String>> {
    Ok(sqlx::query_scalar(
        "SELECT id FROM downloads WHERE game_id = ? AND extra IS NOT NULL AND queue_position IS NOT NULL",
    )
    .bind(game_id)
    .fetch_all(pool)
    .await?)
}

pub async fn complete_install(
    pool: &SqlitePool,
    id: &str,
    game_id: &str,
    install_path: &str,
    executable_path: Option<&str>,
    size_bytes: i64,
    compat: CompatibilityTool,
) -> AppResult<()> {
    let now = now_iso();
    sqlx::query(
        "INSERT INTO library_entries (game_id, source_id, installed, install_path, executable_path, install_size_bytes, \
           compatibility_tool, added_at) VALUES (?, 'manual', 1, ?, ?, ?, ?, ?) \
         ON CONFLICT (game_id) DO UPDATE SET installed = 1, install_path = excluded.install_path, \
           executable_path = excluded.executable_path, install_size_bytes = excluded.install_size_bytes",
    )
    .bind(game_id)
    .bind(install_path)
    .bind(executable_path)
    .bind(size_bytes)
    .bind(compat.as_str())
    .bind(&now)
    .execute(pool)
    .await?;
    sqlx::query("UPDATE downloads SET install_path = ?, executable_path = ? WHERE id = ?")
        .bind(install_path)
        .bind(executable_path)
        .bind(id)
        .execute(pool)
        .await?;
    set_status(pool, id, DownloadStatus::Completed).await
}

/// A store client finished the install it was handed.
pub async fn complete_handoff(pool: &SqlitePool, id: &str) -> AppResult<()> {
    set_status(pool, id, DownloadStatus::Completed).await
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
            enqueue_transfer(
                &pool,
                &NewTransfer {
                    title: id.into(),
                    url: Some(format!("https://x.test/{id}.zip")),
                    target_dir: "/g".into(),
                    ..NewTransfer::default()
                },
            )
            .await
            .unwrap();
        }
        let ids =
            |items: &[DownloadItem]| items.iter().map(|i| i.game.id.clone()).collect::<Vec<_>>();
        assert_eq!(ids(&list(&pool).await.unwrap()), ["dl-a", "dl-b", "dl-c"]);

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
        assert_eq!(ids(&list(&pool).await.unwrap()), ["dl-c", "dl-b", "dl-a"]);

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

        // Store games are handed off (once).
        insert_store_game(
            &pool,
            &sample_game("s", "S"),
            StoreSource::Steam,
            Some("620"),
            None,
        )
        .await
        .unwrap();
        let (first, store, store_id) = enqueue_handoff(&pool, "s").await.unwrap();
        assert_eq!((store, store_id.as_str()), (StoreSource::Steam, "620"));
        assert_eq!(enqueue_handoff(&pool, "s").await.unwrap().0, first);
        assert_eq!(handed_off(&pool).await.unwrap().len(), 1);
    }

    #[tokio::test]
    async fn manual_games_cannot_be_downloaded() {
        let pool = open_in_memory().await.unwrap();
        crate::db::library::add_from_catalog(&pool, &sample_game("m", "M"))
            .await
            .unwrap();
        assert!(matches!(
            enqueue_handoff(&pool, "m").await,
            Err(AppError::InvalidInput(_))
        ));
    }

    #[tokio::test]
    async fn transfers_create_a_game_and_join_the_library_when_installed() {
        let pool = open_in_memory().await.unwrap();
        let input = NewTransfer {
            title: "Lantern".into(),
            url: Some("https://example.test/lantern.zip".into()),
            sha256: Some(" ABCDEF ".into()),
            target_dir: "/games".into(),
            ..NewTransfer::default()
        };
        let id = enqueue_transfer(&pool, &input).await.unwrap();
        let saved = record(&pool, &id).await.unwrap();
        assert_eq!(
            (saved.game_id.as_str(), saved.title.as_str()),
            ("dl-lantern", "Lantern")
        );
        assert_eq!(saved.checksum_sha256.as_deref(), Some("abcdef"));
        assert_eq!(queued(&pool).await.unwrap(), std::slice::from_ref(&id));
        assert!(
            crate::db::library::get(&pool, "dl-lantern")
                .await
                .unwrap()
                .is_none(),
            "not in the library yet"
        );

        save_progress(&pool, &id, 10, &[0, 2]).await.unwrap();
        set_status(&pool, &id, DownloadStatus::Downloading)
            .await
            .unwrap();
        assert_eq!(requeue_interrupted(&pool).await.unwrap(), 1);
        assert_eq!(record_status(&pool, &id).await, "queued");

        complete_install(
            &pool,
            &id,
            "dl-lantern",
            "/games/lantern",
            Some("/games/lantern/run"),
            99,
            CompatibilityTool::Native,
        )
        .await
        .unwrap();
        let item = crate::db::library::require(&pool, "dl-lantern")
            .await
            .unwrap();
        assert!(item.entry.installed);
        assert_eq!(
            item.entry.executable_path.as_deref(),
            Some("/games/lantern/run")
        );
        let download = get(&pool, &id).await.unwrap().download;
        assert_eq!(
            (download.status, download.queue_position),
            (DownloadStatus::Completed, None)
        );

        // The same title again gets its own game.
        let again = enqueue_transfer(&pool, &input).await.unwrap();
        assert_eq!(record(&pool, &again).await.unwrap().game_id, "dl-lantern-2");
    }

    async fn record_status(pool: &SqlitePool, id: &str) -> String {
        record(pool, id).await.unwrap().status
    }
}
