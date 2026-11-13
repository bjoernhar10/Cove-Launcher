//! Games + library entries.

use std::collections::HashSet;
use std::path::Path;

use sqlx::{FromRow, SqlitePool};

use crate::db::slugify;
use crate::error::{AppError, AppResult};
use crate::models::{
    now_iso, AchievementProgress, CompatibilityTool, EnvVar, Game, GameArtwork, GameId, ImageRef,
    LaunchOptions, LibraryEntry, LibraryItem, LibraryUpdate, ManualGameInput, MatchCandidate,
    MatchStatus, Platform, StoreSource,
};

#[derive(Debug, FromRow)]
pub(crate) struct GameRow {
    pub id: String,
    pub title: String,
    pub summary: String,
    pub description: String,
    pub genres: String,
    pub platforms: String,
    pub release_date: String,
    pub developer: String,
    pub publisher: String,
    pub rating: Option<i64>,
    pub popularity: i64,
    pub cover_url: Option<String>,
    pub header_url: Option<String>,
    pub screenshots: String,
    pub features: Option<String>,
}

pub(crate) const GAME_COLUMNS: &str =
    "g.id, g.title, g.summary, g.description, g.genres, g.platforms, g.release_date, \
     g.developer, g.publisher, g.rating, g.popularity, g.cover_url, g.header_url, g.screenshots, g.features";

impl GameRow {
    pub(crate) fn into_game(self) -> AppResult<Game> {
        let screenshots: Vec<Option<String>> = serde_json::from_str(&self.screenshots)?;
        let title = self.title;
        // Unreadable feature JSON (older format) simply means "unknown".
        let features = self
            .features
            .as_deref()
            .and_then(|f| serde_json::from_str(f).ok());
        Ok(Game {
            features,
            artwork: GameArtwork {
                cover: ImageRef {
                    url: self.cover_url,
                    alt: format!("Cover von {title}"),
                },
                header: ImageRef {
                    url: self.header_url,
                    alt: format!("Titelbild von {title}"),
                },
                screenshots: screenshots
                    .into_iter()
                    .enumerate()
                    .map(|(i, url)| ImageRef {
                        url,
                        alt: format!("Screenshot {} von {title}", i + 1),
                    })
                    .collect(),
            },
            id: self.id,
            summary: self.summary,
            description: self.description,
            genres: serde_json::from_str(&self.genres)?,
            platforms: serde_json::from_str(&self.platforms)?,
            // Stored as '' when unknown (column is NOT NULL since 0001).
            release_date: Some(self.release_date).filter(|d| !d.is_empty()),
            developer: self.developer,
            publisher: self.publisher,
            rating: self.rating,
            popularity: self.popularity,
            title,
        })
    }
}

#[derive(Debug, FromRow)]
struct EntryRow {
    #[sqlx(flatten)]
    game: GameRow,
    source_id: String,
    store_game_id: Option<String>,
    installed: bool,
    install_path: Option<String>,
    executable_path: Option<String>,
    install_size_bytes: Option<i64>,
    favorite: bool,
    playtime_offset_minutes: i64,
    launch_arguments: String,
    compatibility_tool: String,
    proton_version: Option<String>,
    env: String,
    tweaks: Option<String>,
    added_at: String,
    played_seconds: i64,
    last_played_at: Option<String>,
    achievements_total: i64,
    achievements_unlocked: i64,
    catalog_id: Option<String>,
    match_status: String,
    installed_by_cove: bool,
}

impl EntryRow {
    fn into_item(self) -> AppResult<LibraryItem> {
        let env: Vec<EnvVar> = serde_json::from_str(&self.env)?;
        let game = self.game.into_game()?;
        let source = StoreSource::parse(&self.source_id).unwrap_or(StoreSource::Manual);
        let launch_via = crate::launcher::launch_via(
            source,
            self.store_game_id.as_deref(),
            self.executable_path.as_deref(),
            self.installed_by_cove,
        );
        Ok(LibraryItem {
            entry: LibraryEntry {
                game_id: game.id.clone(),
                source,
                store_game_id: self.store_game_id,
                installed: self.installed,
                install_path: self.install_path,
                executable_path: self.executable_path,
                install_size_bytes: self.install_size_bytes,
                favorite: self.favorite,
                playtime_minutes: self.playtime_offset_minutes + self.played_seconds / 60,
                last_played_at: self.last_played_at,
                added_at: self.added_at,
                achievements: AchievementProgress {
                    unlocked: self.achievements_unlocked,
                    total: self.achievements_total,
                },
                launch_options: LaunchOptions {
                    arguments: self.launch_arguments,
                    compatibility_tool: CompatibilityTool::parse(&self.compatibility_tool),
                    env,
                    proton_version: self.proton_version,
                    // Unreadable or missing (older rows): the defaults.
                    tweaks: self
                        .tweaks
                        .as_deref()
                        .and_then(|t| serde_json::from_str(t).ok())
                        .unwrap_or_default(),
                },
                catalog_id: self.catalog_id,
                match_status: MatchStatus::parse(&self.match_status),
                launch_via,
                installed_by_cove: self.installed_by_cove,
            },
            game,
        })
    }
}

