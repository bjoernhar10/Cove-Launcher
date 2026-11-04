//! Plugins: folders with a `plugin.json` and ES module files. They run in a
//! Web Worker in the UI; the core decides which plugins may run (user
//! approval of the requested permissions), serves their files through
//! `cove-plugin://` and re-checks permissions for every data-changing call.

use std::collections::{BTreeSet, HashMap};
use std::path::{Path, PathBuf};

use serde::Deserialize;
use serde_json::Value;
use sqlx::SqlitePool;

use crate::cove_achievements::{Definition, Rule};
use crate::downloads::extract::safe_relative;
use crate::error::{AppError, AppResult};
use crate::models::{now_iso, PluginInfo, PluginManifestPreview, PluginPermission, PluginStatus};

pub const MANIFEST: &str = "plugin.json";
pub const API_VERSION: u32 = 1;
pub const SCHEME: &str = "cove-plugin";
const MAX_INSTALL_BYTES: u64 = 20 * 1024 * 1024;
const MAX_INSTALL_FILES: usize = 1000;
const MAX_VALUE_BYTES: usize = 64 * 1024;
const MAX_STORAGE_BYTES: i64 = 1024 * 1024;
const MAX_KEYS: i64 = 1000;
const MAX_ACHIEVEMENTS: usize = 50;

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
struct RawManifest {
    id: String,
    name: String,
    version: String,
    #[serde(default)]
    author: Option<String>,
    #[serde(default)]
    description: Option<String>,
    #[serde(default)]
    api_version: Option<u32>,
    #[serde(default)]
    permissions: Vec<PluginPermission>,
    entry: String,
}

/// A validated `plugin.json`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Manifest {
    pub id: String,
    pub name: String,
    pub version: String,
    pub author: String,
    pub description: String,
    pub permissions: Vec<PluginPermission>,
    /// Relative path with forward slashes.
    pub entry: String,
}

fn valid_id(id: &str) -> bool {
    let mut chars = id.chars();
    chars
        .next()
        .is_some_and(|c| c.is_ascii_lowercase() || c.is_ascii_digit())
        && id.len() <= 64
        && id
            .chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-')
}

fn valid_version(version: &str) -> bool {
    let core = version.split(['-', '+']).next().unwrap_or("");
    let parts: Vec<&str> = core.split('.').collect();
    parts.len() == 3
        && parts
            .iter()
            .all(|p| !p.is_empty() && p.chars().all(|c| c.is_ascii_digit()))
}

/// Reads and checks `<dir>/plugin.json`. Errors are user-facing (German).
pub fn read_manifest(dir: &Path) -> Result<Manifest, String> {
    let raw = std::fs::read_to_string(dir.join(MANIFEST))
        .map_err(|_| format!("{MANIFEST} fehlt oder ist nicht lesbar."))?;
    let raw: RawManifest = serde_json::from_str(&raw).map_err(|e| {
        if e.to_string().contains("unknown variant") {
            format!("{MANIFEST} enthält eine unbekannte Berechtigung ({e}).")
        } else {
            format!("{MANIFEST} ist ungültig ({e}).")
        }
    })?;
    if !valid_id(&raw.id) {
        return Err(
            "Die Plugin-ID darf nur Kleinbuchstaben, Ziffern und Bindestriche enthalten.".into(),
        );
    }
    let name = raw.name.trim();
    if name.is_empty() || name.chars().count() > 60 {
        return Err("Der Name fehlt oder ist länger als 60 Zeichen.".into());
    }
    if !valid_version(&raw.version) {
        return Err("Die Version muss die Form 1.2.3 haben.".into());
    }
    let api = raw.api_version.unwrap_or(API_VERSION);
    if api != API_VERSION {
        return Err(format!(
            "Das Plugin braucht die Plugin-API {api}; diese Cove-Version bietet {API_VERSION}."
        ));
    }
    let entry = safe_relative(&raw.entry)
        .filter(|p| p.extension().is_some_and(|e| e == "js" || e == "mjs"))
        .ok_or_else(|| {
            "„entry“ muss eine .js- oder .mjs-Datei im Plugin-Ordner sein.".to_owned()
        })?;
    if !dir.join(&entry).is_file() {
        return Err(format!("Die Einstiegsdatei {} fehlt.", entry.display()));
    }
    let permissions: BTreeSet<PluginPermission> = raw.permissions.into_iter().collect();
    Ok(Manifest {
        id: raw.id,
        name: name.to_owned(),
        version: raw.version,
        author: raw
            .author
            .filter(|a| !a.trim().is_empty())
            .unwrap_or_else(|| "Unbekannt".into()),
        description: raw.description.unwrap_or_default(),
        permissions: permissions.into_iter().collect(),
        entry: entry.to_string_lossy().replace('\\', "/"),
    })
}

