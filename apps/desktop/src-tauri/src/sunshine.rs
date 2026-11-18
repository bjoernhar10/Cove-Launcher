//! Streaming with Sunshine and Moonlight: Cove writes the installed games into
//! Sunshine's app list (`apps.json`), so Moonlight on a phone, TV or another
//! computer shows them with their covers.
//!
//! Every entry starts Cove with `--launch <game id>` (like a desktop shortcut)
//! as a detached command: Cove hands the start to the running Cove and the
//! stream stays until it is ended in Moonlight. Cove's entries carry the key
//! `cove-game`; Cove only ever changes or removes those and keeps everything
//! else in the file. Before the first change it keeps a copy of the file
//! (`apps.json.cove-backup`). Sunshine reads the list when it starts.

use std::path::{Path, PathBuf};

use serde_json::{Map, Value};

use crate::error::{AppError, AppResult};
use crate::models::SunshineStatus;
use crate::shortcuts::{CoveCommand, LAUNCH_ARG};

/// Marks Cove's entries in `apps.json`.
const MARKER: &str = "cove-game";
/// Flatpak id of Sunshine (its config lives in the sandbox's folder).
const FLATPAK_ID: &str = "dev.lizardbyte.app.Sunshine";
/// Box art size Moonlight shows (portrait).
const COVER_WIDTH: u32 = 600;
const COVER_HEIGHT: u32 = 800;

/// A game for Sunshine's list.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StreamGame {
    pub id: String,
    pub title: String,
    /// The cover's bytes (any format the image cache holds).
    pub cover: Option<Vec<u8>>,
}

/// One place Sunshine may keep its configuration.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConfigDir {
    pub path: PathBuf,
    /// Sunshine runs as a Flatpak: its commands need `flatpak-spawn --host`.
    pub flatpak: bool,
}

pub struct Sunshine {
    dirs: Vec<ConfigDir>,
    command: Option<CoveCommand>,
}

/// Sunshine's configuration folders on this system, most likely first.
pub fn config_dirs(home: Option<&Path>) -> Vec<ConfigDir> {
    let mut dirs = Vec::new();
    if cfg!(windows) {
        for var in ["ProgramFiles", "ProgramW6432"] {
            if let Some(base) = std::env::var_os(var) {
                let path = PathBuf::from(base).join("Sunshine").join("config");
                if !dirs.iter().any(|d: &ConfigDir| d.path == path) {
                    dirs.push(ConfigDir {
                        path,
                        flatpak: false,
                    });
                }
            }
        }
        return dirs;
    }
    if let Some(home) = home {
        dirs.push(ConfigDir {
            path: home.join(".config/sunshine"),
            flatpak: false,
        });
        if cfg!(target_os = "linux") {
            dirs.push(ConfigDir {
                path: home
                    .join(".var/app")
                    .join(FLATPAK_ID)
                    .join("config/sunshine"),
                flatpak: true,
            });
        }
    }
    dirs
}

/// The app list file: `file_apps` from `sunshine.conf`, else `apps.json`.
fn apps_file(dir: &Path) -> PathBuf {
    let configured = std::fs::read_to_string(dir.join("sunshine.conf"))
        .ok()
        .and_then(|text| {
            text.lines().find_map(|line| {
                let (key, value) = line.split_once('=')?;
                (key.trim() == "file_apps")
                    .then(|| value.trim().to_owned())
                    .filter(|v| !v.is_empty())
            })
        });
    match configured {
        Some(path) if Path::new(&path).is_absolute() => PathBuf::from(path),
        Some(path) => dir.join(path),
        None => dir.join("apps.json"),
    }
}

/// One argument of Sunshine's command line (quoted when it has spaces).
fn quote(arg: &str) -> String {
    if arg.is_empty() || arg.contains([' ', '\t']) {
        format!("\"{arg}\"")
    } else {
        arg.to_owned()
    }
}

/// The command that starts the game through Cove.
fn command_line(command: &CoveCommand, game_id: &str, flatpak: bool) -> String {
    let mut parts: Vec<String> = Vec::new();
    if flatpak {
        parts.extend(["flatpak-spawn".into(), "--host".into()]);
    }
    parts.push(command.program.clone());
    parts.extend(command.args.iter().cloned());
    parts.push(LAUNCH_ARG.into());
    parts.push(game_id.into());
    parts.iter().map(|a| quote(a)).collect::<Vec<_>>().join(" ")
}