fn select_entries(filter: &str) -> String {
    format!(
        "SELECT {GAME_COLUMNS}, e.source_id, e.store_game_id, e.installed, e.install_path, e.executable_path, \
           e.install_size_bytes, e.favorite, e.playtime_offset_minutes, e.launch_arguments, e.compatibility_tool, \
           e.proton_version, e.env, e.tweaks, e.added_at, e.catalog_id, e.match_status, \
           (SELECT COALESCE(SUM(s.duration_seconds), 0) FROM play_sessions s WHERE s.game_id = g.id) AS played_seconds, \
           (SELECT COALESCE(MAX(MAX(COALESCE(s.ended_at, s.started_at)), e.store_last_played_at), \
              MAX(COALESCE(s.ended_at, s.started_at)), e.store_last_played_at) \
              FROM play_sessions s WHERE s.game_id = g.id) AS last_played_at, \
           (SELECT COUNT(*) FROM achievements a WHERE a.game_id = g.id) AS achievements_total, \
           (SELECT COUNT(*) FROM user_achievements u WHERE u.game_id = g.id) AS achievements_unlocked, \
           EXISTS (SELECT 1 FROM downloads d WHERE d.game_id = g.id AND d.status = 'completed' \
             AND d.install_path = e.install_path) AS installed_by_cove \
         FROM library_entries e JOIN games g ON g.id = e.game_id {filter}"
    )
}

pub async fn list(pool: &SqlitePool) -> AppResult<Vec<LibraryItem>> {
    let rows: Vec<EntryRow> = sqlx::query_as(&select_entries("ORDER BY g.title COLLATE NOCASE"))
        .fetch_all(pool)
        .await?;
    rows.into_iter().map(EntryRow::into_item).collect()
}

pub async fn get(pool: &SqlitePool, game_id: &str) -> AppResult<Option<LibraryItem>> {
    // A catalogue id finds the library game linked to it as well.
    let row: Option<EntryRow> = sqlx::query_as(&select_entries(
        "WHERE g.id = ?1 OR e.catalog_id = ?1 LIMIT 1",
    ))
    .bind(game_id)
    .fetch_optional(pool)
    .await?;
    row.map(EntryRow::into_item).transpose()
}

pub async fn require(pool: &SqlitePool, game_id: &str) -> AppResult<LibraryItem> {
    get(pool, game_id)
        .await?
        .ok_or_else(|| AppError::NotInLibrary(game_id.to_owned()))
}

/// Executable paths already in the library (for marking scan results).
pub async fn executable_paths(pool: &SqlitePool) -> AppResult<HashSet<String>> {
    let paths: Vec<String> = sqlx::query_scalar(
        "SELECT executable_path FROM library_entries WHERE executable_path IS NOT NULL",
    )
    .fetch_all(pool)
    .await?;
    Ok(paths.into_iter().collect())
}

async fn unique_game_id(pool: &SqlitePool, base: &str) -> AppResult<GameId> {
    let base = if base.is_empty() {
        "game".to_owned()
    } else {
        base.to_owned()
    };
    let mut candidate = format!("manual-{base}");
    let mut n = 2;
    loop {
        let exists: Option<i64> = sqlx::query_scalar("SELECT 1 FROM games WHERE id = ?")
            .bind(&candidate)
            .fetch_optional(pool)
            .await?;
        if exists.is_none() {
            return Ok(candidate);
        }
        candidate = format!("manual-{base}-{n}");
        n += 1;
    }
}

/// Adds a game the user points at by executable path.
pub async fn add_manual(pool: &SqlitePool, input: &ManualGameInput) -> AppResult<LibraryItem> {
    let title = input.title.trim();
    let executable = input.executable_path.trim();
    if title.is_empty() {
        return Err(AppError::InvalidInput(tr!(
            "Bitte gib einen Namen ein.",
            "Please enter a name."
        )));
    }
    if executable.is_empty() {
        return Err(AppError::InvalidInput(tr!(
            "Bitte gib den Pfad zur ausführbaren Datei an.",
            "Please enter the path to the executable."
        )));
    }
    if executable_paths(pool).await?.contains(executable) {
        return Err(AppError::AlreadyInLibrary(title.to_owned()));
    }

    let id = unique_game_id(pool, &slugify(title)).await?;
    let now = now_iso();
    let install_dir = Path::new(executable)
        .parent()
        .map(|p| p.to_string_lossy().into_owned());
    let platforms = serde_json::to_string(&[Platform::current()])?;

    let mut tx = pool.begin().await?;
    sqlx::query(
        "INSERT INTO games (id, title, summary, description, platforms, release_date, created_at) \
         VALUES (?, ?, 'Manuell hinzugefügt.', ?, ?, ?, ?)",
    )
    .bind(&id)
    .bind(title)
    .bind(tr!(
        "Manuell hinzugefügt.\n\nPfad: {executable}",
        "Added manually.\n\nPath: {executable}"
    ))
    .bind(platforms)
    .bind(&now[..10])
    .bind(&now)
    .execute(&mut *tx)
    .await?;
    sqlx::query(
        "INSERT INTO library_entries (game_id, source_id, installed, install_path, executable_path, install_size_bytes, added_at) \
         VALUES (?, 'manual', 1, ?, ?, ?, ?)",
    )
    .bind(&id)
    .bind(install_dir)
    .bind(executable)
    .bind(input.size_bytes)
    .bind(&now)
    .execute(&mut *tx)
    .await?;
    tx.commit().await?;

    require(pool, &id).await
}

