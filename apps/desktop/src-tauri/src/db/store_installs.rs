//! Store games Cove installed itself: build, platform and installed files.

use sqlx::{FromRow, SqlitePool};

use crate::error::AppResult;
use crate::models::{now_iso, StoreSource};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StoreInstall {
    pub game_id: String,
    pub store: StoreSource,
    pub product_id: String,
    pub build_id: String,
    pub version: String,
    pub platform: String,
    pub files: Vec<String>,
    /// Store-specific start data (JSON; Epic: `downloads::epic::EpicMeta`).
    pub meta: String,
}

#[derive(FromRow)]
struct Row {
    game_id: String,
    store: String,
    product_id: String,
    build_id: String,
    version: String,
    platform: String,
    files: String,
    meta: String,
}

/// A DLC installed into the game's folder, with its own build and files.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InstalledDlc {
    pub app_name: String,
    pub title: String,
    pub build_id: String,
    pub files: Vec<String>,
}

pub async fn dlcs(pool: &SqlitePool, game_id: &str) -> AppResult<Vec<InstalledDlc>> {
    let rows: Vec<(String, String, String, String)> = sqlx::query_as(
        "SELECT app_name, title, build_id, files FROM store_install_dlcs WHERE game_id = ? ORDER BY title",
    )
    .bind(game_id)
    .fetch_all(pool)
    .await?;
    rows.into_iter()
        .map(|(app_name, title, build_id, files)| {
            Ok(InstalledDlc {
                app_name,
                title,
                build_id,
                files: serde_json::from_str(&files)?,
            })
        })
        .collect()
}

/// Replaces the game's DLC list (DLCs no longer owned drop out).
pub async fn save_dlcs(pool: &SqlitePool, game_id: &str, dlcs: &[InstalledDlc]) -> AppResult<()> {
    let mut tx = pool.begin().await?;
    sqlx::query("DELETE FROM store_install_dlcs WHERE game_id = ?")
        .bind(game_id)
        .execute(&mut *tx)
        .await?;
    for dlc in dlcs {
        sqlx::query(
            "INSERT INTO store_install_dlcs (game_id, app_name, title, build_id, files, installed_at) \
             VALUES (?, ?, ?, ?, ?, ?)",
        )
        .bind(game_id)
        .bind(&dlc.app_name)
        .bind(&dlc.title)
        .bind(&dlc.build_id)
        .bind(serde_json::to_string(&dlc.files)?)
        .bind(now_iso())
        .execute(&mut *tx)
        .await?;
    }
    tx.commit().await?;
    Ok(())
}

/// DLCs the user left out for this game (GOG product ids or Epic app names).
pub async fn excluded_dlcs(
    pool: &SqlitePool,
    game_id: &str,
) -> AppResult<std::collections::HashSet<String>> {
    let rows: Vec<(String,)> =
        sqlx::query_as("SELECT dlc_id FROM store_dlc_choices WHERE game_id = ?")
            .bind(game_id)
            .fetch_all(pool)
            .await?;
    Ok(rows.into_iter().map(|(id,)| id).collect())
}

/// Remembers the install dialog's DLC choice (replaces the previous one).
pub async fn set_excluded_dlcs(pool: &SqlitePool, game_id: &str, ids: &[String]) -> AppResult<()> {
    let mut tx = pool.begin().await?;
    sqlx::query("DELETE FROM store_dlc_choices WHERE game_id = ?")
        .bind(game_id)
        .execute(&mut *tx)
        .await?;
    for id in ids {
        sqlx::query("INSERT OR IGNORE INTO store_dlc_choices (game_id, dlc_id) VALUES (?, ?)")
            .bind(game_id)
            .bind(id)
            .execute(&mut *tx)
            .await?;
    }
    tx.commit().await?;
    Ok(())
}

