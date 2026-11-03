//! Plugin discovery: every sub folder with a `manifest.json` is a plugin.
//! Enabled state is stored in the settings table (`plugins` key).

use std::collections::HashMap;
use std::path::PathBuf;

use serde::Deserialize;
use sqlx::SqlitePool;

use crate::db::settings::{get_json, set_json};
use crate::error::{AppError, AppResult};
use crate::models::PluginInfo;

#[derive(Debug, Deserialize)]
struct Manifest {
    id: String,
    name: String,
    version: String,
    #[serde(default)]
    author: Option<String>,
    #[serde(default)]
    description: String,
}

pub fn discover(dirs: &[PathBuf], enabled: &HashMap<String, bool>) -> Vec<PluginInfo> {
    let mut plugins: Vec<PluginInfo> = Vec::new();
    for dir in dirs {
        let Ok(entries) = std::fs::read_dir(dir) else {
            continue;
        };
        for entry in entries.flatten() {
            let Ok(raw) = std::fs::read_to_string(entry.path().join("manifest.json")) else {
                continue;
            };
            let Ok(manifest) = serde_json::from_str::<Manifest>(&raw) else {
                continue;
            };
            if plugins.iter().any(|p| p.id == manifest.id) {
                continue;
            }
            plugins.push(PluginInfo {
                enabled: enabled.get(&manifest.id).copied().unwrap_or(true),
                id: manifest.id,
                name: manifest.name,
                version: manifest.version,
                author: manifest.author.unwrap_or_else(|| "Unbekannt".into()),
                description: manifest.description,
            });
        }
    }
    plugins.sort_by_key(|p| p.name.to_lowercase());
    plugins
}

pub async fn list(pool: &SqlitePool, dirs: &[PathBuf]) -> AppResult<Vec<PluginInfo>> {
    let enabled: HashMap<String, bool> = get_json(pool, "plugins").await?.unwrap_or_default();
    Ok(discover(dirs, &enabled))
}

pub async fn set_enabled(
    pool: &SqlitePool,
    dirs: &[PathBuf],
    id: &str,
    enabled: bool,
) -> AppResult<PluginInfo> {
    let mut map: HashMap<String, bool> = get_json(pool, "plugins").await?.unwrap_or_default();
    let mut plugin = discover(dirs, &map)
        .into_iter()
        .find(|p| p.id == id)
        .ok_or_else(|| AppError::InvalidInput(format!("Plugin nicht gefunden: {id}")))?;
    map.insert(id.to_owned(), enabled);
    set_json(pool, "plugins", &map).await?;
    plugin.enabled = enabled;
    Ok(plugin)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn discovers_and_toggles() {
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path().join("hello");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(
            dir.join("manifest.json"),
            r#"{"id":"hello","name":"Hello","version":"1.0.0","entry":"index.js"}"#,
        )
        .unwrap();
        std::fs::create_dir_all(tmp.path().join("broken")).unwrap();
        std::fs::write(tmp.path().join("broken/manifest.json"), "{").unwrap();

        let pool = crate::db::open_in_memory().await.unwrap();
        let dirs = vec![tmp.path().to_path_buf()];
        let plugins = list(&pool, &dirs).await.unwrap();
        assert_eq!(plugins.len(), 1);
        assert!(plugins[0].enabled);
        set_enabled(&pool, &dirs, "hello", false).await.unwrap();
        assert!(!list(&pool, &dirs).await.unwrap()[0].enabled);
        assert!(set_enabled(&pool, &dirs, "nope", true).await.is_err());
    }
}