/// A game without install (plugins, imports): Cove looks up catalogue data later.
pub async fn add_not_installed(pool: &SqlitePool, title: &str) -> AppResult<LibraryItem> {
    let title = title.trim();
    if title.is_empty() || title.chars().count() > 200 {
        return Err(AppError::InvalidInput(tr!(
            "Der Name fehlt oder ist zu lang.",
            "The name is missing or too long."
        )));
    }
    let exists: Option<i64> = sqlx::query_scalar(
        "SELECT 1 FROM library_entries e JOIN games g ON g.id = e.game_id WHERE LOWER(g.title) = LOWER(?)",
    )
    .bind(title)
    .fetch_optional(pool)
    .await?;
    if exists.is_some() {
        return Err(AppError::AlreadyInLibrary(title.to_owned()));
    }
    let id = unique_game_id(pool, &slugify(title)).await?;
    let now = now_iso();
    let mut tx = pool.begin().await?;
    sqlx::query("INSERT INTO games (id, title, platforms, release_date, created_at) VALUES (?, ?, ?, '', ?)")
        .bind(&id)
        .bind(title)
        .bind(serde_json::to_string(&[Platform::current()])?)
        .bind(&now)
        .execute(&mut *tx)
        .await?;
    sqlx::query("INSERT INTO library_entries (game_id, source_id, installed, added_at) VALUES (?, 'manual', 0, ?)")
        .bind(&id)
        .bind(&now)
        .execute(&mut *tx)
        .await?;
    tx.commit().await?;
    require(pool, &id).await
}

/// Stores catalogue metadata and adds the game as a (not yet set up) library entry.
/// Stores a catalogue game's data (new row or refreshed), without a library entry.
pub(crate) async fn upsert_game(conn: &mut sqlx::SqliteConnection, game: &Game) -> AppResult<()> {
    let now = now_iso();
    let screenshots: Vec<Option<&str>> = game
        .artwork
        .screenshots
        .iter()
        .map(|s| s.url.as_deref())
        .collect();
    sqlx::query(
        "INSERT INTO games (id, title, summary, description, genres, platforms, release_date, developer, publisher, \
           rating, popularity, cover_url, header_url, screenshots, created_at, features) \
         VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?) \
         ON CONFLICT (id) DO UPDATE SET title = excluded.title, summary = excluded.summary, \
           description = excluded.description, genres = excluded.genres, platforms = excluded.platforms, \
           release_date = excluded.release_date, developer = excluded.developer, publisher = excluded.publisher, \
           rating = excluded.rating, popularity = excluded.popularity, cover_url = excluded.cover_url, \
           header_url = excluded.header_url, screenshots = excluded.screenshots, \
           features = excluded.features",
    )
    .bind(&game.id)
    .bind(&game.title)
    .bind(&game.summary)
    .bind(&game.description)
    .bind(serde_json::to_string(&game.genres)?)
    .bind(serde_json::to_string(&game.platforms)?)
    .bind(game.release_date.as_deref().unwrap_or(""))
    .bind(&game.developer)
    .bind(&game.publisher)
    .bind(game.rating)
    .bind(game.popularity)
    .bind(&game.artwork.cover.url)
    .bind(&game.artwork.header.url)
    .bind(serde_json::to_string(&screenshots)?)
    .bind(&now)
    .bind(game.features.as_ref().map(serde_json::to_string).transpose()?)
    .execute(conn)
    .await?;
    Ok(())
}

pub async fn add_from_catalog(pool: &SqlitePool, game: &Game) -> AppResult<LibraryItem> {
    if game.id.trim().is_empty() || game.title.trim().is_empty() {
        return Err(AppError::InvalidInput(tr!(
            "Ungültiges Spiel.",
            "Invalid game."
        )));
    }
    if get(pool, &game.id).await?.is_some() {
        return Err(AppError::AlreadyInLibrary(game.title.clone()));
    }
    let now = now_iso();
    let mut tx = pool.begin().await?;
    upsert_game(&mut tx, game).await?;
    sqlx::query(
        "INSERT INTO library_entries (game_id, source_id, installed, added_at, catalog_id, match_status) \
         VALUES (?1, 'manual', 0, ?2, ?1, 'matched')",
    )
    .bind(&game.id)
        .bind(&now)
        .execute(&mut *tx)
        .await?;
    tx.commit().await?;
    require(pool, &game.id).await
}

/// Removes the library entry. Game metadata and play sessions are kept, so
/// playtime comes back if the game is added again.
pub async fn remove(pool: &SqlitePool, game_id: &str) -> AppResult<()> {
    let result = sqlx::query("DELETE FROM library_entries WHERE game_id = ?")
        .bind(game_id)
        .execute(pool)
        .await?;
    if result.rows_affected() == 0 {
        return Err(AppError::NotInLibrary(game_id.to_owned()));
    }
    Ok(())
}

