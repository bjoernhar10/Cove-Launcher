//! Screenshots of a game, gathered from where they already are: Steam's
//! screenshot folders (`userdata/<user>/760/remote/<app id>/screenshots`),
//! the Xbox Game Bar's captures (Windows, `Videos\Captures`, named after the
//! game) and a `Screenshots` folder in the game's own folder. Nothing is
//! copied or changed.
//!
//! The webview gets them through `cove-shot://localhost/<token>` (Windows:
//! `http://cove-shot.localhost/<token>`): a random token per file of a listing,
//! so only files Cove listed can be read, never an arbitrary path.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use crate::catalog::matching::normalize;
use crate::error::{AppError, AppResult};
use crate::models::{LibraryItem, Screenshot, ScreenshotSource, StoreSource};

pub const SCHEME: &str = "cove-shot";
/// More are not listed (newest first).
const MAX_SHOTS: usize = 500;
/// Tokens kept (the newest listings).
const MAX_TOKENS: usize = 4000;
const EXTENSIONS: &[&str] = &["png", "jpg", "jpeg", "webp", "bmp"];

pub struct Screenshots {
    steam_roots: Vec<PathBuf>,
    captures: Vec<PathBuf>,
    tokens: Mutex<HashMap<String, (PathBuf, u64)>>,
    next: Mutex<u64>,
}

/// Where Steam keeps its data on this machine.
pub fn steam_roots(home: Option<&Path>) -> Vec<PathBuf> {
    let mut roots: Vec<PathBuf> = home
        .map(|h| {
            [
                ".steam/steam",
                ".local/share/Steam",
                ".var/app/com.valvesoftware.Steam/data/Steam",
                "Library/Application Support/Steam",
            ]
            .iter()
            .map(|p| h.join(p))
            .collect()
        })
        .unwrap_or_default();
    if let Some(root) = crate::steam::windows_root() {
        roots.push(root);
    }
    roots
}

/// The Xbox Game Bar's folder (Windows).
pub fn capture_dirs(home: Option<&Path>) -> Vec<PathBuf> {
    if cfg!(windows) {
        home.map(|h| vec![h.join("Videos").join("Captures")])
            .unwrap_or_default()
    } else {
        Vec::new()
    }
}

fn is_image(path: &Path) -> bool {
    path.extension()
        .and_then(|e| e.to_str())
        .is_some_and(|e| EXTENSIONS.contains(&e.to_ascii_lowercase().as_str()))
}

/// Image files directly in a folder (links are not followed).
fn images_in(dir: &Path, out: &mut Vec<(PathBuf, ScreenshotSource)>, source: ScreenshotSource) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        let is_file = std::fs::symlink_metadata(&path).is_ok_and(|m| m.is_file());
        if is_file && is_image(&path) {
            out.push((path, source));
        }
    }
}

pub fn mime(path: &Path) -> &'static str {
    match path
        .extension()
        .and_then(|e| e.to_str())
        .map(str::to_ascii_lowercase)
        .as_deref()
    {
        Some("png") => "image/png",
        Some("webp") => "image/webp",
        Some("bmp") => "image/bmp",
        _ => "image/jpeg",
    }
}

fn url(token: &str) -> String {
    if cfg!(windows) {
        format!("http://{SCHEME}.localhost/{token}")
    } else {
        format!("{SCHEME}://localhost/{token}")
    }
}

impl Screenshots {
    pub fn new(steam_roots: Vec<PathBuf>, captures: Vec<PathBuf>) -> Self {
        Self {
            steam_roots,
            captures,
            tokens: Mutex::new(HashMap::new()),
            next: Mutex::new(0),
        }
    }

    /// The files that show this game, newest first.
    fn files(&self, item: &LibraryItem) -> Vec<(PathBuf, ScreenshotSource)> {
        let mut found = Vec::new();
        if let (StoreSource::Steam, Some(app)) =
            (item.entry.source, item.entry.store_game_id.as_deref())
        {
            if app.chars().all(|c| c.is_ascii_digit()) {
                for root in &self.steam_roots {
                    let Ok(users) = std::fs::read_dir(root.join("userdata")) else {
                        continue;
                    };
                    for user in users.flatten() {
                        let dir = user.path().join("760/remote").join(app).join("screenshots");
                        images_in(&dir, &mut found, ScreenshotSource::Steam);
                    }
                }
            }
        }
        // Game Bar names captures "<game> <date>.png".
        let title = normalize(&item.game.title);
        if !title.is_empty() {
            for dir in &self.captures {
                let mut captures = Vec::new();
                images_in(dir, &mut captures, ScreenshotSource::Xbox);
                found.extend(captures.into_iter().filter(|(path, _)| {
                    path.file_stem()
                        .map(|s| normalize(&s.to_string_lossy()))
                        .is_some_and(|stem| {
                            stem.strip_prefix(&title)
                                .is_some_and(|rest| rest.is_empty() || rest.starts_with(' '))
                        })
                }));
            }
        }
        if let Some(folder) = item
            .entry
            .install_path
            .as_deref()
            .filter(|_| item.entry.installed)
            .map(Path::new)
            .filter(|p| p.components().count() >= 3)
        {
            for name in ["Screenshots", "screenshots", "ScreenShots"] {
                images_in(&folder.join(name), &mut found, ScreenshotSource::Game);
            }
        }
        found.sort();
        found.dedup_by(|a, b| a.0 == b.0);
        found
    }

