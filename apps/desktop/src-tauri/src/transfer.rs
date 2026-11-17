//! Export and import of the user's Cove data, for a new computer or as a backup.
//!
//! The file is JSON: the library (games with their settings and the playtime
//! the stores reported), play sessions, collections, the wishlist and the
//! settings. Store logins are never part of it (they live in the keyring).
//!
//! An import merges and never deletes. Store games are matched by store and
//! store id; ones that aren't here yet come back with the store's next sync,
//! so they are skipped. Manual games are matched by their start file, else
//! added. Sessions are added unless one of the same game starts at the same
//! moment. Collections are matched by name, wishlist games by id. Settings are
//! only taken when asked, and the folders of this computer stay.

use std::collections::HashMap;
use std::path::Path;

use serde::{Deserialize, Serialize};
use sqlx::SqlitePool;

use crate::db;
use crate::error::{AppError, AppResult};
use crate::models::{
    now_iso, AppSettings, Game, ImportPreview, ImportResult, LaunchOptions, LibraryUpdate,
    StoreSource,
};

const FORMAT: &str = "cove-export";
const VERSION: u32 = 1;
/// Bigger files are not a Cove export.
const MAX_BYTES: u64 = 64 * 1024 * 1024;

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ExportFile {
    format: String,
    version: u32,
    exported_at: String,
    cove_version: String,
    settings: serde_json::Value,
    games: Vec<ExportedGame>,
    sessions: Vec<ExportedSession>,
    collections: Vec<ExportedCollection>,
    wishlist: Vec<ExportedWish>,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ExportedGame {
    game: Game,
    source: StoreSource,
    store_game_id: Option<String>,
    executable_path: Option<String>,
    install_path: Option<String>,
    favorite: bool,
    /// Playtime the stores reported (Cove's own sessions are in `sessions`).
    playtime_offset_minutes: i64,
    launch_options: LaunchOptions,
    added_at: String,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ExportedSession {
    game_id: String,
    started_at: String,
    ended_at: Option<String>,
    duration_seconds: i64,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ExportedCollection {
    name: String,
    game_ids: Vec<String>,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ExportedWish {
    game: Game,
    added_at: String,
    target_price: Option<f64>,
    notify_on_sale: bool,
}

/// Everything worth carrying to another computer.
pub async fn export(pool: &SqlitePool, settings: &AppSettings) -> AppResult<ExportFile> {
    let library = db::library::list(pool).await?;
    let offsets: HashMap<String, i64> = sqlx::query_as::<_, (String, i64)>(
        "SELECT game_id, playtime_offset_minutes FROM library_entries",
    )
    .fetch_all(pool)
    .await?
    .into_iter()
    .collect();
    let games = library
        .into_iter()
        .map(|item| ExportedGame {
            playtime_offset_minutes: offsets.get(&item.game.id).copied().unwrap_or(0),
            source: item.entry.source,
            store_game_id: item.entry.store_game_id,
            executable_path: item.entry.executable_path,
            install_path: item.entry.install_path,
            favorite: item.entry.favorite,
            launch_options: item.entry.launch_options,
            added_at: item.entry.added_at,
            game: item.game,
        })
        .collect::<Vec<_>>();
    let sessions = sqlx::query_as::<_, (String, String, Option<String>, i64)>(
        "SELECT s.game_id, s.started_at, s.ended_at, s.duration_seconds FROM play_sessions s \
         JOIN library_entries e ON e.game_id = s.game_id WHERE s.duration_seconds > 0 ORDER BY s.started_at",
    )
    .fetch_all(pool)
    .await?
    .into_iter()
    .map(|(game_id, started_at, ended_at, duration_seconds)| ExportedSession {
        game_id,
        started_at,
        ended_at,
        duration_seconds,
    })
    .collect();
    let collections = db::collections::list(pool)
        .await?
        .into_iter()
        .map(|c| ExportedCollection {
            name: c.name,
            game_ids: c.game_ids,
        })
        .collect();
    let wishlist = db::wishlist::list(pool)
        .await?
        .into_iter()
        .map(|w| ExportedWish {
            game: w.game,
            added_at: w.added_at,
            target_price: w.target_price,
            notify_on_sale: w.notify_on_sale,
        })
        .collect();
    Ok(ExportFile {
        format: FORMAT.into(),
        version: VERSION,
        exported_at: now_iso(),
        cove_version: env!("CARGO_PKG_VERSION").into(),
        settings: serde_json::to_value(settings)?,
        games,
        sessions,
        collections,
        wishlist,
    })
}

pub async fn write(path: &Path, file: &ExportFile) -> AppResult<()> {
    let json = serde_json::to_vec_pretty(file)?;
    // Write next to it first: a failed write never leaves half a file.
    let partial = path.with_extension("json.part");
    tokio::fs::write(&partial, json).await?;
    tokio::fs::rename(&partial, path).await?;
    Ok(())
}

fn not_an_export() -> AppError {
    AppError::InvalidInput(tr!(
        "Diese Datei ist kein Cove-Export.",
        "This file isn't a Cove export."
    ))
}

pub async fn read(path: &Path) -> AppResult<ExportFile> {
    let size = tokio::fs::metadata(path).await?.len();
    if size > MAX_BYTES {
        return Err(not_an_export());
    }
    let bytes = tokio::fs::read(path).await?;
    let file: ExportFile = serde_json::from_slice(&bytes).map_err(|_| not_an_export())?;
    if file.format != FORMAT {
        return Err(not_an_export());
    }
    if file.version > VERSION {
        return Err(AppError::InvalidInput(tr!(
            "Dieser Export stammt aus einer neueren Cove-Version. Aktualisiere Cove und versuch es noch einmal.",
            "This export comes from a newer Cove version. Update Cove and try again."
        )));
    }
    Ok(file)
}

pub fn preview(path: &Path, file: &ExportFile) -> ImportPreview {
    ImportPreview {
        path: path.to_string_lossy().into_owned(),
        exported_at: file.exported_at.clone(),
        cove_version: file.cove_version.clone(),
        games: file.games.len(),
        sessions: file.sessions.len(),
        collections: file.collections.len(),
        wishlist: file.wishlist.len(),
    }
}

/// The local game for an exported one, if there is one.
async fn local_game(pool: &SqlitePool, game: &ExportedGame) -> AppResult<Option<String>> {
    if game.source == StoreSource::Manual {
        if let Some(exe) = &game.executable_path {
            return Ok(sqlx::query_scalar(
                "SELECT game_id FROM library_entries WHERE source_id = 'manual' AND executable_path = ?",
            )
            .bind(exe)
            .fetch_optional(pool)
            .await?);
        }
        return Ok(sqlx::query_scalar(
            "SELECT e.game_id FROM library_entries e JOIN games g ON g.id = e.game_id \
             WHERE e.source_id = 'manual' AND e.executable_path IS NULL AND g.title = ?",
        )
        .bind(&game.game.title)
        .fetch_optional(pool)
        .await?);
    }
    let Some(store_id) = &game.store_game_id else {
        return Ok(None);
    };
    Ok(sqlx::query_scalar(
        "SELECT game_id FROM library_entries WHERE source_id = ? AND store_game_id = ?",
    )
    .bind(game.source.as_str())
    .bind(store_id)
    .fetch_optional(pool)
    .await?)
}

/// A manual game from the export (under a new id when its own is taken here).
async fn add_manual(pool: &SqlitePool, exported: &ExportedGame) -> AppResult<String> {
    let mut game = exported.game.clone();
    game.id = db::library::unique_game_id(pool, &db::slugify(&game.title)).await?;
    let installed = exported
        .executable_path
        .as_deref()
        .is_some_and(|p| Path::new(p).is_file());
    let mut tx = pool.begin().await?;
    db::library::upsert_game(&mut tx, &game).await?;
    sqlx::query(
        "INSERT INTO library_entries (game_id, source_id, installed, install_path, executable_path, added_at) \
         VALUES (?, 'manual', ?, ?, ?, ?)",
    )
    .bind(&game.id)
    .bind(installed)
    .bind(&exported.install_path)
    .bind(&exported.executable_path)
    .bind(&exported.added_at)
    .execute(&mut *tx)
    .await?;
    tx.commit().await?;
    Ok(game.id)
}

/// Favourite, store playtime and launch options of an exported game onto a local one.
async fn merge_game(pool: &SqlitePool, game_id: &str, exported: &ExportedGame) -> AppResult<()> {
    sqlx::query(
        "UPDATE library_entries SET favorite = MAX(favorite, ?), \
         playtime_offset_minutes = MAX(playtime_offset_minutes, ?) WHERE game_id = ?",
    )
    .bind(exported.favorite)
    .bind(exported.playtime_offset_minutes.max(0))
    .bind(game_id)
    .execute(pool)
    .await?;
    // Own launch options win over Cove's defaults, never over choices made here.
    let local = db::library::require(pool, game_id).await?;
    if local.entry.launch_options == LaunchOptions::default()
        && exported.launch_options != LaunchOptions::default()
    {
        let patch = LibraryUpdate {
            launch_options: Some(exported.launch_options.clone()),
            ..LibraryUpdate::default()
        };
        if let Err(error) = db::library::update(pool, game_id, &patch).await {
            eprintln!("[transfer] launch options of {game_id}: {error}");
        }
    }
    Ok(())
}

/// The settings of the export with this computer's folders.
fn merged_settings(local: &AppSettings, exported: &serde_json::Value) -> Option<AppSettings> {
    let mut settings: AppSettings = serde_json::from_value(exported.clone()).ok()?;
    settings
        .downloads
        .directory
        .clone_from(&local.downloads.directory);
    settings
        .library
        .game_folders
        .clone_from(&local.library.game_folders);
    settings
        .backups
        .directory
        .clone_from(&local.backups.directory);
    settings
        .emulators
        .rom_folders
        .clone_from(&local.emulators.rom_folders);
    Some(settings)
}

pub async fn import(
    pool: &SqlitePool,
    file: &ExportFile,
    local_settings: &AppSettings,
    with_settings: bool,
) -> AppResult<ImportResult> {
    let mut result = ImportResult::default();
    // Exported game id → local game id.
    let mut ids: HashMap<&str, String> = HashMap::new();
    for exported in &file.games {
        let local = match local_game(pool, exported).await? {
            Some(id) => {
                result.games_updated += 1;
                id
            }
            None if exported.source == StoreSource::Manual => {
                result.games_added += 1;
                add_manual(pool, exported).await?
            }
            None => {
                result.games_skipped += 1;
                continue;
            }
        };
        merge_game(pool, &local, exported).await?;
        ids.insert(exported.game.id.as_str(), local);
    }

    for session in &file.sessions {
        let Some(game_id) = ids.get(session.game_id.as_str()) else {
            continue;
        };
        let exists: Option<i64> =
            sqlx::query_scalar("SELECT 1 FROM play_sessions WHERE game_id = ? AND started_at = ?")
                .bind(game_id)
                .bind(&session.started_at)
                .fetch_optional(pool)
                .await?;
        if exists.is_some() || session.duration_seconds <= 0 {
            continue;
        }
        sqlx::query(
            "INSERT INTO play_sessions (game_id, started_at, ended_at, duration_seconds) VALUES (?, ?, ?, ?)",
        )
        .bind(game_id)
        .bind(&session.started_at)
        .bind(session.ended_at.as_deref().unwrap_or(&session.started_at))
        .bind(session.duration_seconds)
        .execute(pool)
        .await?;
        result.sessions_added += 1;
    }

    let mut collections = db::collections::list(pool).await?;
    for exported in &file.collections {
        let wanted = exported.name.trim().to_lowercase();
        let existing = collections.iter().find(|c| c.name.to_lowercase() == wanted);
        let id = if let Some(collection) = existing {
            collection.id.clone()
        } else {
            match db::collections::create(pool, &exported.name).await {
                Ok(created) => {
                    result.collections_added += 1;
                    let id = created.id.clone();
                    collections.push(created);
                    id
                }
                // An invalid name or the limit: the collection is left out.
                Err(error) => {
                    eprintln!("[transfer] collection {}: {error}", exported.name);
                    continue;
                }
            }
        };
        for game_id in exported.game_ids.iter().filter_map(|g| ids.get(g.as_str())) {
            db::collections::set_game(pool, &id, game_id, true).await?;
        }
    }

    let library: std::collections::HashSet<String> = db::library::list(pool)
        .await?
        .into_iter()
        .map(|item| item.game.id)
        .collect();
    for wish in &file.wishlist {
        if library.contains(&wish.game.id)
            || db::wishlist::get(pool, &wish.game.id).await?.is_some()
        {
            continue;
        }
        if db::wishlist::add(pool, &wish.game).await.is_ok() {
            let _ =
                db::wishlist::update(pool, &wish.game.id, wish.target_price, wish.notify_on_sale)
                    .await;
            result.wishlist_added += 1;
        }
    }

    if with_settings {
        let settings = merged_settings(local_settings, &file.settings).ok_or_else(not_an_export)?;
        db::settings::save(pool, &settings).await?;
        result.settings = true;
    }
    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::library::tests::sample_game;
    use crate::models::{CompatibilityTool, ManualGameInput};

    async fn manual(pool: &SqlitePool, dir: &Path, title: &str) -> String {
        let exe = dir.join(format!("{title}.sh"));
        std::fs::write(&exe, b"#!/bin/sh\n").unwrap();
        db::library::add_manual(
            pool,
            &ManualGameInput {
                title: title.into(),
                executable_path: exe.to_string_lossy().into_owned(),
                size_bytes: None,
            },
        )
        .await
        .unwrap()
        .game
        .id
    }

    #[tokio::test]
    #[allow(clippy::too_many_lines)] // one export carried through a whole import
    async fn an_export_moves_library_playtime_collections_wishlist_and_settings() {
        let tmp = tempfile::tempdir().unwrap();
        let defaults = crate::state::default_settings(Some(&tmp.path().to_path_buf()));

        // The old computer.
        let old = db::open_in_memory().await.unwrap();
        let ember = manual(&old, tmp.path(), "Ember").await;
        db::library::insert_store_game(
            &old,
            &sample_game("steam-620", "Portal 2"),
            StoreSource::Steam,
            Some("620"),
            None,
        )
        .await
        .unwrap();
        db::library::insert_store_game(
            &old,
            &sample_game("gog-9", "Only Old"),
            StoreSource::Gog,
            Some("9"),
            None,
        )
        .await
        .unwrap();
        let mut favourite = LibraryUpdate {
            favorite: Some(true),
            ..LibraryUpdate::default()
        };
        favourite.launch_options = Some(LaunchOptions {
            arguments: "-windowed".into(),
            compatibility_tool: CompatibilityTool::Native,
            ..LaunchOptions::default()
        });
        db::library::update(&old, &ember, &favourite).await.unwrap();
        for (game, start, seconds) in [
            (ember.as_str(), "2026-11-01T10:00:00Z", 3600),
            ("steam-620", "2026-11-02T10:00:00Z", 1800),
        ] {
            sqlx::query("INSERT INTO play_sessions (game_id, started_at, ended_at, duration_seconds) VALUES (?, ?, ?, ?)")
                .bind(game)
                .bind(start)
                .bind(start)
                .bind(seconds)
                .execute(&old)
                .await
                .unwrap();
        }
        let friends = db::collections::create(&old, "Mit Freunden").await.unwrap();
        db::collections::set_game(&old, &friends.id, "steam-620", true)
            .await
            .unwrap();
        db::collections::set_game(&old, &friends.id, &ember, true)
            .await
            .unwrap();
        db::wishlist::add(&old, &sample_game("igdb-77", "Hollow Peak"))
            .await
            .unwrap();
        db::wishlist::update(&old, "igdb-77", Some(9.99), false)
            .await
            .unwrap();
        let mut settings = defaults.clone();
        settings.general.language = "en".into();
        settings.downloads.directory = "/old/computer/Games".into();

        let file = export(&old, &settings).await.unwrap();
        let file_path = tmp.path().join("cove-export.json");
        write(&file_path, &file).await.unwrap();
        let file = read(&file_path).await.unwrap();
        let shown = preview(&file_path, &file);
        assert_eq!(
            (
                shown.games,
                shown.sessions,
                shown.collections,
                shown.wishlist
            ),
            (3, 2, 1, 1)
        );

        // The new computer: Portal 2 came with the Steam login, the rest not yet.
        let new = db::open_in_memory().await.unwrap();
        db::library::insert_store_game(
            &new,
            &sample_game("steam-620", "Portal 2"),
            StoreSource::Steam,
            Some("620"),
            None,
        )
        .await
        .unwrap();
        let result = import(&new, &file, &defaults, true).await.unwrap();
        assert_eq!(
            result,
            ImportResult {
                games_added: 1,
                games_updated: 1,
                games_skipped: 1,
                sessions_added: 2,
                collections_added: 1,
                wishlist_added: 1,
                settings: true,
            }
        );
        let library = db::library::list(&new).await.unwrap();
        let imported = library.iter().find(|i| i.game.title == "Ember").unwrap();
        assert!(imported.entry.favorite);
        assert_eq!(imported.entry.launch_options.arguments, "-windowed");
        assert_eq!(imported.entry.playtime_minutes, 60);
        let portal = library.iter().find(|i| i.game.id == "steam-620").unwrap();
        assert_eq!(portal.entry.playtime_minutes, 30);
        let collections = db::collections::list(&new).await.unwrap();
        assert_eq!(collections[0].name, "Mit Freunden");
        assert_eq!(collections[0].game_ids.len(), 2);
        let wish = db::wishlist::get(&new, "igdb-77").await.unwrap().unwrap();
        assert_eq!(
            (wish.target_price, wish.notify_on_sale),
            (Some(9.99), false)
        );
        let loaded = db::settings::load(&new, &defaults).await.unwrap();
        assert_eq!(loaded.general.language, "en");
        assert_eq!(loaded.downloads.directory, defaults.downloads.directory);

        // A second import adds nothing.
        let again = import(&new, &file, &defaults, false).await.unwrap();
        assert_eq!(
            (
                again.games_added,
                again.sessions_added,
                again.collections_added,
                again.wishlist_added
            ),
            (0, 0, 0, 0)
        );
    }

    #[tokio::test]
    async fn other_files_are_refused() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("other.json");
        std::fs::write(&path, br#"{"hello":"world"}"#).unwrap();
        assert!(matches!(read(&path).await, Err(AppError::InvalidInput(_))));
        std::fs::write(
            &path,
            br#"{"format":"cove-export","version":99,"exportedAt":"","coveVersion":"9","settings":{},"games":[],"sessions":[],"collections":[],"wishlist":[]}"#,
        )
        .unwrap();
        assert!(matches!(read(&path).await, Err(AppError::InvalidInput(_))));
    }
}