/// URL the worker imports the entry module from. `token` is secret per plugin
/// and app start, so a plugin cannot load another plugin's files.
pub fn entry_url(token: &str, entry: &str) -> String {
    if cfg!(windows) {
        format!("http://{SCHEME}.localhost/{token}/{entry}")
    } else {
        format!("{SCHEME}://localhost/{token}/{entry}")
    }
}

#[derive(Debug, Clone)]
struct Found {
    /// Manifest id, or the folder name when the manifest is broken.
    id: String,
    dir: PathBuf,
    manifest: Result<Manifest, String>,
    removable: bool,
}

#[derive(Debug, Clone, sqlx::FromRow)]
struct Row {
    id: String,
    enabled: bool,
    granted: String,
}

pub struct Plugins {
    pool: SqlitePool,
    /// Plugin id → random path token for `cove-plugin://` (new on every start).
    tokens: std::sync::Mutex<HashMap<String, String>>,
    /// Cove's own plugin folder (installs land here).
    user_dir: PathBuf,
    /// Extra folders (development: the repository's `plugins/`).
    extra_dirs: Vec<PathBuf>,
}

fn not_found(id: &str) -> AppError {
    AppError::InvalidInput(format!("Plugin nicht gefunden: {id}"))
}

fn label(permission: PluginPermission) -> &'static str {
    match permission {
        PluginPermission::LibraryRead => "Bibliothek lesen",
        PluginPermission::LibraryAdd => "Spiele zur Bibliothek hinzufügen",
        PluginPermission::CatalogSearch => "Im Katalog suchen",
        PluginPermission::AchievementsDefine => "Eigene Achievements festlegen",
        PluginPermission::UiAddSidebarItem => "Einträge in der Seitenleiste",
        PluginPermission::UiNotify => "Hinweise anzeigen",
        PluginPermission::Storage => "Eigene Daten speichern",
    }
}

impl Plugins {
    pub fn new(pool: SqlitePool, user_dir: PathBuf, extra_dirs: Vec<PathBuf>) -> Self {
        Self {
            pool,
            tokens: std::sync::Mutex::new(HashMap::new()),
            user_dir,
            extra_dirs,
        }
    }

    pub fn user_dir(&self) -> &Path {
        &self.user_dir
    }

    /// All plugin folders; the user folder wins over development folders for the same id.
    fn discover(&self) -> Vec<Found> {
        let mut found: Vec<Found> = Vec::new();
        let dirs = std::iter::once((&self.user_dir, true))
            .chain(self.extra_dirs.iter().map(|d| (d, false)));
        for (root, removable) in dirs {
            let Ok(entries) = std::fs::read_dir(root) else {
                continue;
            };
            let mut entries: Vec<_> = entries.flatten().filter(|e| e.path().is_dir()).collect();
            entries.sort_by_key(std::fs::DirEntry::file_name);
            for entry in entries {
                let dir = entry.path();
                if !dir.join(MANIFEST).exists() {
                    continue;
                }
                let manifest = read_manifest(&dir);
                let id = manifest.as_ref().map_or_else(
                    |_| entry.file_name().to_string_lossy().into_owned(),
                    |m| m.id.clone(),
                );
                if found.iter().any(|f| f.id == id) {
                    continue;
                }
                found.push(Found {
                    id,
                    dir,
                    manifest,
                    removable,
                });
            }
        }
        found
    }

    async fn rows(&self) -> AppResult<HashMap<String, Row>> {
        let rows: Vec<Row> = sqlx::query_as("SELECT id, enabled, granted FROM plugins")
            .fetch_all(&self.pool)
            .await?;
        Ok(rows.into_iter().map(|r| (r.id.clone(), r)).collect())
    }

    fn token(&self, id: &str) -> String {
        let mut tokens = self
            .tokens
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        tokens
            .entry(id.to_owned())
            .or_insert_with(|| {
                crate::auth::state::new_state().unwrap_or_else(|_| format!("{id}-{}", now_iso()))
            })
            .clone()
    }

