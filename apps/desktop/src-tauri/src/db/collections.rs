//! The user's own collections: named groups of library games.

use std::collections::HashMap;

use sqlx::SqlitePool;

use crate::error::{AppError, AppResult};
use crate::models::{now_iso, Collection};

/// Longest collection name, in characters.
pub const MAX_NAME_CHARS: usize = 50;
/// More collections than this no longer fit the library's group bar.
pub const MAX_COLLECTIONS: usize = 100;

fn missing() -> AppError {
    AppError::InvalidInput(tr!(
        "Diese Sammlung gibt es nicht mehr.",
        "This collection no longer exists."
    ))
}

/// The row id behind the id the UI holds.
fn row_id(collection_id: &str) -> AppResult<i64> {
    collection_id.parse().map_err(|_| missing())
}

/// The name as it is stored; refuses empty, overlong and taken names
/// (`except` is the collection being renamed).
async fn checked_name(pool: &SqlitePool, name: &str, except: Option<i64>) -> AppResult<String> {
    let name = name.split_whitespace().collect::<Vec<_>>().join(" ");
    if name.is_empty() {
        return Err(AppError::InvalidInput(tr!(
            "Bitte gib der Sammlung einen Namen.",
            "Please give the collection a name."
        )));
    }
    if name.chars().count() > MAX_NAME_CHARS {
        return Err(AppError::InvalidInput(tr!(
            "Der Name ist zu lang (höchstens {MAX_NAME_CHARS} Zeichen).",
            "The name is too long ({MAX_NAME_CHARS} characters at most)."
        )));
    }
    // Compared in Rust: SQLite's NOCASE only knows ASCII ("Übung" vs "übung").
    let taken: Vec<(i64, String)> = sqlx::query_as("SELECT id, name FROM collections")
        .fetch_all(pool)
        .await?;
    let wanted = name.to_lowercase();
    if taken
        .iter()
        .any(|(id, other)| Some(*id) != except && other.to_lowercase() == wanted)
    {
        return Err(AppError::InvalidInput(tr!(
            "Eine Sammlung „{name}“ gibt es schon.",
            "A collection “{name}” already exists."
        )));
    }
    Ok(name)
}

/// All collections by name, each with its games that are in the library.
pub async fn list(pool: &SqlitePool) -> AppResult<Vec<Collection>> {
    let rows: Vec<(i64, String)> = sqlx::query_as("SELECT id, name FROM collections")
        .fetch_all(pool)
        .await?;
    let members: Vec<(i64, String)> = sqlx::query_as(
        "SELECT c.collection_id, c.game_id FROM collection_games c \
         JOIN library_entries e ON e.game_id = c.game_id \
         ORDER BY c.added_at, c.game_id",
    )
    .fetch_all(pool)
    .await?;
    let mut games: HashMap<i64, Vec<String>> = HashMap::new();
    for (collection, game) in members {
        games.entry(collection).or_default().push(game);
    }
    let mut collections: Vec<Collection> = rows
        .into_iter()
        .map(|(id, name)| Collection {
            id: id.to_string(),
            name,
            game_ids: games.remove(&id).unwrap_or_default(),
        })
        .collect();
    collections.sort_by_cached_key(|c| (c.name.to_lowercase(), c.id.clone()));
    Ok(collections)
}

async fn require(pool: &SqlitePool, id: i64) -> AppResult<Collection> {
    let wanted = id.to_string();
    list(pool)
        .await?
        .into_iter()
        .find(|c| c.id == wanted)
        .ok_or_else(missing)
}

pub async fn create(pool: &SqlitePool, name: &str) -> AppResult<Collection> {
    let name = checked_name(pool, name, None).await?;
    let count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM collections")
        .fetch_one(pool)
        .await?;
    if usize::try_from(count).unwrap_or(usize::MAX) >= MAX_COLLECTIONS {
        return Err(AppError::InvalidInput(tr!(
            "Mehr als {MAX_COLLECTIONS} Sammlungen gehen nicht. Lösche zuerst eine.",
            "More than {MAX_COLLECTIONS} collections are not possible. Delete one first."
        )));
    }
    let id = sqlx::query("INSERT INTO collections (name, created_at) VALUES (?, ?)")
        .bind(&name)
        .bind(now_iso())
        .execute(pool)
        .await?
        .last_insert_rowid();
    Ok(Collection {
        id: id.to_string(),
        name,
        game_ids: Vec::new(),
    })
}

pub async fn rename(pool: &SqlitePool, collection_id: &str, name: &str) -> AppResult<Collection> {
    let id = row_id(collection_id)?;
    let name = checked_name(pool, name, Some(id)).await?;
    let result = sqlx::query("UPDATE collections SET name = ? WHERE id = ?")
        .bind(&name)
        .bind(id)
        .execute(pool)
        .await?;
    if result.rows_affected() == 0 {
        return Err(missing());
    }
    require(pool, id).await
}

/// Deletes the collection; its games stay in the library.
pub async fn delete(pool: &SqlitePool, collection_id: &str) -> AppResult<()> {
    let result = sqlx::query("DELETE FROM collections WHERE id = ?")
        .bind(row_id(collection_id)?)
        .execute(pool)
        .await?;
    if result.rows_affected() == 0 {
        return Err(missing());
    }
    Ok(())
}

