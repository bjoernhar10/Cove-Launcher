//! SQLite persistence (sqlx). All queries are runtime-checked so the crate
//! builds without a live database; migrations are embedded at compile time.

pub mod achievements;
pub mod downloads;
pub mod library;
pub mod sessions;
pub mod settings;
pub mod sources;

use std::path::Path;

use sqlx::sqlite::{SqliteConnectOptions, SqliteJournalMode, SqlitePoolOptions};
use sqlx::SqlitePool;

use crate::error::AppResult;

static MIGRATOR: sqlx::migrate::Migrator = sqlx::migrate!("./migrations");

/// Opens (or creates) the database file and runs pending migrations.
pub async fn open(path: &Path) -> AppResult<SqlitePool> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let options = SqliteConnectOptions::new()
        .filename(path)
        .create_if_missing(true)
        .journal_mode(SqliteJournalMode::Wal)
        .foreign_keys(true);
    let pool = SqlitePoolOptions::new()
        .max_connections(4)
        .connect_with(options)
        .await?;
    MIGRATOR.run(&pool).await?;
    Ok(pool)
}

/// Fresh in-memory database with all migrations applied (tests).
#[cfg(test)]
pub async fn open_in_memory() -> AppResult<SqlitePool> {
    use std::str::FromStr;
    let options = SqliteConnectOptions::from_str("sqlite::memory:")?.foreign_keys(true);
    // One connection: every in-memory connection would be its own database.
    let pool = SqlitePoolOptions::new()
        .max_connections(1)
        .connect_with(options)
        .await?;
    MIGRATOR.run(&pool).await?;
    Ok(pool)
}

/// Lowercase ASCII slug for ids: "Ember & Oak" → "ember-and-oak".
pub fn slugify(title: &str) -> String {
    let mut slug = String::with_capacity(title.len());
    let mut last_dash = true;
    for c in title.replace('&', " and ").chars() {
        if c.is_ascii_alphanumeric() {
            slug.push(c.to_ascii_lowercase());
            last_dash = false;
        } else if !last_dash {
            slug.push('-');
            last_dash = true;
        }
    }
    while slug.ends_with('-') {
        slug.pop();
    }
    slug
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn slugify_titles() {
        assert_eq!(slugify("Ember & Oak"), "ember-and-oak");
        assert_eq!(
            slugify("  Half—Life 2: Episode One "),
            "half-life-2-episode-one"
        );
        assert_eq!(slugify("???"), "");
    }

    #[tokio::test]
    async fn migrations_seed_sources() {
        let pool = open_in_memory().await.unwrap();
        let count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM sources")
            .fetch_one(&pool)
            .await
            .unwrap();
        assert_eq!(count, 4);
    }
}