    fn info(&self, found: &Found, row: Option<&Row>) -> PluginInfo {
        let granted: Vec<PluginPermission> = row
            .and_then(|r| serde_json::from_str(&r.granted).ok())
            .unwrap_or_default();
        let enabled = row.is_some_and(|r| r.enabled);
        match &found.manifest {
            Err(error) => PluginInfo {
                id: found.id.clone(),
                name: found.id.clone(),
                version: String::new(),
                author: String::new(),
                description: String::new(),
                permissions: Vec::new(),
                granted,
                enabled: false,
                status: PluginStatus::Invalid,
                error: Some(error.clone()),
                removable: found.removable,
                entry_url: None,
            },
            Ok(m) => {
                let approved = row.is_some() && m.permissions.iter().all(|p| granted.contains(p));
                let status = match (approved, enabled) {
                    (false, _) => PluginStatus::NeedsApproval,
                    (true, true) => PluginStatus::Active,
                    (true, false) => PluginStatus::Disabled,
                };
                PluginInfo {
                    id: m.id.clone(),
                    name: m.name.clone(),
                    version: m.version.clone(),
                    author: m.author.clone(),
                    description: m.description.clone(),
                    permissions: m.permissions.clone(),
                    granted,
                    enabled: status == PluginStatus::Active,
                    status,
                    error: None,
                    removable: found.removable,
                    entry_url: (status == PluginStatus::Active)
                        .then(|| entry_url(&self.token(&m.id), &m.entry)),
                }
            }
        }
    }

    pub async fn list(&self) -> AppResult<Vec<PluginInfo>> {
        let rows = self.rows().await?;
        let mut list: Vec<PluginInfo> = self
            .discover()
            .iter()
            .map(|f| self.info(f, rows.get(&f.id)))
            .collect();
        list.sort_by_key(|p| p.name.to_lowercase());
        Ok(list)
    }

    pub async fn get(&self, id: &str) -> AppResult<PluginInfo> {
        self.list()
            .await?
            .into_iter()
            .find(|p| p.id == id)
            .ok_or_else(|| not_found(id))
    }

    fn manifest(&self, id: &str) -> AppResult<(Found, Manifest)> {
        let found = self
            .discover()
            .into_iter()
            .find(|f| f.id == id)
            .ok_or_else(|| not_found(id))?;
        let manifest = found.manifest.clone().map_err(AppError::Plugin)?;
        Ok((found, manifest))
    }

    /// The user confirmed the requested permissions.
    pub async fn approve(&self, id: &str) -> AppResult<PluginInfo> {
        let (_, manifest) = self.manifest(id)?;
        sqlx::query(
            "INSERT INTO plugins (id, enabled, granted, approved_version, approved_at) VALUES (?, 1, ?, ?, ?) \
             ON CONFLICT (id) DO UPDATE SET enabled = 1, granted = excluded.granted, \
               approved_version = excluded.approved_version, approved_at = excluded.approved_at",
        )
        .bind(id)
        .bind(serde_json::to_string(&manifest.permissions)?)
        .bind(&manifest.version)
        .bind(now_iso())
        .execute(&self.pool)
        .await?;
        self.get(id).await
    }

    pub async fn set_enabled(&self, id: &str, enabled: bool) -> AppResult<PluginInfo> {
        let info = self.get(id).await?;
        match info.status {
            PluginStatus::Invalid => return Err(AppError::Plugin(info.error.unwrap_or_default())),
            PluginStatus::NeedsApproval if enabled => {
                return Err(AppError::Plugin(
                    "Bitte bestätige zuerst die Berechtigungen des Plugins.".into(),
                ));
            }
            _ => {}
        }
        sqlx::query("UPDATE plugins SET enabled = ? WHERE id = ?")
            .bind(enabled)
            .bind(id)
            .execute(&self.pool)
            .await?;
        self.get(id).await
    }

    pub fn inspect(&self, path: &Path) -> AppResult<PluginManifestPreview> {
        if !path.is_dir() {
            return Err(AppError::InvalidInput(
                "Bitte gib den Ordner des Plugins an (mit plugin.json).".into(),
            ));
        }
        let m = read_manifest(path).map_err(AppError::Plugin)?;
        Ok(PluginManifestPreview {
            already_installed: self.user_dir.join(&m.id).join(MANIFEST).exists(),
            id: m.id,
            name: m.name,
            version: m.version,
            author: m.author,
            description: m.description,
            permissions: m.permissions,
        })
    }

