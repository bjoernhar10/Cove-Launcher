//! The Ludusavi manifest (MIT, compiled from PCGamingWiki): where thousands of
//! games keep their saves. Cove downloads it (`ETag`, at most once a day) and
//! turns the 17 MB YAML into a small JSON index on disk; a backup reads the
//! index, takes its game and drops the rest again.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::time::Duration;

use serde::{Deserialize, Serialize};

use crate::error::{AppError, AppResult};

pub const MANIFEST_URL: &str =
    "https://raw.githubusercontent.com/mtkennerly/ludusavi-manifest/refs/heads/data/data/manifest.yaml";
const REFRESH: Duration = Duration::from_secs(24 * 60 * 60);

/// One file rule: a path with placeholders and where it applies.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FileRule {
    pub path: String,
    /// `windows`, `linux`, `mac`; empty = everywhere.
    #[serde(default)]
    pub os: Vec<String>,
    /// `steam`, `gog`, `epic` …; empty = every store.
    #[serde(default)]
    pub stores: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct IndexGame {
    pub name: String,
    #[serde(default)]
    pub files: Vec<FileRule>,
    #[serde(default)]
    pub steam: Option<u64>,
    #[serde(default)]
    pub gog: Option<u64>,
    /// This name is another name of that game.
    #[serde(default)]
    pub alias: Option<String>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Index {
    pub games: Vec<IndexGame>,
}

// ── YAML as it comes ──

#[derive(Deserialize)]
struct RawGame {
    #[serde(default)]
    files: HashMap<String, Option<RawFile>>,
    #[serde(default)]
    steam: Option<RawId>,
    #[serde(default)]
    gog: Option<RawId>,
    #[serde(default)]
    alias: Option<String>,
}

#[derive(Deserialize, Default)]
struct RawFile {
    #[serde(default)]
    tags: Vec<String>,
    #[serde(default)]
    when: Vec<RawWhen>,
}

#[derive(Deserialize)]
struct RawWhen {
    #[serde(default)]
    os: Option<String>,
    #[serde(default)]
    store: Option<String>,
}

#[derive(Deserialize)]
struct RawId {
    #[serde(default)]
    id: Option<u64>,
}

/// Saves and settings (like Ludusavi's default); rules without tags count too.
fn wanted(tags: &[String]) -> bool {
    tags.is_empty() || tags.iter().any(|t| t == "save" || t == "config")
}

/// The index of a manifest. Relative paths are refused (the format forbids them).
pub fn index_from_yaml(yaml: &str) -> AppResult<Index> {
    let raw: HashMap<String, RawGame> = serde_yaml_ng::from_str(yaml).map_err(|e| {
        eprintln!("[cove] Ludusavi-Manifest unlesbar: {e}");
        AppError::Download(tr!(
            "Die Liste der Spielstand-Orte ist beschädigt.",
            "The list of save locations is damaged."
        ))
    })?;
    let mut games: Vec<IndexGame> = raw
        .into_iter()
        .filter_map(|(name, game)| {
            let mut files: Vec<FileRule> = game
                .files
                .into_iter()
                .filter(|(path, _)| !path.starts_with("./") && !path.starts_with("../"))
                .filter_map(|(path, rule)| {
                    let rule = rule.unwrap_or_default();
                    wanted(&rule.tags).then(|| FileRule {
                        path,
                        os: rule.when.iter().filter_map(|w| w.os.clone()).collect(),
                        stores: rule.when.iter().filter_map(|w| w.store.clone()).collect(),
                    })
                })
                .collect();
            files.sort_by(|a, b| a.path.cmp(&b.path));
            (!files.is_empty() || game.alias.is_some()).then(|| IndexGame {
                name,
                files,
                steam: game.steam.and_then(|s| s.id),
                gog: game.gog.and_then(|g| g.id),
                alias: game.alias,
            })
        })
        .collect();
    games.sort_by(|a, b| a.name.cmp(&b.name));
    Ok(Index { games })
}

impl Index {
    fn named(&self, name: &str) -> Option<&IndexGame> {
        self.games.iter().find(|g| g.name == name)
    }

    /// The game by its store id, else by its exact (normalized) title.
    /// Aliases lead to the game they stand for.
    pub fn find(&self, steam: Option<u64>, gog: Option<u64>, title: &str) -> Option<&IndexGame> {
        let by_id = self
            .games
            .iter()
            .find(|g| (steam.is_some() && g.steam == steam) || (gog.is_some() && g.gog == gog));
        let wanted = crate::catalog::matching::normalize(title);
        let found = by_id.or_else(|| {
            (!wanted.is_empty()).then(|| {
                self.games
                    .iter()
                    .find(|g| crate::catalog::matching::normalize(&g.name) == wanted)
            })?
        })?;
        match &found.alias {
            Some(target) if found.files.is_empty() => self.named(target),
            _ => Some(found),
        }
    }
}

/// Keeps the index in `<dir>/index.json` up to date.
pub struct ManifestStore {
    dir: PathBuf,
    url: String,
    client: reqwest::Client,
}

impl ManifestStore {
    pub fn new(dir: PathBuf, url: String, client: reqwest::Client) -> Self {
        Self { dir, url, client }
    }

    fn index_path(&self) -> PathBuf {
        self.dir.join("index.json")
    }

    fn etag_path(&self) -> PathBuf {
        self.dir.join("manifest.etag")
    }

    fn fresh(path: &Path) -> bool {
        std::fs::metadata(path)
            .and_then(|m| m.modified())
            .is_ok_and(|t| t.elapsed().is_ok_and(|age| age < REFRESH))
    }

    /// The index: as it is when it was checked today, else after asking for a
    /// new manifest. An old index beats none when the download fails.
    pub async fn index(&self) -> AppResult<Index> {
        let path = self.index_path();
        if !(path.is_file() && Self::fresh(&path)) {
            if let Err(error) = self.refresh().await {
                if !path.is_file() {
                    return Err(error);
                }
                eprintln!("[cove] Ludusavi-Manifest nicht aktualisiert: {error}");
            }
        }
        let data = tokio::fs::read(&path).await?;
        tokio::task::spawn_blocking(move || serde_json::from_slice(&data).map_err(AppError::from))
            .await
            .map_err(|e| AppError::Download(e.to_string()))?
    }

    async fn refresh(&self) -> AppResult<()> {
        tokio::fs::create_dir_all(&self.dir).await?;
        let mut request = self.client.get(&self.url).timeout(Duration::from_secs(120));
        let index_exists = self.index_path().is_file();
        if index_exists {
            if let Ok(etag) = tokio::fs::read_to_string(self.etag_path()).await {
                request = request.header("If-None-Match", etag.trim());
            }
        }
        let unreachable = || {
            AppError::Offline(tr!(
                "Die Liste der Spielstand-Orte konnte nicht geladen werden.",
                "The list of save locations could not be loaded."
            ))
        };
        let response = request.send().await.map_err(|_| unreachable())?;
        if response.status() == reqwest::StatusCode::NOT_MODIFIED && index_exists {
            touch(&self.index_path());
            return Ok(());
        }
        if !response.status().is_success() {
            return Err(unreachable());
        }
        let etag = response
            .headers()
            .get("etag")
            .and_then(|v| v.to_str().ok())
            .map(str::to_owned);
        let yaml = response.text().await.map_err(|_| unreachable())?;
        let index = tokio::task::spawn_blocking(move || index_from_yaml(&yaml))
            .await
            .map_err(|e| AppError::Download(e.to_string()))??;
        let temp = self.dir.join("index.json.part");
        tokio::fs::write(&temp, serde_json::to_vec(&index)?).await?;
        tokio::fs::rename(&temp, self.index_path()).await?;
        if let Some(etag) = etag {
            tokio::fs::write(self.etag_path(), etag).await?;
        }
        Ok(())
    }
}

fn touch(path: &Path) {
    if let Ok(file) = std::fs::File::options().write(true).open(path) {
        let _ = file.set_modified(std::time::SystemTime::now());
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use wiremock::matchers::{header, method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    pub const YAML: &str = r"
Saltmarsh:
  files:
    <winDocuments>/Saltmarsh/Saves:
      tags: [save]
    <base>/settings.ini:
      tags: [config]
      when:
        - os: windows
    <base>/logs:
      tags: [log]
    ./relative:
      tags: [save]
  steam:
    id: 1234
  gog:
    id: 5678
Saltmarsh GOTY:
  alias: Saltmarsh
Only Registry:
  registry:
    HKEY_CURRENT_USER/Software/Only:
      tags: [save]
";

    #[test]
    fn the_index_keeps_saves_and_settings() {
        let index = index_from_yaml(YAML).unwrap();
        assert_eq!(index.games.len(), 2, "registry-only games are dropped");
        let game = index.find(Some(1234), None, "").unwrap();
        let paths: Vec<&str> = game.files.iter().map(|f| f.path.as_str()).collect();
        assert_eq!(
            paths,
            ["<base>/settings.ini", "<winDocuments>/Saltmarsh/Saves"]
        );
        assert_eq!(game.files[0].os, ["windows"]);
        // By GOG id, by title, through an alias.
        assert_eq!(index.find(None, Some(5678), "x").unwrap().name, "Saltmarsh");
        assert_eq!(
            index.find(None, None, "saltmarsh").unwrap().name,
            "Saltmarsh"
        );
        assert_eq!(
            index.find(None, None, "Saltmarsh GOTY").unwrap().name,
            "Saltmarsh"
        );
        assert!(index.find(None, None, "Unknown").is_none());
    }

    #[tokio::test]
    async fn the_manifest_is_loaded_once_and_then_checked_with_its_etag() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/manifest.yaml"))
            .and(header("If-None-Match", "\"v1\""))
            .respond_with(ResponseTemplate::new(304))
            .expect(1)
            .mount(&server)
            .await;
        Mock::given(method("GET"))
            .and(path("/manifest.yaml"))
            .respond_with(
                ResponseTemplate::new(200)
                    .insert_header("etag", "\"v1\"")
                    .set_body_string(YAML),
            )
            .expect(1)
            .mount(&server)
            .await;
        let dir = tempfile::tempdir().unwrap();
        let store = ManifestStore::new(
            dir.path().to_path_buf(),
            format!("{}/manifest.yaml", server.uri()),
            reqwest::Client::new(),
        );
        assert_eq!(store.index().await.unwrap().games.len(), 2);
        // Fresh today: no request at all.
        assert_eq!(store.index().await.unwrap().games.len(), 2);
        // A day later: asked with the ETag, 304 keeps the index.
        let old = std::time::SystemTime::now() - Duration::from_secs(2 * 24 * 60 * 60);
        std::fs::File::options()
            .write(true)
            .open(dir.path().join("index.json"))
            .unwrap()
            .set_modified(old)
            .unwrap();
        assert_eq!(store.index().await.unwrap().games.len(), 2);
    }

    /// The real manifest: `COVE_LUDUSAVI_MANIFEST=<manifest.yaml> cargo test real_manifest -- --ignored`.
    #[test]
    #[ignore = "needs a downloaded manifest"]
    fn real_manifest() {
        let path = std::env::var("COVE_LUDUSAVI_MANIFEST").unwrap();
        let yaml = std::fs::read_to_string(path).unwrap();
        let start = std::time::Instant::now();
        let index = index_from_yaml(&yaml).unwrap();
        let json = serde_json::to_vec(&index).unwrap();
        eprintln!(
            "{} games, {} MB index, {:?}",
            index.games.len(),
            json.len() / 1_000_000,
            start.elapsed()
        );
        let celeste = index.find(Some(504_230), None, "").unwrap();
        assert_eq!(celeste.name, "Celeste");
        assert_ne!(celeste.files, Vec::new());
        assert!(index.find(None, None, "The Witcher 3: Wild Hunt").is_some());
    }

    #[tokio::test]
    async fn without_any_index_the_error_says_so() {
        let dir = tempfile::tempdir().unwrap();
        let store = ManifestStore::new(
            dir.path().to_path_buf(),
            "http://127.0.0.1:9/manifest.yaml".into(),
            reqwest::Client::new(),
        );
        assert!(matches!(store.index().await, Err(AppError::Offline(_))));
    }
}