/// New folder after "Spiel verschieben" (also on the download that installed it).
pub async fn set_install_location(
    pool: &SqlitePool,
    game_id: &str,
    install_path: &str,
    executable_path: Option<&str>,
) -> AppResult<()> {
    let mut tx = pool.begin().await?;
    let result = sqlx::query(
        "UPDATE library_entries SET installed = 1, install_path = ?, executable_path = ? WHERE game_id = ?",
    )
    .bind(install_path)
    .bind(executable_path)
    .bind(game_id)
    .execute(&mut *tx)
    .await?;
    if result.rows_affected() == 0 {
        return Err(AppError::NotInLibrary(game_id.to_owned()));
    }
    sqlx::query(
        "UPDATE downloads SET install_path = ?, executable_path = ? \
         WHERE game_id = ? AND install_path IS NOT NULL",
    )
    .bind(install_path)
    .bind(executable_path)
    .bind(game_id)
    .execute(&mut *tx)
    .await?;
    tx.commit().await?;
    Ok(())
}

/// Windows games installed by Cove on Linux start through Proton unless the user chose otherwise.
pub async fn set_compat_if_native(
    pool: &SqlitePool,
    game_id: &str,
    tool: CompatibilityTool,
) -> AppResult<()> {
    sqlx::query(
        "UPDATE library_entries SET compatibility_tool = ? WHERE game_id = ? AND compatibility_tool = 'native'",
    )
    .bind(tool.as_str())
    .bind(game_id)
    .execute(pool)
    .await?;
    Ok(())
}

/// The game stays in the library, but is no longer installed.
pub async fn mark_uninstalled(pool: &SqlitePool, game_id: &str) -> AppResult<()> {
    for table in ["store_installs", "store_install_dlcs"] {
        sqlx::query(&format!("DELETE FROM {table} WHERE game_id = ?"))
            .bind(game_id)
            .execute(pool)
            .await?;
    }
    sqlx::query(
        "UPDATE library_entries SET installed = 0, install_path = NULL, executable_path = NULL, \
           install_size_bytes = NULL WHERE game_id = ?",
    )
    .bind(game_id)
    .execute(pool)
    .await?;
    Ok(())
}

pub async fn update(
    pool: &SqlitePool,
    game_id: &str,
    patch: &LibraryUpdate,
) -> AppResult<LibraryItem> {
    require(pool, game_id).await?;
    let mut tx = pool.begin().await?;

    if let Some(title) = &patch.title {
        let title = title.trim();
        if title.is_empty() {
            return Err(AppError::InvalidInput(tr!(
                "Der Name darf nicht leer sein.",
                "The name can't be empty."
            )));
        }
        sqlx::query("UPDATE games SET title = ? WHERE id = ?")
            .bind(title)
            .bind(game_id)
            .execute(&mut *tx)
            .await?;
    }
    if let Some(favorite) = patch.favorite {
        sqlx::query("UPDATE library_entries SET favorite = ? WHERE game_id = ?")
            .bind(favorite)
            .bind(game_id)
            .execute(&mut *tx)
            .await?;
    }
    if let Some(raw_path) = &patch.executable_path {
        let exe_path = raw_path.trim();
        if exe_path.is_empty() {
            return Err(AppError::InvalidInput(tr!(
                "Bitte gib den Pfad zur ausführbaren Datei an.",
                "Please enter the path to the executable."
            )));
        }
        let install_dir = Path::new(exe_path)
            .parent()
            .map(|p| p.to_string_lossy().into_owned());
        sqlx::query(
            "UPDATE library_entries SET executable_path = ?, install_path = COALESCE(install_path, ?), installed = 1 \
             WHERE game_id = ?",
        )
        .bind(exe_path)
        .bind(install_dir)
        .bind(game_id)
        .execute(&mut *tx)
        .await?;
    }
    if let Some(options) = &patch.launch_options {
        let env: Vec<&EnvVar> = options
            .env
            .iter()
            .filter(|e| !e.key.trim().is_empty())
            .collect();
        if env
            .iter()
            .any(|e| e.key.contains('=') || e.key.chars().any(char::is_whitespace))
        {
            return Err(AppError::InvalidInput(tr!(
                "Variablennamen dürfen weder „=“ noch Leerzeichen enthalten.",
                "Variable names can't contain “=” or spaces."
            )));
        }
        let proton_version = options
            .proton_version
            .as_deref()
            .map(str::trim)
            .filter(|v| !v.is_empty());
        if proton_version
            .is_some_and(|v| v.chars().count() > 100 || v.chars().any(char::is_control))
        {
            return Err(AppError::InvalidInput(tr!(
                "Diese Proton-Version gibt es nicht.",
                "This Proton version does not exist."
            )));
        }
        let tweaks = &options.tweaks;
        let size_ok = |v: Option<u32>| v.is_none_or(|v| (160..=15_360).contains(&v));
        if !size_ok(tweaks.gamescope_width) || !size_ok(tweaks.gamescope_height) {
            return Err(AppError::InvalidInput(tr!(
                "Die Auflösung für gamescope muss zwischen 160 und 15360 Pixeln liegen.",
                "The gamescope resolution must be between 160 and 15360 pixels."
            )));
        }
        sqlx::query(
            "UPDATE library_entries SET launch_arguments = ?, compatibility_tool = ?, env = ?, proton_version = ?, \
             tweaks = ? WHERE game_id = ?",
        )
        .bind(options.arguments.trim())
        .bind(options.compatibility_tool.as_str())
        .bind(serde_json::to_string(&env)?)
        .bind(proton_version)
        .bind(serde_json::to_string(tweaks)?)
        .bind(game_id)
        .execute(&mut *tx)
        .await?;
    }
    tx.commit().await?;
    require(pool, game_id).await
}