    /// The game's screenshots, newest first, each with a token URL.
    pub fn list(&self, item: &LibraryItem) -> Vec<Screenshot> {
        let mut shots: Vec<(PathBuf, ScreenshotSource, std::time::SystemTime, u64)> = self
            .files(item)
            .into_iter()
            .filter_map(|(path, source)| {
                let meta = std::fs::metadata(&path).ok()?;
                Some((path, source, meta.modified().ok()?, meta.len()))
            })
            .collect();
        shots.sort_by(|a, b| b.2.cmp(&a.2).then_with(|| a.0.cmp(&b.0)));
        shots.truncate(MAX_SHOTS);
        let Ok(mut tokens) = self.tokens.lock() else {
            return Vec::new();
        };
        let Ok(mut next) = self.next.lock() else {
            return Vec::new();
        };
        let known: HashMap<PathBuf, String> = tokens
            .iter()
            .map(|(token, (path, _))| (path.clone(), token.clone()))
            .collect();
        let result = shots
            .into_iter()
            .filter_map(|(path, source, modified, size)| {
                let token = if let Some(token) = known.get(&path) {
                    token.clone()
                } else {
                    let token = crate::auth::state::new_state().ok()?;
                    *next += 1;
                    tokens.insert(token.clone(), (path.clone(), *next));
                    token
                };
                Some(Screenshot {
                    url: url(&token),
                    file_name: path
                        .file_name()
                        .map(|n| n.to_string_lossy().into_owned())
                        .unwrap_or_default(),
                    taken_at: chrono::DateTime::<chrono::Utc>::from(modified)
                        .to_rfc3339_opts(chrono::SecondsFormat::Secs, true),
                    size_bytes: i64::try_from(size).unwrap_or(i64::MAX),
                    source,
                })
            })
            .collect();
        // Forget the oldest tokens beyond the limit.
        if tokens.len() > MAX_TOKENS {
            let mut order: Vec<(String, u64)> =
                tokens.iter().map(|(t, (_, n))| (t.clone(), *n)).collect();
            order.sort_by_key(|(_, n)| *n);
            for (token, _) in order.into_iter().take(tokens.len() - MAX_TOKENS) {
                tokens.remove(&token);
            }
        }
        result
    }

    /// The file behind a token or a screenshot URL from [`Self::list`].
    pub fn path(&self, token_or_url: &str) -> Option<PathBuf> {
        let token = token_or_url
            .rsplit('/')
            .next()
            .unwrap_or(token_or_url)
            .trim();
        self.tokens
            .lock()
            .ok()?
            .get(token)
            .map(|(path, _)| path.clone())
    }

    /// The bytes for the `cove-shot` scheme.
    pub fn read(&self, request_path: &str) -> AppResult<(Vec<u8>, &'static str)> {
        let path = self
            .path(request_path)
            .ok_or_else(|| AppError::InvalidInput("unknown screenshot".into()))?;
        Ok((std::fs::read(&path)?, mime(&path)))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db;
    use crate::db::library::tests::sample_game;

    fn write(path: &Path) {
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, b"img").unwrap();
    }

    #[tokio::test]
    async fn steam_game_bar_and_game_folder_shots_with_tokens() {
        let tmp = tempfile::tempdir().unwrap();
        let t = tmp.path();
        let steam = t.join("Steam");
        write(&steam.join("userdata/123/760/remote/620/screenshots/20261101_1.jpg"));
        write(&steam.join("userdata/123/760/remote/620/screenshots/thumbnails/20261101_1.jpg"));
        write(&steam.join("userdata/123/760/remote/999/screenshots/other.jpg"));
        let captures = t.join("Captures");
        write(&captures.join("Portal 2 2026-11-02 10-00-00.png"));
        write(&captures.join("Portal 22 2026-11-02.png"));
        write(&captures.join("notes.txt"));

        let pool = db::open_in_memory().await.unwrap();
        db::library::insert_store_game(
            &pool,
            &sample_game("steam-620", "Portal 2"),
            StoreSource::Steam,
            Some("620"),
            None,
        )
        .await
        .unwrap();
        let item = db::library::require(&pool, "steam-620").await.unwrap();
        let shots = Screenshots::new(vec![steam.clone()], vec![captures.clone()]);
        let list = shots.list(&item);
        let names: Vec<&str> = list.iter().map(|s| s.file_name.as_str()).collect();
        assert_eq!(names.len(), 2, "{names:?}");
        assert!(names.contains(&"20261101_1.jpg"));
        assert!(names.contains(&"Portal 2 2026-11-02 10-00-00.png"));
        let png = list
            .iter()
            .find(|s| s.source == ScreenshotSource::Xbox)
            .unwrap();
        let (bytes, mime) = shots.read(&png.url).unwrap();
        assert_eq!((bytes.as_slice(), mime), (&b"img"[..], "image/png"));
        // The same file keeps its token; unknown tokens and paths are refused.
        assert_eq!(shots.list(&item)[0].url, list[0].url);
        assert!(shots.read("/etc/passwd").is_err());
        assert!(shots.read("deadbeef").is_err());

        // A manual game's own Screenshots folder.
        let folder = t.join("Games/Lantern");
        let exe = folder.join("lantern.x86_64");
        write(&exe);
        write(&folder.join("Screenshots/shot1.png"));
        let manual = db::library::add_manual(
            &pool,
            &crate::models::ManualGameInput {
                title: "Lantern".into(),
                executable_path: exe.to_string_lossy().into_owned(),
                size_bytes: None,
            },
        )
        .await
        .unwrap();
        let list = shots.list(&manual);
        assert_eq!(list.len(), 1);
        assert_eq!(list[0].source, ScreenshotSource::Game);
    }
}