pub async fn get(pool: &SqlitePool, game_id: &str) -> AppResult<Option<StoreInstall>> {
    let row: Option<Row> = sqlx::query_as(
        "SELECT game_id, store, product_id, build_id, version, platform, files, meta FROM store_installs WHERE game_id = ?",
    )
    .bind(game_id)
    .fetch_optional(pool)
    .await?;
    row.map(|r| {
        Ok(StoreInstall {
            store: StoreSource::parse(&r.store).unwrap_or(StoreSource::Gog),
            files: serde_json::from_str(&r.files)?,
            game_id: r.game_id,
            product_id: r.product_id,
            build_id: r.build_id,
            version: r.version,
            platform: r.platform,
            meta: r.meta,
        })
    })
    .transpose()
}

pub async fn save(pool: &SqlitePool, install: &StoreInstall) -> AppResult<()> {
    sqlx::query(
        "INSERT INTO store_installs (game_id, store, product_id, build_id, version, platform, files, meta, installed_at) \
         VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?) \
         ON CONFLICT (game_id) DO UPDATE SET store = excluded.store, product_id = excluded.product_id, \
           build_id = excluded.build_id, version = excluded.version, platform = excluded.platform, \
           files = excluded.files, meta = excluded.meta, installed_at = excluded.installed_at",
    )
    .bind(&install.game_id)
    .bind(install.store.as_str())
    .bind(&install.product_id)
    .bind(&install.build_id)
    .bind(&install.version)
    .bind(&install.platform)
    .bind(serde_json::to_string(&install.files)?)
    .bind(&install.meta)
    .bind(now_iso())
    .execute(pool)
    .await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::open_in_memory;
    use crate::db::store_games::{upsert, StoreGameInput};

    #[tokio::test]
    async fn saves_replaces_and_forgets_on_uninstall() {
        let pool = open_in_memory().await.unwrap();
        upsert(
            &pool,
            StoreSource::Gog,
            &StoreGameInput {
                store_game_id: "100".into(),
                title: "Saltmarsh".into(),
                ..StoreGameInput::default()
            },
        )
        .await
        .unwrap();
        let mut install = StoreInstall {
            game_id: "gog-100".into(),
            store: StoreSource::Gog,
            product_id: "100".into(),
            build_id: "8".into(),
            version: "1.0".into(),
            platform: "windows".into(),
            files: vec!["bin/game.exe".into()],
            meta: "{}".into(),
        };
        save(&pool, &install).await.unwrap();
        install.build_id = "9".into();
        save(&pool, &install).await.unwrap();
        assert_eq!(get(&pool, "gog-100").await.unwrap(), Some(install));
        let island = InstalledDlc {
            app_name: "Island".into(),
            title: "Island".into(),
            build_id: "3".into(),
            files: vec!["Paks/island.pak".into()],
        };
        let skin = InstalledDlc {
            app_name: "Skin".into(),
            title: "Golden Skin".into(),
            build_id: "1".into(),
            files: vec![],
        };
        save_dlcs(&pool, "gog-100", &[island.clone(), skin])
            .await
            .unwrap();
        save_dlcs(&pool, "gog-100", std::slice::from_ref(&island))
            .await
            .unwrap();
        assert_eq!(dlcs(&pool, "gog-100").await.unwrap(), vec![island]);
        crate::db::library::mark_uninstalled(&pool, "gog-100")
            .await
            .unwrap();
        assert_eq!(get(&pool, "gog-100").await.unwrap(), None);
        assert_eq!(dlcs(&pool, "gog-100").await.unwrap(), Vec::new());
        // The DLC choice stays for a reinstall.
        set_excluded_dlcs(&pool, "gog-100", &["7".into(), "8".into()])
            .await
            .unwrap();
        set_excluded_dlcs(&pool, "gog-100", &["8".into()])
            .await
            .unwrap();
        assert_eq!(
            excluded_dlcs(&pool, "gog-100").await.unwrap(),
            std::collections::HashSet::from(["8".to_owned()])
        );
    }
}
