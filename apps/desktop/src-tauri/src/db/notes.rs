//! The user's own notes per game.

use sqlx::SqlitePool;

use crate::error::{AppError, AppResult};
use crate::models::{now_iso, GameNote};

/// Longest note, in characters.
pub const MAX_CHARS: usize = 20_000;

pub async fn get(pool: &SqlitePool, game_id: &str) -> AppResult<Option<GameNote>> {
    let row: Option<(String, String)> =
        sqlx::query_as("SELECT text, updated_at FROM game_notes WHERE game_id = ?")
            .bind(game_id)
            .fetch_optional(pool)
            .await?;
    Ok(row.map(|(text, updated_at)| GameNote { text, updated_at }))
}

/// Stores the note (trailing white space dropped); an empty note is removed.
pub async fn set(pool: &SqlitePool, game_id: &str, text: &str) -> AppResult<Option<GameNote>> {
    let text = text.trim_end();
    if text.chars().count() > MAX_CHARS {
        return Err(AppError::InvalidInput(tr!(
            "Die Notiz ist zu lang (höchstens {MAX_CHARS} Zeichen).",
            "The note is too long ({MAX_CHARS} characters at most)."
        )));
    }
    if text.trim().is_empty() {
        sqlx::query("DELETE FROM game_notes WHERE game_id = ?")
            .bind(game_id)
            .execute(pool)
            .await?;
        return Ok(None);
    }
    write(pool, game_id, text, &now_iso()).await?;
    get(pool, game_id).await
}

/// Writes a note with the given time (imports and library sync keep the original time).
pub async fn write(
    pool: &SqlitePool,
    game_id: &str,
    text: &str,
    updated_at: &str,
) -> AppResult<()> {
    sqlx::query(
        "INSERT INTO game_notes (game_id, text, updated_at) VALUES (?, ?, ?) \
         ON CONFLICT (game_id) DO UPDATE SET text = excluded.text, updated_at = excluded.updated_at",
    )
    .bind(game_id)
    .bind(text)
    .bind(updated_at)
    .execute(pool)
    .await?;
    Ok(())
}

/// Every note, by game id.
pub async fn all(pool: &SqlitePool) -> AppResult<Vec<(String, GameNote)>> {
    let rows: Vec<(String, String, String)> =
        sqlx::query_as("SELECT game_id, text, updated_at FROM game_notes ORDER BY game_id")
            .fetch_all(pool)
            .await?;
    Ok(rows
        .into_iter()
        .map(|(id, text, updated_at)| (id, GameNote { text, updated_at }))
        .collect())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::{library, open_in_memory};
    use crate::models::ManualGameInput;

    #[tokio::test]
    async fn notes_are_kept_trimmed_and_removed_when_empty() {
        let pool = open_in_memory().await.unwrap();
        let item = library::add_manual(
            &pool,
            &ManualGameInput {
                title: "Lantern".into(),
                executable_path: "/g/lantern".into(),
                size_bytes: None,
            },
        )
        .await
        .unwrap();
        let id = &item.game.id;
        assert_eq!(get(&pool, id).await.unwrap(), None);
        let note = set(&pool, id, "Chapter 3, find the key  \n\n")
            .await
            .unwrap()
            .unwrap();
        assert_eq!(note.text, "Chapter 3, find the key");
        assert_eq!(get(&pool, id).await.unwrap(), Some(note));
        assert_eq!(all(&pool).await.unwrap().len(), 1);

        let too_long = "x".repeat(MAX_CHARS + 1);
        assert!(matches!(
            set(&pool, id, &too_long).await,
            Err(AppError::InvalidInput(_))
        ));

        assert_eq!(set(&pool, id, "  \n").await.unwrap(), None);
        assert_eq!(get(&pool, id).await.unwrap(), None);
    }
}