/// The Proton build of every game that runs with Proton (`None` = the default).
pub async fn proton_choices(pool: &SqlitePool) -> AppResult<Vec<Option<String>>> {
    let rows: Vec<(Option<String>,)> = sqlx::query_as(
        "SELECT proton_version FROM library_entries WHERE compatibility_tool = 'proton'",
    )
    .fetch_all(pool)
    .await?;
    Ok(rows.into_iter().map(|(v,)| v).collect())
}

/// Inserts a store-owned game (connectors will use this; tests for now).
#[cfg(test)]
pub async fn insert_store_game(
    pool: &SqlitePool,
    game: &Game,
    source: StoreSource,
    store_game_id: Option<&str>,
    install_path: Option<&str>,
) -> AppResult<LibraryItem> {
    add_from_catalog(pool, game).await?;
    sqlx::query(
        "UPDATE library_entries SET source_id = ?, store_game_id = ?, install_path = ?, installed = ? WHERE game_id = ?",
    )
    .bind(source.as_str())
    .bind(store_game_id)
    .bind(install_path)
    .bind(install_path.is_some())
    .bind(&game.id)
    .execute(pool)
    .await?;
    require(pool, &game.id).await
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use crate::db::open_in_memory;

    pub(crate) fn sample_game(id: &str, title: &str) -> Game {
        Game {
            id: id.into(),
            title: title.into(),
            summary: "Kurz.".into(),
            description: "Lang.".into(),
            genres: vec!["puzzle".into()],
            platforms: vec![Platform::Linux, Platform::Windows],
            release_date: Some("2024-05-01".into()),
            developer: "Studio".into(),
            publisher: "Verlag".into(),
            rating: Some(81),
            popularity: 40,
            features: None,
            artwork: GameArtwork {
                cover: ImageRef {
                    url: None,
                    alt: String::new(),
                },
                header: ImageRef {
                    url: None,
                    alt: String::new(),
                },
                screenshots: vec![ImageRef {
                    url: None,
                    alt: String::new(),
                }],
            },
        }
    }

    /// Step 8 target: a library of 2500+ games must load fast enough for a start under 2 s.
    #[tokio::test]
    async fn lists_a_large_library_quickly() {
        let pool = open_in_memory().await.unwrap();
        sqlx::query(
            "WITH RECURSIVE n(i) AS (SELECT 1 UNION ALL SELECT i + 1 FROM n WHERE i < 2500) \
             INSERT INTO games (id, title, release_date, created_at) \
             SELECT 'g' || i, 'Spiel ' || i, '2020-01-01', '2026-01-01T00:00:00Z' FROM n",
        )
        .execute(&pool)
        .await
        .unwrap();
        sqlx::query(
            "INSERT INTO library_entries (game_id, source_id, added_at) \
             SELECT id, 'manual', '2026-01-01T00:00:00Z' FROM games",
        )
        .execute(&pool)
        .await
        .unwrap();
        sqlx::query(
            "WITH RECURSIVE n(i) AS (SELECT 1 UNION ALL SELECT i + 1 FROM n WHERE i < 20000) \
             INSERT INTO play_sessions (game_id, started_at, ended_at, duration_seconds) \
             SELECT 'g' || (i % 2500 + 1), '2026-10-01T10:00:00Z', '2026-10-01T11:00:00Z', 3600 FROM n",
        )
        .execute(&pool)
        .await
        .unwrap();

        let started = std::time::Instant::now();
        let items = list(&pool).await.unwrap();
        let elapsed = started.elapsed();
        assert_eq!(items.len(), 2500);
        assert_eq!(items[0].entry.playtime_minutes, 8 * 60);
        // Release builds take about 60 ms; debug builds are several times slower.
        assert!(elapsed < std::time::Duration::from_secs(2), "{elapsed:?}");
    }

    #[tokio::test]
    async fn adds_games_without_install_once() {
        let pool = open_in_memory().await.unwrap();
        let item = add_not_installed(&pool, "  Hollow Lantern ").await.unwrap();
        assert_eq!(
            (item.game.title.as_str(), item.entry.installed),
            ("Hollow Lantern", false)
        );
        assert_eq!(item.entry.source, StoreSource::Manual);
        assert!(matches!(
            add_not_installed(&pool, "hollow lantern").await,
            Err(AppError::AlreadyInLibrary(_))
        ));
        assert!(matches!(
            add_not_installed(&pool, " ").await,
            Err(AppError::InvalidInput(_))
        ));
    }

    fn manual(title: &str, path: &str) -> ManualGameInput {
        ManualGameInput {
            title: title.into(),
            executable_path: path.into(),
            size_bytes: Some(1234),
        }
    }

    #[tokio::test]
    async fn add_manual_and_list() {
        let pool = open_in_memory().await.unwrap();
        let item = add_manual(&pool, &manual("Ember & Oak", "/games/ember/ember.x86_64"))
            .await
            .unwrap();
        assert_eq!(item.game.id, "manual-ember-and-oak");
        assert_eq!(item.entry.source, StoreSource::Manual);
        assert!(item.entry.installed);
        assert_eq!(item.entry.install_path.as_deref(), Some("/games/ember"));
        assert_eq!(item.entry.install_size_bytes, Some(1234));
        assert_eq!(item.entry.playtime_minutes, 0);
        assert_eq!(item.game.artwork.screenshots.len(), 0);

        add_manual(&pool, &manual("Alpha", "/games/alpha/alpha"))
            .await
            .unwrap();
        let titles: Vec<String> = list(&pool)
            .await
            .unwrap()
            .into_iter()
            .map(|i| i.game.title)
            .collect();
        assert_eq!(titles, ["Alpha", "Ember & Oak"]);
    }

    #[tokio::test]
    async fn rejects_duplicates_and_empty_input() {
        let pool = open_in_memory().await.unwrap();
        add_manual(&pool, &manual("Game", "/g/game")).await.unwrap();
        assert!(matches!(
            add_manual(&pool, &manual("Other", "/g/game")).await,
            Err(AppError::AlreadyInLibrary(_))
        ));
        assert!(matches!(
            add_manual(&pool, &manual("  ", "/g/x")).await,
            Err(AppError::InvalidInput(_))
        ));
        // Same title, different path → unique id
        let second = add_manual(&pool, &manual("Game", "/h/game")).await.unwrap();
        assert_eq!(second.game.id, "manual-game-2");
    }

    #[tokio::test]
    async fn update_fields() {
        let pool = open_in_memory().await.unwrap();
        let item = add_manual(&pool, &manual("Game", "/g/game.exe"))
            .await
            .unwrap();
        let patch = LibraryUpdate {
            title: Some("Renamed".into()),
            favorite: Some(true),
            executable_path: None,
            launch_options: Some(LaunchOptions {
                arguments: " -novid ".into(),
                compatibility_tool: CompatibilityTool::Proton,
                env: vec![
                    EnvVar {
                        key: "DXVK_HUD".into(),
                        value: "fps".into(),
                    },
                    EnvVar {
                        key: String::new(),
                        value: "dropped".into(),
                    },
                ],
                proton_version: Some(" GE-Proton10-25 ".into()),
                tweaks: crate::models::GameTweaks {
                    mangohud: true,
                    gamescope: true,
                    gamescope_width: Some(1280),
                    gamescope_height: Some(720),
                    vkd3d: false,
                    ..crate::models::GameTweaks::default()
                },
            }),
        };
        let updated = update(&pool, &item.game.id, &patch).await.unwrap();
        assert_eq!(updated.game.title, "Renamed");
        assert!(updated.entry.favorite);
        assert_eq!(updated.entry.launch_options.arguments, "-novid");
        assert_eq!(
            updated.entry.launch_options.compatibility_tool,
            CompatibilityTool::Proton
        );
        assert_eq!(updated.entry.launch_options.env.len(), 1);
        assert_eq!(
            updated.entry.launch_options.proton_version.as_deref(),
            Some("GE-Proton10-25")
        );
        assert_eq!(
            proton_choices(&pool).await.unwrap(),
            [Some("GE-Proton10-25".to_owned())]
        );
        let tweaks = &updated.entry.launch_options.tweaks;
        assert!(tweaks.mangohud && tweaks.gamescope && tweaks.dxvk && !tweaks.vkd3d);
        assert_eq!(tweaks.gamescope_width, Some(1280));
        let too_small = LibraryUpdate {
            launch_options: Some(LaunchOptions {
                tweaks: crate::models::GameTweaks {
                    gamescope_width: Some(10),
                    ..crate::models::GameTweaks::default()
                },
                ..LaunchOptions::default()
            }),
            ..LibraryUpdate::default()
        };
        assert!(matches!(
            update(&pool, &item.game.id, &too_small).await,
            Err(AppError::InvalidInput(_))
        ));

        let bad = LibraryUpdate {
            launch_options: Some(LaunchOptions {
                env: vec![EnvVar {
                    key: "A B".into(),
                    value: String::new(),
                }],
                ..LaunchOptions::default()
            }),
            ..LibraryUpdate::default()
        };
        assert!(matches!(
            update(&pool, &item.game.id, &bad).await,
            Err(AppError::InvalidInput(_))
        ));
    }

    #[tokio::test]
    async fn catalog_game_then_set_executable() {
        let pool = open_in_memory().await.unwrap();
        let item = add_from_catalog(&pool, &sample_game("tidewater", "Tidewater"))
            .await
            .unwrap();
        assert!(!item.entry.installed);
        assert_eq!(item.game.genres, ["puzzle"]);
        assert_eq!(item.game.artwork.screenshots.len(), 1);
        assert!(matches!(
            add_from_catalog(&pool, &sample_game("tidewater", "Tidewater")).await,
            Err(AppError::AlreadyInLibrary(_))
        ));

        let patch = LibraryUpdate {
            executable_path: Some("/opt/tide/tide".into()),
            ..LibraryUpdate::default()
        };
        let updated = update(&pool, "tidewater", &patch).await.unwrap();
        assert!(updated.entry.installed);
        assert_eq!(updated.entry.install_path.as_deref(), Some("/opt/tide"));
    }

    #[tokio::test]
    async fn remove_keeps_sessions() {
        let pool = open_in_memory().await.unwrap();
        let item = add_manual(&pool, &manual("Game", "/g/game")).await.unwrap();
        let id = item.game.id.clone();
        let session = crate::db::sessions::start(&pool, &id, "2026-11-01T10:00:00Z")
            .await
            .unwrap();
        crate::db::sessions::update(&pool, session, "2026-11-01T11:00:00Z", 3600)
            .await
            .unwrap();
        assert_eq!(
            require(&pool, &id).await.unwrap().entry.playtime_minutes,
            60
        );

        remove(&pool, &id).await.unwrap();
        assert!(get(&pool, &id).await.unwrap().is_none());
        assert!(matches!(
            remove(&pool, &id).await,
            Err(AppError::NotInLibrary(_))
        ));
        let sessions: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM play_sessions")
            .fetch_one(&pool)
            .await
            .unwrap();
        assert_eq!(sessions, 1);
    }
}