    /// Copies the plugin into Cove's plugin folder (replacing an older version)
    /// and grants the permissions the user just confirmed.
    pub async fn install(&self, path: &Path) -> AppResult<PluginInfo> {
        let preview = self.inspect(path)?;
        let source = path.canonicalize()?;
        let target = self.user_dir.join(&preview.id);
        if target.exists() && target.canonicalize()? == source {
            return self.approve(&preview.id).await;
        }
        std::fs::create_dir_all(&self.user_dir)?;
        let staging = self.user_dir.join(format!(".installing-{}", preview.id));
        let _ = std::fs::remove_dir_all(&staging);
        let mut budget = (MAX_INSTALL_FILES, MAX_INSTALL_BYTES);
        if let Err(error) = copy_tree(&source, &staging, &mut budget) {
            let _ = std::fs::remove_dir_all(&staging);
            return Err(error);
        }
        if target.exists() {
            std::fs::remove_dir_all(&target)?;
        }
        std::fs::rename(&staging, &target)?;
        self.approve(&preview.id).await
    }

    /// Removes an installed plugin with its data.
    pub async fn uninstall(&self, id: &str) -> AppResult<()> {
        let found = self
            .discover()
            .into_iter()
            .find(|f| f.id == id)
            .ok_or_else(|| not_found(id))?;
        if !found.removable {
            return Err(AppError::Plugin(
                "Dieses Plugin liegt nicht in Coves Plugin-Ordner und kann hier nicht entfernt werden.".into(),
            ));
        }
        std::fs::remove_dir_all(&found.dir)?;
        for table in ["plugins", "plugin_storage", "plugin_achievements"] {
            let column = if table == "plugins" {
                "id"
            } else {
                "plugin_id"
            };
            sqlx::query(&format!("DELETE FROM {table} WHERE {column} = ?"))
                .bind(id)
                .execute(&self.pool)
                .await?;
        }
        Ok(())
    }

    /// Fails unless the plugin runs and was granted `permission`.
    pub async fn require(&self, id: &str, permission: PluginPermission) -> AppResult<PluginInfo> {
        let info = self.get(id).await?;
        if info.status != PluginStatus::Active {
            return Err(AppError::Plugin(format!(
                "Das Plugin „{}“ ist nicht aktiv.",
                info.name
            )));
        }
        if !info.granted.contains(&permission) {
            return Err(AppError::Plugin(format!(
                "Das Plugin „{}“ hat keine Berechtigung für „{}“.",
                info.name,
                label(permission)
            )));
        }
        Ok(info)
    }

    // ───────────── storage ─────────────

    fn check_key(key: &str) -> AppResult<()> {
        if key.is_empty() || key.len() > 200 {
            return Err(AppError::InvalidInput(
                "Speicher-Schlüssel müssen 1 bis 200 Zeichen lang sein.".into(),
            ));
        }
        Ok(())
    }

    pub async fn storage_get(&self, id: &str, key: &str) -> AppResult<Value> {
        self.require(id, PluginPermission::Storage).await?;
        let value: Option<String> =
            sqlx::query_scalar("SELECT value FROM plugin_storage WHERE plugin_id = ? AND key = ?")
                .bind(id)
                .bind(key)
                .fetch_optional(&self.pool)
                .await?;
        Ok(value
            .map(|v| serde_json::from_str(&v))
            .transpose()?
            .unwrap_or(Value::Null))
    }