fn is_ours(app: &Value) -> bool {
    app.get(MARKER).is_some_and(Value::is_string)
}

/// The file's app list with Cove's entries replaced by `entries`.
fn merge(mut root: Value, entries: Vec<Value>) -> AppResult<Value> {
    let object = root.as_object_mut().ok_or_else(unreadable)?;
    let apps = object
        .entry("apps")
        .or_insert_with(|| Value::Array(Vec::new()))
        .as_array_mut()
        .ok_or_else(unreadable)?;
    apps.retain(|app| !is_ours(app));
    apps.extend(entries);
    Ok(root)
}

fn unreadable() -> AppError {
    AppError::InvalidInput(tr!(
        "Sunshines App-Liste ist nicht lesbar. Cove ändert sie deshalb nicht.",
        "Sunshine's app list can't be read, so Cove leaves it alone."
    ))
}

fn write_error(error: &std::io::Error) -> AppError {
    eprintln!("[sunshine] apps.json: {error}");
    if error.kind() == std::io::ErrorKind::PermissionDenied {
        AppError::InvalidInput(tr!(
            "Cove darf Sunshines App-Liste nicht ändern. Unter Windows hilft es, Cove einmal als Administrator zu starten.",
            "Cove isn't allowed to change Sunshine's app list. On Windows, start Cove once as administrator."
        ))
    } else {
        AppError::Io(std::io::Error::new(error.kind(), error.to_string()))
    }
}

/// The cover as Moonlight's portrait box art (cut to 3:4).
fn write_cover(bytes: &[u8], path: &Path) -> Result<(), String> {
    let image = image::load_from_memory(bytes).map_err(|e| e.to_string())?;
    let art = image
        .resize_to_fill(
            COVER_WIDTH,
            COVER_HEIGHT,
            image::imageops::FilterType::Lanczos3,
        )
        .to_rgb8();
    art.save_with_format(path, image::ImageFormat::Png)
        .map_err(|e| e.to_string())
}

fn read_list(file: &Path) -> AppResult<Value> {
    match std::fs::read_to_string(file) {
        Ok(text) => serde_json::from_str(&text).map_err(|_| unreadable()),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(serde_json::json!({
            "env": {},
            "apps": []
        })),
        Err(e) => Err(write_error(&e)),
    }
}

/// Writes the list next to the old one first, then replaces it.
fn write_list(file: &Path, root: &Value) -> AppResult<()> {
    let backup = file.with_extension("json.cove-backup");
    if file.is_file() && !backup.exists() {
        std::fs::copy(file, &backup).map_err(|e| write_error(&e))?;
    }
    let text = serde_json::to_string_pretty(root)?;
    let temp = file.with_extension("json.cove-tmp");
    std::fs::write(&temp, text).map_err(|e| write_error(&e))?;
    std::fs::rename(&temp, file).map_err(|e| {
        let _ = std::fs::remove_file(&temp);
        write_error(&e)
    })
}

fn count_ours(root: &Value) -> u32 {
    root.get("apps")
        .and_then(Value::as_array)
        .map_or(0, |apps| {
            u32::try_from(apps.iter().filter(|a| is_ours(a)).count()).unwrap_or(u32::MAX)
        })
}

impl Sunshine {
    pub fn new(dirs: Vec<ConfigDir>, command: Option<CoveCommand>) -> Self {
        Self { dirs, command }
    }

    /// The first folder where Sunshine is set up.
    fn found(&self) -> Option<&ConfigDir> {
        self.dirs
            .iter()
            .find(|d| d.path.join("apps.json").is_file() || d.path.join("sunshine.conf").is_file())
    }

    fn require(&self) -> AppResult<&ConfigDir> {
        self.found().ok_or_else(|| {
            AppError::InvalidInput(tr!(
                "Sunshine wurde nicht gefunden. Installiere es und starte es einmal, dann versuche es noch einmal.",
                "Sunshine wasn't found. Install it and start it once, then try again."
            ))
        })
    }

    pub fn status(&self) -> SunshineStatus {
        let Some(dir) = self.found() else {
            return SunshineStatus {
                found: false,
                apps_file: None,
                games: 0,
            };
        };
        let file = apps_file(&dir.path);
        let games = read_list(&file).map_or(0, |root| count_ours(&root));
        SunshineStatus {
            found: true,
            apps_file: Some(file.to_string_lossy().into_owned()),
            games,
        }
    }

