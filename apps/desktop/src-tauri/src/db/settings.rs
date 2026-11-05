//! Settings: one JSON document per section in the key/value `settings` table.

use serde::de::DeserializeOwned;
use serde::Serialize;
use sqlx::SqlitePool;

use crate::error::{AppError, AppResult};
use crate::models::AppSettings;

pub async fn get_json<T: DeserializeOwned>(pool: &SqlitePool, key: &str) -> AppResult<Option<T>> {
    let raw: Option<String> = sqlx::query_scalar("SELECT value FROM settings WHERE key = ?")
        .bind(key)
        .fetch_optional(pool)
        .await?;
    // A section that no longer parses (e.g. after a schema change) falls back to defaults.
    Ok(raw.and_then(|r| serde_json::from_str(&r).ok()))
}

pub async fn set_json<T: Serialize + Sync>(
    pool: &SqlitePool,
    key: &str,
    value: &T,
) -> AppResult<()> {
    sqlx::query("INSERT INTO settings (key, value) VALUES (?, ?) ON CONFLICT (key) DO UPDATE SET value = excluded.value")
        .bind(key)
        .bind(serde_json::to_string(value)?)
        .execute(pool)
        .await?;
    Ok(())
}

/// Stored settings, section by section, falling back to `defaults`.
pub async fn load(pool: &SqlitePool, defaults: &AppSettings) -> AppResult<AppSettings> {
    Ok(AppSettings {
        general: get_json(pool, "general")
            .await?
            .unwrap_or_else(|| defaults.general.clone()),
        appearance: get_json(pool, "appearance")
            .await?
            .unwrap_or_else(|| defaults.appearance.clone()),
        library: get_json(pool, "library")
            .await?
            .unwrap_or_else(|| defaults.library.clone()),
        downloads: get_json(pool, "downloads")
            .await?
            .unwrap_or_else(|| defaults.downloads.clone()),
        catalog: get_json(pool, "catalog")
            .await?
            .unwrap_or_else(|| defaults.catalog.clone()),
    })
}

pub fn validate(settings: &AppSettings) -> AppResult<()> {
    let invalid = |m: String| Err(AppError::InvalidInput(m));
    if !["system", "light", "dark"].contains(&settings.appearance.theme.as_str()) {
        return invalid(tr!("Unbekanntes Design.", "Unknown theme."));
    }
    if !["blue", "teal", "sage", "plum", "clay"].contains(&settings.appearance.accent.as_str()) {
        return invalid(tr!("Unbekannte Akzentfarbe.", "Unknown accent colour."));
    }
    if !["de", "en"].contains(&settings.general.language.as_str()) {
        return invalid(tr!("Unbekannte Sprache.", "Unknown language."));
    }
    if !["home", "library"].contains(&settings.general.start_page.as_str()) {
        return invalid(tr!("Unbekannte Startseite.", "Unknown start page."));
    }
    if !(1..=3).contains(&settings.downloads.max_concurrent) {
        return invalid(tr!(
            "Gleichzeitige Downloads: 1 bis 3.",
            "Parallel downloads: 1 to 3."
        ));
    }
    if settings
        .downloads
        .bandwidth_limit_bytes_per_second
        .is_some_and(|b| b <= 0)
    {
        return invalid(tr!(
            "Das Bandbreitenlimit muss größer als 0 sein.",
            "The bandwidth limit must be greater than 0."
        ));
    }
    if settings.downloads.directory.trim().is_empty() {
        return invalid(tr!(
            "Bitte gib einen Zielordner an.",
            "Please enter a target folder."
        ));
    }
    if settings
        .library
        .scan_folders
        .iter()
        .any(|f| f.trim().is_empty())
    {
        return invalid(tr!(
            "Leere Scan-Ordner sind nicht erlaubt.",
            "Empty scan folders aren't allowed."
        ));
    }
    Ok(())
}

pub async fn save(pool: &SqlitePool, settings: &AppSettings) -> AppResult<()> {
    validate(settings)?;
    let mut library = settings.library.clone();
    library.scan_folders.dedup();
    set_json(pool, "general", &settings.general).await?;
    set_json(pool, "appearance", &settings.appearance).await?;
    set_json(pool, "library", &library).await?;
    set_json(pool, "downloads", &settings.downloads).await?;
    set_json(pool, "catalog", &settings.catalog).await?;
    Ok(())
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use crate::db::open_in_memory;
    use crate::models::{
        AppearanceSettings, CatalogSettings, DownloadSettings, GeneralSettings, LibrarySettings,
    };

    pub(crate) fn defaults() -> AppSettings {
        AppSettings {
            general: GeneralSettings {
                language: "de".into(),
                launch_on_startup: false,
                minimize_to_tray: true,
                start_page: "home".into(),
            },
            appearance: AppearanceSettings {
                theme: "system".into(),
                accent: "blue".into(),
            },
            library: LibrarySettings {
                scan_folders: vec![],
                scan_on_startup: true,
            },
            downloads: DownloadSettings {
                directory: "/home/u/Games".into(),
                bandwidth_limit_bytes_per_second: None,
                max_concurrent: 1,
            },
            catalog: CatalogSettings {
                use_cove_service: true,
            },
        }
    }

    #[tokio::test]
    async fn defaults_then_roundtrip() {
        let pool = open_in_memory().await.unwrap();
        assert_eq!(load(&pool, &defaults()).await.unwrap(), defaults());

        let mut changed = defaults();
        changed.appearance.theme = "dark".into();
        changed.library.scan_folders = vec!["/a".into(), "/b".into()];
        changed.downloads.directory = "/mnt/games".into();
        save(&pool, &changed).await.unwrap();
        assert_eq!(load(&pool, &defaults()).await.unwrap(), changed);
    }

    #[tokio::test]
    async fn rejects_invalid_values() {
        let pool = open_in_memory().await.unwrap();
        let mut bad = defaults();
        bad.appearance.theme = "neon".into();
        assert!(matches!(
            save(&pool, &bad).await,
            Err(AppError::InvalidInput(_))
        ));
        let mut bad = defaults();
        bad.downloads.max_concurrent = 9;
        assert!(save(&pool, &bad).await.is_err());
    }

    #[tokio::test]
    async fn corrupt_section_falls_back_to_default() {
        let pool = open_in_memory().await.unwrap();
        sqlx::query("INSERT INTO settings VALUES ('appearance', '{\"theme\": 42}')")
            .execute(&pool)
            .await
            .unwrap();
        assert_eq!(
            load(&pool, &defaults()).await.unwrap().appearance,
            defaults().appearance
        );
    }
}