// ───────────── catalogue matching ─────────────

pub async fn set_match_status(
    pool: &SqlitePool,
    game_id: &str,
    status: MatchStatus,
) -> AppResult<()> {
    sqlx::query("UPDATE library_entries SET match_status = ? WHERE game_id = ?")
        .bind(status.as_str())
        .bind(game_id)
        .execute(pool)
        .await?;
    Ok(())
}

/// Stores suggestions for the user and marks the entry as `pending`.
pub async fn store_candidates(
    pool: &SqlitePool,
    game_id: &str,
    candidates: &[MatchCandidate],
) -> AppResult<()> {
    let mut tx = pool.begin().await?;
    sqlx::query("DELETE FROM match_candidates WHERE game_id = ?")
        .bind(game_id)
        .execute(&mut *tx)
        .await?;
    for c in candidates {
        sqlx::query(
            "INSERT INTO match_candidates (game_id, catalog_id, score, game) VALUES (?, ?, ?, ?)",
        )
        .bind(game_id)
        .bind(&c.game.id)
        .bind(c.score)
        .bind(serde_json::to_string(&c.game)?)
        .execute(&mut *tx)
        .await?;
    }
    sqlx::query("UPDATE library_entries SET match_status = 'pending' WHERE game_id = ?")
        .bind(game_id)
        .execute(&mut *tx)
        .await?;
    tx.commit().await?;
    Ok(())
}