    /// Puts `games` into Sunshine's list (Cove's earlier entries are replaced).
    pub fn sync(&self, games: &[StreamGame]) -> AppResult<SunshineStatus> {
        let dir = self.require()?;
        let command = self.command.as_ref().ok_or_else(|| {
            AppError::InvalidInput(tr!(
                "Cove findet sein eigenes Programm nicht und kann Sunshine deshalb keine Spiele geben.",
                "Cove can't find its own program, so it can't give Sunshine any games."
            ))
        })?;
        let file = apps_file(&dir.path);
        let root = read_list(&file)?;
        let covers = dir.path.join("covers");
        let old = covers_of(&root);
        let mut entries = Vec::with_capacity(games.len());
        for game in games {
            let mut app = Map::new();
            app.insert("name".into(), Value::String(game.title.clone()));
            app.insert(
                "detached".into(),
                Value::Array(vec![Value::String(command_line(
                    command,
                    &game.id,
                    dir.flatpak,
                ))]),
            );
            if let Some(bytes) = &game.cover {
                let path = covers.join(format!("cove-{}.png", game.id));
                let written = std::fs::create_dir_all(&covers)
                    .map_err(|e| e.to_string())
                    .and_then(|()| write_cover(bytes, &path));
                match written {
                    Ok(()) => {
                        app.insert(
                            "image-path".into(),
                            Value::String(path.to_string_lossy().into_owned()),
                        );
                    }
                    Err(e) => eprintln!("[sunshine] cover of {}: {e}", game.id),
                }
            }
            app.insert(MARKER.into(), Value::String(game.id.clone()));
            entries.push(Value::Object(app));
        }
        let merged = merge(root, entries)?;
        if let Some(parent) = file.parent() {
            std::fs::create_dir_all(parent).map_err(|e| write_error(&e))?;
        }
        write_list(&file, &merged)?;
        remove_unused_covers(&old, &covers_of(&merged));
        Ok(SunshineStatus {
            found: true,
            apps_file: Some(file.to_string_lossy().into_owned()),
            games: count_ours(&merged),
        })
    }

    /// Takes Cove's entries (and their covers) out of Sunshine's list.
    pub fn remove(&self) -> AppResult<SunshineStatus> {
        let dir = self.require()?;
        let file = apps_file(&dir.path);
        if !file.is_file() {
            return Ok(self.status());
        }
        let root = read_list(&file)?;
        let old = covers_of(&root);
        let merged = merge(root, Vec::new())?;
        write_list(&file, &merged)?;
        remove_unused_covers(&old, &[]);
        Ok(SunshineStatus {
            found: true,
            apps_file: Some(file.to_string_lossy().into_owned()),
            games: 0,
        })
    }
}

/// The cover files of Cove's entries.
fn covers_of(root: &Value) -> Vec<PathBuf> {
    root.get("apps")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter(|app| is_ours(app))
        .filter_map(|app| app.get("image-path")?.as_str().map(PathBuf::from))
        .collect()
}