/// Puts a library game into the collection or takes it out. Doing either
/// twice is fine.
pub async fn set_game(
    pool: &SqlitePool,
    collection_id: &str,
    game_id: &str,
    member: bool,
) -> AppResult<()> {
    let id = row_id(collection_id)?;
    let exists: Option<i64> = sqlx::query_scalar("SELECT 1 FROM collections WHERE id = ?")
        .bind(id)
        .fetch_optional(pool)
        .await?;
    if exists.is_none() {
        return Err(missing());
    }
    if member {
        super::library::require(pool, game_id).await?;
        sqlx::query(
            "INSERT OR IGNORE INTO collection_games (collection_id, game_id, added_at) VALUES (?, ?, ?)",
        )
        .bind(id)
        .bind(game_id)
        .bind(now_iso())
        .execute(pool)
        .await?;
    } else {
        sqlx::query("DELETE FROM collection_games WHERE collection_id = ? AND game_id = ?")
            .bind(id)
            .bind(game_id)
            .execute(pool)
            .await?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::library::tests::sample_game;
    use crate::db::{library, open_in_memory};

    fn names(collections: &[Collection]) -> Vec<&str> {
        collections.iter().map(|c| c.name.as_str()).collect()
    }

    #[tokio::test]
    async fn collections_are_listed_by_name_with_their_games() {
        let pool = open_in_memory().await.unwrap();
        for (id, title) in [("a", "Alpha"), ("b", "Beta")] {
            library::add_from_catalog(&pool, &sample_game(id, title))
                .await
                .unwrap();
        }
        let friends = create(&pool, "  mit   Freunden ").await.unwrap();
        assert_eq!(friends.name, "mit Freunden");
        let started = create(&pool, "Angefangen").await.unwrap();
        create(&pool, "Übung").await.unwrap();

        set_game(&pool, &friends.id, "a", true).await.unwrap();
        set_game(&pool, &friends.id, "b", true).await.unwrap();
        set_game(&pool, &friends.id, "b", true).await.unwrap();
        set_game(&pool, &started.id, "b", true).await.unwrap();
        set_game(&pool, &started.id, "a", false).await.unwrap();

        let all = list(&pool).await.unwrap();
        assert_eq!(names(&all), ["Angefangen", "mit Freunden", "Übung"]);
        assert_eq!(all[0].game_ids, ["b"]);
        assert_eq!(all[1].game_ids, ["a", "b"]);
        assert_eq!(all[2].game_ids.len(), 0);

        set_game(&pool, &friends.id, "a", false).await.unwrap();
        assert_eq!(list(&pool).await.unwrap()[1].game_ids, ["b"]);
    }

    #[tokio::test]
    async fn names_must_be_given_short_and_unique() {
        let pool = open_in_memory().await.unwrap();
        let first = create(&pool, "Übung").await.unwrap();
        let second = create(&pool, "Koop").await.unwrap();

        for bad in ["", "   ", "übung", &"x".repeat(MAX_NAME_CHARS + 1)] {
            let error = create(&pool, bad).await.unwrap_err();
            assert!(matches!(error, AppError::InvalidInput(_)), "{bad:?}");
        }
        assert_eq!(
            create(&pool, "ÜBUNG").await.unwrap_err().user_message(),
            "Eine Sammlung „ÜBUNG“ gibt es schon."
        );
        assert!(create(&pool, &"x".repeat(MAX_NAME_CHARS)).await.is_ok());

        // Renaming keeps the id; the own name in another case is fine, a taken one is not.
        let renamed = rename(&pool, &first.id, "ÜBUNG").await.unwrap();
        assert_eq!(
            (renamed.id.as_str(), renamed.name.as_str()),
            (first.id.as_str(), "ÜBUNG")
        );
        assert!(rename(&pool, &second.id, "übung").await.is_err());
        assert!(rename(&pool, "999", "Neu").await.is_err());
        assert!(rename(&pool, "abc", "Neu").await.is_err());
    }

    #[tokio::test]
    async fn only_library_games_and_existing_collections_are_accepted() {
        let pool = open_in_memory().await.unwrap();
        let collection = create(&pool, "Koop").await.unwrap();
        assert!(matches!(
            set_game(&pool, &collection.id, "nope", true).await,
            Err(AppError::NotInLibrary(_))
        ));
        library::add_from_catalog(&pool, &sample_game("a", "Alpha"))
            .await
            .unwrap();
        assert!(matches!(
            set_game(&pool, "999", "a", true).await,
            Err(AppError::InvalidInput(_))
        ));
        assert!(delete(&pool, "999").await.is_err());
    }

    #[tokio::test]
    async fn a_removed_game_leaves_its_collections_until_it_is_back() {
        let pool = open_in_memory().await.unwrap();
        let game = sample_game("a", "Alpha");
        library::add_from_catalog(&pool, &game).await.unwrap();
        let collection = create(&pool, "Koop").await.unwrap();
        set_game(&pool, &collection.id, "a", true).await.unwrap();

        library::remove(&pool, "a").await.unwrap();
        assert_eq!(list(&pool).await.unwrap()[0].game_ids.len(), 0);
        library::add_from_catalog(&pool, &game).await.unwrap();
        assert_eq!(list(&pool).await.unwrap()[0].game_ids, ["a"]);

        // Deleting the collection keeps the game.
        delete(&pool, &collection.id).await.unwrap();
        assert_eq!(list(&pool).await.unwrap().len(), 0);
        assert!(library::get(&pool, "a").await.unwrap().is_some());
    }

    #[tokio::test]
    async fn the_number_of_collections_is_limited() {
        let pool = open_in_memory().await.unwrap();
        for i in 0..MAX_COLLECTIONS {
            create(&pool, &format!("Sammlung {i}")).await.unwrap();
        }
        assert!(create(&pool, "Eine zu viel").await.is_err());
    }
}