pub async fn candidates(pool: &SqlitePool, game_id: &str) -> AppResult<Vec<MatchCandidate>> {
    let rows: Vec<(f64, String)> = sqlx::query_as(
        "SELECT score, game FROM match_candidates WHERE game_id = ? ORDER BY score DESC",
    )
    .bind(game_id)
    .fetch_all(pool)
    .await?;
    rows.into_iter()
        .map(|(score, game)| {
            Ok(MatchCandidate {
                score,
                game: serde_json::from_str(&game)?,
            })
        })
        .collect()
}

/// User declined all suggestions.
pub async fn reject_match(pool: &SqlitePool, game_id: &str) -> AppResult<LibraryItem> {
    require(pool, game_id).await?;
    sqlx::query("DELETE FROM match_candidates WHERE game_id = ?")
        .bind(game_id)
        .execute(pool)
        .await?;
    set_match_status(pool, game_id, MatchStatus::Rejected).await?;
    require(pool, game_id).await
}

/// Copies catalogue metadata + artwork onto a library game and links it.
/// Executable, launch options, playtime and favourites stay untouched.
pub async fn apply_catalog(
    pool: &SqlitePool,
    game_id: &str,
    catalog: &Game,
) -> AppResult<LibraryItem> {
    require(pool, game_id).await?;
    let screenshots: Vec<Option<&str>> = catalog
        .artwork
        .screenshots
        .iter()
        .map(|s| s.url.as_deref())
        .collect();
    let mut tx = pool.begin().await?;
    sqlx::query(
        "UPDATE games SET title = ?, summary = ?, description = ?, genres = ?, platforms = ?, release_date = ?, \
           developer = ?, publisher = ?, rating = ?, popularity = ?, \
           cover_url = COALESCE(?, cover_url), header_url = COALESCE(?, header_url), screenshots = ?, \
           features = ? \
         WHERE id = ?",
    )
    .bind(&catalog.title)
    .bind(&catalog.summary)
    .bind(&catalog.description)
    .bind(serde_json::to_string(&catalog.genres)?)
    .bind(serde_json::to_string(&catalog.platforms)?)
    .bind(catalog.release_date.as_deref().unwrap_or(""))
    .bind(&catalog.developer)
    .bind(&catalog.publisher)
    .bind(catalog.rating)
    .bind(catalog.popularity)
    .bind(&catalog.artwork.cover.url)
    .bind(&catalog.artwork.header.url)
    .bind(serde_json::to_string(&screenshots)?)
    .bind(catalog.features.as_ref().map(serde_json::to_string).transpose()?)
    .bind(game_id)
    .execute(&mut *tx)
    .await?;
    sqlx::query(
        "UPDATE library_entries SET catalog_id = ?, match_status = 'matched' WHERE game_id = ?",
    )
    .bind(&catalog.id)
    .bind(game_id)
    .execute(&mut *tx)
    .await?;
    sqlx::query("DELETE FROM match_candidates WHERE game_id = ?")
        .bind(game_id)
        .execute(&mut *tx)
        .await?;
    tx.commit().await?;
    require(pool, game_id).await
}