/// Deletes covers Cove wrote that no entry uses anymore.
fn remove_unused_covers(old: &[PathBuf], kept: &[PathBuf]) {
    for path in old {
        let cove_file = path
            .file_name()
            .and_then(|n| n.to_str())
            .is_some_and(|n| n.starts_with("cove-"))
            && path.extension().is_some_and(|e| e == "png");
        if cove_file && !kept.contains(path) {
            let _ = std::fs::remove_file(path);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cove() -> CoveCommand {
        CoveCommand {
            program: "/opt/Cove Launcher/cove".into(),
            args: Vec::new(),
        }
    }

    fn png() -> Vec<u8> {
        let mut bytes = Vec::new();
        image::RgbImage::from_pixel(60, 90, image::Rgb([40, 60, 90]))
            .write_to(
                &mut std::io::Cursor::new(&mut bytes),
                image::ImageFormat::Png,
            )
            .unwrap();
        bytes
    }

    fn game(id: &str, title: &str, cover: bool) -> StreamGame {
        StreamGame {
            id: id.into(),
            title: title.into(),
            cover: cover.then(png),
        }
    }

    #[test]
    fn commands_quote_paths_with_spaces_and_go_through_the_host_for_flatpak() {
        assert_eq!(
            command_line(&cove(), "manual-1", false),
            "\"/opt/Cove Launcher/cove\" --launch manual-1"
        );
        let flatpak = CoveCommand {
            program: "flatpak".into(),
            args: vec!["run".into(), "io.github.bjoernhar10.Cove_Launcher".into()],
        };
        assert_eq!(
            command_line(&flatpak, "steam-620", true),
            "flatpak-spawn --host flatpak run io.github.bjoernhar10.Cove_Launcher --launch steam-620"
        );
    }

    #[test]
    fn the_apps_file_follows_sunshine_conf() {
        let tmp = tempfile::tempdir().unwrap();
        assert_eq!(apps_file(tmp.path()), tmp.path().join("apps.json"));
        std::fs::write(
            tmp.path().join("sunshine.conf"),
            "port = 47989\nfile_apps = my-apps.json\n",
        )
        .unwrap();
        assert_eq!(apps_file(tmp.path()), tmp.path().join("my-apps.json"));
    }

    #[test]
    fn sync_keeps_other_apps_replaces_coves_and_removes_them_again() {
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path().join("sunshine");
        std::fs::create_dir_all(&dir).unwrap();
        let file = dir.join("apps.json");
        let original = serde_json::json!({
            "env": { "PATH": "$(PATH):$(HOME)/.local/bin" },
            "apps": [
                { "name": "Desktop", "image-path": "desktop.png" },
                { "name": "Old", "detached": ["cove --launch old"], "cove-game": "old" }
            ]
        });
        std::fs::write(&file, original.to_string()).unwrap();
        let sunshine = Sunshine::new(
            vec![
                ConfigDir {
                    path: tmp.path().join("missing"),
                    flatpak: false,
                },
                ConfigDir {
                    path: dir.clone(),
                    flatpak: false,
                },
            ],
            Some(cove()),
        );
        assert_eq!(sunshine.status().games, 1);

        let status = sunshine
            .sync(&[
                game("manual-1", "Ember", true),
                game("steam-620", "Tide", false),
            ])
            .unwrap();
        assert_eq!(status.games, 2);
        let root: Value = serde_json::from_str(&std::fs::read_to_string(&file).unwrap()).unwrap();
        let apps = root["apps"].as_array().unwrap();
        let names: Vec<&str> = apps.iter().map(|a| a["name"].as_str().unwrap()).collect();
        assert_eq!(names, ["Desktop", "Ember", "Tide"]);
        assert_eq!(root["env"]["PATH"], "$(PATH):$(HOME)/.local/bin");
        assert_eq!(
            apps[1]["detached"][0],
            "\"/opt/Cove Launcher/cove\" --launch manual-1"
        );
        let cover = PathBuf::from(apps[1]["image-path"].as_str().unwrap());
        let art = image::open(&cover).unwrap();
        assert_eq!((art.width(), art.height()), (COVER_WIDTH, COVER_HEIGHT));
        assert!(apps[2].get("image-path").is_none());
        // The file as it was before Cove's first change is kept.
        let backup = std::fs::read_to_string(dir.join("apps.json.cove-backup")).unwrap();
        assert_eq!(serde_json::from_str::<Value>(&backup).unwrap(), original);

        let status = sunshine.remove().unwrap();
        assert_eq!(status.games, 0);
        let root: Value = serde_json::from_str(&std::fs::read_to_string(&file).unwrap()).unwrap();
        assert_eq!(root["apps"].as_array().unwrap().len(), 1);
        assert!(!cover.exists());
    }

    #[test]
    fn an_unreadable_list_is_left_alone() {
        let tmp = tempfile::tempdir().unwrap();
        std::fs::write(tmp.path().join("apps.json"), "{ not json").unwrap();
        let sunshine = Sunshine::new(
            vec![ConfigDir {
                path: tmp.path().to_path_buf(),
                flatpak: false,
            }],
            Some(cove()),
        );
        assert!(sunshine.sync(&[game("a", "A", false)]).is_err());
        assert_eq!(
            std::fs::read_to_string(tmp.path().join("apps.json")).unwrap(),
            "{ not json"
        );
    }

    #[test]
    fn without_sunshine_nothing_is_written() {
        let tmp = tempfile::tempdir().unwrap();
        let sunshine = Sunshine::new(
            vec![ConfigDir {
                path: tmp.path().join("sunshine"),
                flatpak: false,
            }],
            Some(cove()),
        );
        assert!(!sunshine.status().found);
        assert!(sunshine.sync(&[game("a", "A", false)]).is_err());
        assert!(!tmp.path().join("sunshine").exists());
    }
}