    pub async fn storage_set(&self, id: &str, key: &str, value: &Value) -> AppResult<()> {
        self.require(id, PluginPermission::Storage).await?;
        Self::check_key(key)?;
        let raw = serde_json::to_string(value)?;
        if raw.len() > MAX_VALUE_BYTES {
            return Err(AppError::Plugin(
                "Ein gespeicherter Wert darf höchstens 64 KB groß sein.".into(),
            ));
        }
        let (used, keys): (i64, i64) = sqlx::query_as(
            "SELECT COALESCE(SUM(LENGTH(value) + LENGTH(key)), 0), COUNT(*) FROM plugin_storage WHERE plugin_id = ? AND key <> ?",
        )
        .bind(id)
        .bind(key)
        .fetch_one(&self.pool)
        .await?;
        let size = i64::try_from(raw.len() + key.len()).unwrap_or(i64::MAX);
        if used + size > MAX_STORAGE_BYTES || keys >= MAX_KEYS {
            return Err(AppError::Plugin(
                "Der Speicher des Plugins ist voll (1 MB).".into(),
            ));
        }
        sqlx::query(
            "INSERT INTO plugin_storage (plugin_id, key, value) VALUES (?, ?, ?) \
             ON CONFLICT (plugin_id, key) DO UPDATE SET value = excluded.value",
        )
        .bind(id)
        .bind(key)
        .bind(raw)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    pub async fn storage_remove(&self, id: &str, key: &str) -> AppResult<()> {
        self.require(id, PluginPermission::Storage).await?;
        sqlx::query("DELETE FROM plugin_storage WHERE plugin_id = ? AND key = ?")
            .bind(id)
            .bind(key)
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    pub async fn storage_keys(&self, id: &str) -> AppResult<Vec<String>> {
        self.require(id, PluginPermission::Storage).await?;
        Ok(
            sqlx::query_scalar("SELECT key FROM plugin_storage WHERE plugin_id = ? ORDER BY key")
                .bind(id)
                .fetch_all(&self.pool)
                .await?,
        )
    }

    // ───────────── achievements ─────────────

    /// Replaces the plugin's achievements. Returns how many were stored.
    pub async fn define_achievements(&self, id: &str, raw: &[Value]) -> AppResult<usize> {
        self.require(id, PluginPermission::AchievementsDefine)
            .await?;
        let definitions = parse_definitions(raw)?;
        sqlx::query(
            "INSERT INTO plugin_achievements (plugin_id, definitions, updated_at) VALUES (?, ?, ?) \
             ON CONFLICT (plugin_id) DO UPDATE SET definitions = excluded.definitions, updated_at = excluded.updated_at",
        )
        .bind(id)
        .bind(serde_json::to_string(raw)?)
        .bind(now_iso())
        .execute(&self.pool)
        .await?;
        Ok(definitions.len())
    }

    /// Achievements of active plugins with the permission, ids prefixed with the plugin id.
    pub async fn achievement_definitions(&self) -> AppResult<Vec<Definition>> {
        let rows: Vec<(String, String)> =
            sqlx::query_as("SELECT plugin_id, definitions FROM plugin_achievements")
                .fetch_all(&self.pool)
                .await?;
        let active: Vec<PluginInfo> = self
            .list()
            .await?
            .into_iter()
            .filter(|p| {
                p.status == PluginStatus::Active
                    && p.granted.contains(&PluginPermission::AchievementsDefine)
            })
            .collect();
        let mut all = Vec::new();
        for (plugin_id, raw) in rows {
            if !active.iter().any(|p| p.id == plugin_id) {
                continue;
            }
            let values: Vec<Value> = serde_json::from_str(&raw).unwrap_or_default();
            let Ok(definitions) = parse_definitions(&values) else {
                continue;
            };
            all.extend(definitions.into_iter().map(|mut d| {
                d.id = format!("{plugin_id}:{}", d.id);
                d
            }));
        }
        Ok(all)
    }

    // ───────────── files for the worker ─────────────

    /// A file of an active plugin, for `cove-plugin://localhost/<token>/<path>`.
    pub async fn resolve_file(&self, request_path: &str) -> AppResult<PathBuf> {
        let decoded = percent_encoding::percent_decode_str(request_path.trim_start_matches('/'))
            .decode_utf8_lossy()
            .into_owned();
        let (token, rest) = decoded.split_once('/').ok_or_else(|| not_found(&decoded))?;
        let id = {
            let tokens = self
                .tokens
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            tokens
                .iter()
                .find(|(_, t)| crate::auth::state::constant_time_eq(t, token))
                .map(|(id, _)| id.clone())
        }
        .ok_or_else(|| AppError::InvalidInput("Unbekannter Plugin-Pfad.".into()))?;
        let id = id.as_str();
        let info = self.get(id).await?;
        if info.status != PluginStatus::Active {
            return Err(AppError::Plugin(format!(
                "Das Plugin „{}“ ist nicht aktiv.",
                info.name
            )));
        }
        let relative =
            safe_relative(rest).ok_or_else(|| AppError::InvalidInput("Ungültiger Pfad.".into()))?;
        let (found, _) = self.manifest(id)?;
        let root = found.dir.canonicalize()?;
        let file = root
            .join(relative)
            .canonicalize()
            .map_err(|_| not_found(id))?;
        // Symlinks must not lead out of the plugin folder.
        if !file.starts_with(&root) || !file.is_file() {
            return Err(AppError::InvalidInput("Ungültiger Pfad.".into()));
        }
        Ok(file)
    }
}

fn parse_definitions(raw: &[Value]) -> AppResult<Vec<Definition>> {
    if raw.len() > MAX_ACHIEVEMENTS {
        return Err(AppError::Plugin(format!(
            "Ein Plugin kann höchstens {MAX_ACHIEVEMENTS} Achievements festlegen."
        )));
    }
    let mut ids = BTreeSet::new();
    raw.iter()
        .map(|value| {
            let d: Definition = serde_json::from_value(value.clone())
                .map_err(|e| AppError::Plugin(format!("Ungültiges Achievement ({e}).")))?;
            let bad = |m: &str| Err(AppError::Plugin(format!("Achievement „{}“: {m}", d.id)));
            if !valid_id(&d.id) {
                return bad("Die ID darf nur Kleinbuchstaben, Ziffern und Bindestriche enthalten.");
            }
            if !ids.insert(d.id.clone()) {
                return bad("Die ID ist doppelt.");
            }
            if d.title.trim().is_empty()
                || d.title.chars().count() > 60
                || d.description.chars().count() > 200
            {
                return bad(
                    "Titel (bis 60 Zeichen) oder Beschreibung (bis 200 Zeichen) passen nicht.",
                );
            }
            let ok = match d.rule {
                Rule::PlaytimeHours { hours } => (1..=100_000).contains(&hours),
                Rule::LibraryGames { count } | Rule::RecommendedPlayed { count } => {
                    (1..=100_000).contains(&count)
                }
                Rule::PlayedBetween { from_hour, to_hour } => from_hour < to_hour && to_hour <= 24,
            };
            if !ok {
                return bad("Die Regel hat ungültige Werte.");
            }
            Ok(d)
        })
        .collect()
}

/// Copies a plugin folder; symlinks are skipped, size and file count are limited.
fn copy_tree(from: &Path, to: &Path, budget: &mut (usize, u64)) -> AppResult<()> {
    std::fs::create_dir_all(to)?;
    for entry in std::fs::read_dir(from)? {
        let entry = entry?;
        let kind = entry.file_type()?;
        let name = entry.file_name();
        if kind.is_symlink() || name.to_string_lossy().starts_with(".git") || name == "node_modules"
        {
            continue;
        }
        let target = to.join(&name);
        if kind.is_dir() {
            copy_tree(&entry.path(), &target, budget)?;
        } else {
            let size = entry.metadata()?.len();
            if budget.0 == 0 || size > budget.1 {
                return Err(AppError::Plugin(
                    "Das Plugin ist zu groß (höchstens 20 MB und 1000 Dateien).".into(),
                ));
            }
            budget.0 -= 1;
            budget.1 -= size;
            std::fs::copy(entry.path(), &target)?;
        }
    }
    Ok(())
}

pub fn mime(path: &Path) -> &'static str {
    match path.extension().and_then(|e| e.to_str()).unwrap_or("") {
        "js" | "mjs" => "text/javascript; charset=utf-8",
        "json" => "application/json; charset=utf-8",
        "css" => "text/css; charset=utf-8",
        "svg" => "image/svg+xml",
        "png" => "image/png",
        _ => "application/octet-stream",
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::open_in_memory;
    use serde_json::json;

    fn write_plugin(root: &Path, folder: &str, manifest: &Value) -> PathBuf {
        let dir = root.join(folder);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join(MANIFEST), manifest.to_string()).unwrap();
        std::fs::write(dir.join("index.js"), "export function activate() {}").unwrap();
        dir
    }

    fn manifest(id: &str, permissions: &[&str]) -> Value {
        json!({ "id": id, "name": id, "version": "1.0.0", "author": "Test", "entry": "index.js", "permissions": permissions })
    }

    async fn registry() -> (Plugins, tempfile::TempDir) {
        let tmp = tempfile::tempdir().unwrap();
        let pool = open_in_memory().await.unwrap();
        let plugins = Plugins::new(pool, tmp.path().join("user"), vec![tmp.path().join("dev")]);
        (plugins, tmp)
    }

    #[test]
    fn validates_manifests() {
        let tmp = tempfile::tempdir().unwrap();
        let ok = write_plugin(
            tmp.path(),
            "ok",
            &manifest("random-pick", &["ui.notify", "library.read", "ui.notify"]),
        );
        let m = read_manifest(&ok).unwrap();
        assert_eq!(
            m.permissions,
            [PluginPermission::LibraryRead, PluginPermission::UiNotify]
        );
        assert_eq!(m.author, "Test");

        let cases = [
            (
                json!({ "id": "Bad Id", "name": "x", "version": "1.0.0", "entry": "index.js" }),
                "Plugin-ID",
            ),
            (
                json!({ "id": "x", "name": "x", "version": "1.0", "entry": "index.js" }),
                "Version",
            ),
            (
                json!({ "id": "x", "name": "x", "version": "1.0.0", "entry": "../evil.js" }),
                "entry",
            ),
            (
                json!({ "id": "x", "name": "x", "version": "1.0.0", "entry": "index.ts" }),
                "entry",
            ),
            (
                json!({ "id": "x", "name": "x", "version": "1.0.0", "entry": "missing.js" }),
                "fehlt",
            ),
            (
                json!({ "id": "x", "name": "x", "version": "1.0.0", "entry": "index.js", "apiVersion": 2 }),
                "Plugin-API 2",
            ),
            (
                json!({ "id": "x", "name": "x", "version": "1.0.0", "entry": "index.js", "permissions": ["fs.write"] }),
                "unbekannte Berechtigung",
            ),
        ];
        for (i, (value, expected)) in cases.iter().enumerate() {
            let dir = write_plugin(tmp.path(), &format!("case{i}"), value);
            let error = read_manifest(&dir).unwrap_err();
            assert!(error.contains(expected), "{value}: {error}");
        }
    }

    #[tokio::test]
    async fn new_plugins_wait_for_approval_and_new_permissions_need_it_again() {
        let (plugins, tmp) = registry().await;
        let dir = write_plugin(
            &tmp.path().join("dev"),
            "goals",
            &manifest("goals", &["achievements.define"]),
        );
        write_plugin(&tmp.path().join("dev"), "broken", &json!({ "id": "x" }));

        let list = plugins.list().await.unwrap();
        assert_eq!(list.len(), 2);
        let goals = list.iter().find(|p| p.id == "goals").unwrap();
        assert_eq!(
            (goals.status, goals.entry_url.as_deref()),
            (PluginStatus::NeedsApproval, None)
        );
        assert!(!goals.removable);
        let broken = list.iter().find(|p| p.id == "broken").unwrap();
        assert_eq!(broken.status, PluginStatus::Invalid);
        assert!(
            plugins.set_enabled("goals", true).await.is_err(),
            "approval first"
        );

        let goals = plugins.approve("goals").await.unwrap();
        assert_eq!(goals.status, PluginStatus::Active);
        let url = goals.entry_url.unwrap();
        assert!(
            url.ends_with("/index.js") && !url.contains("/goals/"),
            "{url}"
        );
        assert_eq!(
            plugins.set_enabled("goals", false).await.unwrap().status,
            PluginStatus::Disabled
        );
        assert_eq!(
            plugins.set_enabled("goals", true).await.unwrap().status,
            PluginStatus::Active
        );

        // An update asks for more: back to approval.
        std::fs::write(
            dir.join(MANIFEST),
            manifest("goals", &["achievements.define", "storage"]).to_string(),
        )
        .unwrap();
        assert_eq!(
            plugins.get("goals").await.unwrap().status,
            PluginStatus::NeedsApproval
        );
        assert!(plugins
            .require("goals", PluginPermission::AchievementsDefine)
            .await
            .is_err());
    }

    #[tokio::test]
    async fn install_copies_and_uninstall_removes_data() {
        let (plugins, tmp) = registry().await;
        let source = write_plugin(
            &tmp.path().join("downloads"),
            "pick",
            &manifest("random-pick", &["storage"]),
        );
        std::fs::create_dir_all(source.join("lib")).unwrap();
        std::fs::write(source.join("lib/util.js"), "export const x = 1;").unwrap();

        let preview = plugins.inspect(&source).unwrap();
        assert_eq!(
            (preview.id.as_str(), preview.already_installed),
            ("random-pick", false)
        );
        let info = plugins.install(&source).await.unwrap();
        assert_eq!((info.status, info.removable), (PluginStatus::Active, true));
        assert!(tmp.path().join("user/random-pick/lib/util.js").is_file());
        assert!(plugins.inspect(&source).unwrap().already_installed);

        plugins
            .storage_set("random-pick", "last", &json!({ "game": "a" }))
            .await
            .unwrap();
        assert_eq!(plugins.storage_keys("random-pick").await.unwrap(), ["last"]);
        plugins.uninstall("random-pick").await.unwrap();
        assert!(!tmp.path().join("user/random-pick").exists());
        assert!(plugins.get("random-pick").await.is_err());
        let left: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM plugin_storage")
            .fetch_one(&plugins.pool)
            .await
            .unwrap();
        assert_eq!(left, 0);
        assert!(matches!(
            plugins.inspect(&tmp.path().join("nope")),
            Err(AppError::InvalidInput(_))
        ));
    }

    #[tokio::test]
    async fn storage_is_private_checked_and_limited() {
        let (plugins, tmp) = registry().await;
        write_plugin(&tmp.path().join("dev"), "a", &manifest("a", &["storage"]));
        write_plugin(&tmp.path().join("dev"), "b", &manifest("b", &["ui.notify"]));
        plugins.approve("a").await.unwrap();
        plugins.approve("b").await.unwrap();

        plugins
            .storage_set("a", "k", &json!([1, 2, 3]))
            .await
            .unwrap();
        assert_eq!(
            plugins.storage_get("a", "k").await.unwrap(),
            json!([1, 2, 3])
        );
        assert_eq!(
            plugins.storage_get("a", "missing").await.unwrap(),
            Value::Null
        );
        assert!(
            matches!(plugins.storage_get("b", "k").await, Err(AppError::Plugin(m)) if m.contains("Eigene Daten speichern"))
        );

        let big = json!("x".repeat(MAX_VALUE_BYTES + 1));
        assert!(plugins.storage_set("a", "big", &big).await.is_err());
        assert!(plugins.storage_set("a", "", &json!(1)).await.is_err());
        plugins.storage_remove("a", "k").await.unwrap();
        assert_eq!(
            plugins.storage_keys("a").await.unwrap(),
            Vec::<String>::new()
        );

        plugins.set_enabled("a", false).await.unwrap();
        assert!(
            plugins.storage_get("a", "k").await.is_err(),
            "disabled plugins cannot use the API"
        );
    }

    #[tokio::test]
    async fn achievements_are_validated_and_only_active_plugins_count() {
        let (plugins, tmp) = registry().await;
        write_plugin(
            &tmp.path().join("dev"),
            "goals",
            &manifest("goals", &["achievements.define"]),
        );
        plugins.approve("goals").await.unwrap();
        let good = json!([{ "id": "marathon", "title": "Marathon", "description": "100 Stunden", "rule": { "type": "playtimeHours", "hours": 100 } }]);
        assert_eq!(
            plugins
                .define_achievements("goals", good.as_array().unwrap())
                .await
                .unwrap(),
            1
        );
        let ids: Vec<String> = plugins
            .achievement_definitions()
            .await
            .unwrap()
            .into_iter()
            .map(|d| d.id)
            .collect();
        assert_eq!(ids, ["goals:marathon"]);

        for bad in [
            json!([{ "id": "Bad", "title": "x", "description": "", "rule": { "type": "libraryGames", "count": 1 } }]),
            json!([{ "id": "a", "title": "x", "description": "", "rule": { "type": "playtimeHours", "hours": 0 } }]),
            json!([{ "id": "a", "title": "x", "description": "", "rule": { "type": "playedBetween", "fromHour": 5, "toHour": 2 } }]),
            json!([{ "id": "a", "title": "x", "description": "", "rule": { "type": "deleteAll" } }]),
        ] {
            assert!(
                plugins
                    .define_achievements("goals", bad.as_array().unwrap())
                    .await
                    .is_err(),
                "{bad}"
            );
        }
        plugins.set_enabled("goals", false).await.unwrap();
        assert_eq!(plugins.achievement_definitions().await.unwrap(), Vec::new());
    }

    #[tokio::test]
    async fn serves_only_files_of_active_plugins_inside_their_folder() {
        let (plugins, tmp) = registry().await;
        let dir = write_plugin(&tmp.path().join("dev"), "pick", &manifest("pick", &[]));
        write_plugin(&tmp.path().join("dev"), "other", &manifest("other", &[]));
        std::fs::write(tmp.path().join("dev/secret.txt"), "nope").unwrap();
        let token = plugins.token("pick");
        assert!(
            plugins
                .resolve_file(&format!("/{token}/index.js"))
                .await
                .is_err(),
            "not approved yet"
        );
        plugins.approve("pick").await.unwrap();
        plugins.approve("other").await.unwrap();
        assert_eq!(
            plugins
                .resolve_file(&format!("/{token}/index.js"))
                .await
                .unwrap(),
            dir.join("index.js").canonicalize().unwrap()
        );
        // Plugin ids are not paths: another plugin cannot guess its way in.
        assert!(plugins.resolve_file("/pick/index.js").await.is_err());
        assert!(plugins.resolve_file("/other/index.js").await.is_err());
        assert_ne!(plugins.token("other"), token);
        assert!(plugins
            .resolve_file(&format!("/{token}/../secret.txt"))
            .await
            .is_err());
        assert!(plugins
            .resolve_file(&format!("/{token}/%2E%2E/secret.txt"))
            .await
            .is_err());
        #[cfg(unix)]
        {
            std::os::unix::fs::symlink(tmp.path().join("dev/secret.txt"), dir.join("link.js"))
                .unwrap();
            assert!(
                plugins
                    .resolve_file(&format!("/{token}/link.js"))
                    .await
                    .is_err(),
                "symlink out of the folder"
            );
        }
        assert_eq!(mime(Path::new("a.js")), "text/javascript; charset=utf-8");
    }
}