#[cfg(test)]
mod match_tests {
    use super::tests::sample_game;
    use super::*;
    use crate::db::open_in_memory;

    async fn manual_game(pool: &SqlitePool) -> String {
        let input = ManualGameInput {
            title: "tidewater".into(),
            executable_path: "/g/tide".into(),
            size_bytes: None,
        };
        add_manual(pool, &input).await.unwrap().game.id
    }

    #[tokio::test]
    async fn candidates_then_link_applies_metadata() {
        let pool = open_in_memory().await.unwrap();
        let id = manual_game(&pool).await;
        assert_eq!(
            require(&pool, &id).await.unwrap().entry.match_status,
            MatchStatus::None
        );

        let mut catalog = sample_game("igdb-77", "Tidewater");
        catalog.artwork.cover.url = Some("https://images.igdb.com/c.jpg".into());
        let other = sample_game("igdb-78", "Tidewater Remastered");
        store_candidates(
            &pool,
            &id,
            &[
                MatchCandidate {
                    game: other,
                    score: 0.8,
                },
                MatchCandidate {
                    game: catalog.clone(),
                    score: 0.97,
                },
            ],
        )
        .await
        .unwrap();
        assert_eq!(
            require(&pool, &id).await.unwrap().entry.match_status,
            MatchStatus::Pending
        );
        let list = candidates(&pool, &id).await.unwrap();
        assert_eq!(list[0].game.id, "igdb-77", "best first");

        let linked = apply_catalog(&pool, &id, &catalog).await.unwrap();
        assert_eq!(linked.game.title, "Tidewater");
        assert_eq!(
            linked.game.artwork.cover.url.as_deref(),
            Some("https://images.igdb.com/c.jpg")
        );
        assert_eq!(linked.game.release_date.as_deref(), Some("2024-05-01"));
        assert_eq!(linked.entry.catalog_id.as_deref(), Some("igdb-77"));
        assert_eq!(linked.entry.match_status, MatchStatus::Matched);
        assert_eq!(
            linked.entry.executable_path.as_deref(),
            Some("/g/tide"),
            "launch data untouched"
        );
        assert_eq!(
            candidates(&pool, &id).await.unwrap(),
            Vec::<MatchCandidate>::new()
        );

        // The catalogue id now finds the library game.
        assert_eq!(get(&pool, "igdb-77").await.unwrap().unwrap().game.id, id);

        // A catalogue entry without artwork keeps the cover the game already has.
        let mut bare = sample_game("igdb-79", "Tidewater");
        bare.artwork.cover.url = None;
        let relinked = apply_catalog(&pool, &id, &bare).await.unwrap();
        assert_eq!(
            relinked.game.artwork.cover.url.as_deref(),
            Some("https://images.igdb.com/c.jpg")
        );
    }

    #[tokio::test]
    async fn reject_clears_candidates() {
        let pool = open_in_memory().await.unwrap();
        let id = manual_game(&pool).await;
        store_candidates(
            &pool,
            &id,
            &[MatchCandidate {
                game: sample_game("igdb-1", "Tide"),
                score: 0.7,
            }],
        )
        .await
        .unwrap();
        let item = reject_match(&pool, &id).await.unwrap();
        assert_eq!(item.entry.match_status, MatchStatus::Rejected);
        assert_eq!(
            candidates(&pool, &id).await.unwrap(),
            Vec::<MatchCandidate>::new()
        );
    }

    #[tokio::test]
    async fn catalog_adds_are_matched_and_unknown_dates_are_null() {
        let pool = open_in_memory().await.unwrap();
        let mut game = sample_game("rawg-5", "Orbit");
        game.release_date = None;
        let item = add_from_catalog(&pool, &game).await.unwrap();
        assert_eq!(item.entry.catalog_id.as_deref(), Some("rawg-5"));
        assert_eq!(item.entry.match_status, MatchStatus::Matched);
        assert_eq!(item.game.release_date, None);
    }
}
