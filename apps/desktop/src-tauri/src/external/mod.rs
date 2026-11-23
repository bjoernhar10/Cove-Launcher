//! Games from other launchers: Heroic's own ("sideloaded") games, Lutris,
//! Bottles, and on Windows Ubisoft Connect, the EA app, Battle.net and the
//! Xbox app (`windows.rs`).
//!
//! Heroic's store games (GOG, Epic, Amazon) are already found by the
//! connectors' local detection. Here come the games added to Heroic by hand
//! (`sideload_apps/library.json`, settings in `GamesConfig/<app>.json`) and
//! every Wine or Linux game in Lutris (`pga.db`, settings in
//! `games/<config>.yml`). Each keeps its Wine prefix and Wine build, so saves
//! and fixes made there stay; Lutris' playtime comes along. Both launchers'
//! files are only read, never changed. Unreadable files are skipped.

use std::collections::HashSet;
use std::path::{Path, PathBuf};

use serde_json::Value;
use sqlx::sqlite::SqliteConnectOptions;
use sqlx::{Row, SqlitePool};

pub mod windows;

use crate::db;
use crate::error::AppResult;
use crate::models::{
    CompatibilityTool, ExternalGame, ExternalLauncher, LaunchOptions, LibraryUpdate,
    ManualGameInput,
};

/// Where the launchers keep their data.
#[derive(Debug, Clone)]
pub struct LauncherDirs {
    pub heroic: Vec<PathBuf>,
    /// Lutris data folders (with `pga.db`) and config folders (old `games/*.yml`).
    pub lutris_data: Vec<PathBuf>,
    pub lutris_config: Vec<PathBuf>,
    /// Bottles data folders (`bottles/<bottle>/bottle.yml`, `runners/`).
    pub bottles: Vec<PathBuf>,
    /// Drive roots with the Xbox app's `.GamingRoot` (Windows).
    pub drive_roots: Vec<PathBuf>,
}

impl LauncherDirs {
    pub fn for_home(home: Option<&Path>, config: Option<&Path>) -> Self {
        let in_home = |paths: &[&str]| -> Vec<PathBuf> {
            home.map(|h| paths.iter().map(|p| h.join(p)).collect())
                .unwrap_or_default()
        };
        let mut heroic = in_home(&[
            ".config/heroic",
            ".var/app/com.heroicgameslauncher.hgl/config/heroic",
        ]);
        // Windows: %APPDATA%\heroic, macOS: ~/Library/Application Support/heroic.
        if let Some(config) = config {
            let dir = config.join("heroic");
            if !heroic.contains(&dir) {
                heroic.push(dir);
            }
        }
        Self {
            heroic,
            lutris_data: in_home(&[
                ".local/share/lutris",
                ".var/app/net.lutris.Lutris/data/lutris",
            ]),
            lutris_config: in_home(&[".config/lutris", ".var/app/net.lutris.Lutris/config/lutris"]),
            bottles: in_home(&[
                ".local/share/bottles",
                ".var/app/com.usebottles.bottles/data/bottles",
            ]),
            drive_roots: windows::drive_roots(),
        }
    }
}

fn read_json(path: &Path) -> Option<Value> {
    serde_json::from_slice(&std::fs::read(path).ok()?).ok()
}

fn text(value: &Value) -> Option<String> {
    value
        .as_str()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_owned)
}

fn is_windows_exe(path: &str) -> bool {
    Path::new(path)
        .extension()
        .is_some_and(|e| e.eq_ignore_ascii_case("exe"))
}

/// Heroic's sideloaded games.
fn heroic(dirs: &LauncherDirs) -> Vec<ExternalGame> {
    let mut games = Vec::new();
    for dir in &dirs.heroic {
        let Some(library) = read_json(&dir.join("sideload_apps/library.json")) else {
            continue;
        };
        for app in library["games"].as_array().into_iter().flatten() {
            let (Some(app_name), Some(title), Some(exe)) = (
                text(&app["app_name"]),
                text(&app["title"]),
                text(&app["install"]["executable"]),
            ) else {
                continue;
            };
            if app["is_installed"].as_bool() == Some(false) || !Path::new(&exe).is_file() {
                continue;
            }
            let config = read_json(&dir.join("GamesConfig").join(format!("{app_name}.json")));
            let settings = config.as_ref().map(|c| &c[app_name.as_str()]);
            let wine = settings.map(|s| &s["wineVersion"]);
            let windows = is_windows_exe(&exe)
                || text(&app["install"]["platform"])
                    .is_some_and(|p| p.eq_ignore_ascii_case("windows"));
            let proton = wine.and_then(|w| text(&w["type"])).as_deref() == Some("proton");
            games.push(ExternalGame {
                id: format!("heroic:{app_name}"),
                launcher: ExternalLauncher::Heroic,
                title,
                executable_path: exe,
                windows,
                proton: windows && proton,
                wine_prefix: settings
                    .and_then(|s| text(&s["winePrefix"]))
                    .filter(|p| windows && Path::new(p).is_dir()),
                wine_binary: wine
                    .and_then(|w| text(&w["bin"]))
                    .filter(|b| windows && !proton && Path::new(b).is_file()),
                arguments: settings
                    .and_then(|s| text(&s["launcherArgs"]))
                    .unwrap_or_default(),
                playtime_minutes: 0,
                install_path: None,
                launch_url: None,
                in_library: false,
            });
        }
    }
    games
}

/// One Lutris game row.
struct LutrisRow {
    id: i64,
    name: String,
    runner: String,
    directory: Option<String>,
    configpath: Option<String>,
    playtime_hours: f64,
}

async fn lutris_rows(db: &Path) -> Vec<LutrisRow> {
    // Read-only: Lutris may be running and is never changed.
    let options = SqliteConnectOptions::new().filename(db).read_only(true);
    let Ok(pool) = SqlitePool::connect_with(options).await else {
        return Vec::new();
    };
    let rows = sqlx::query(
        "SELECT id, name, runner, directory, configpath, COALESCE(playtime, 0) AS playtime \
         FROM games WHERE installed = 1 AND runner IN ('wine', 'linux')",
    )
    .fetch_all(&pool)
    .await
    .unwrap_or_default();
    pool.close().await;
    rows.into_iter()
        .filter_map(|row| {
            Some(LutrisRow {
                id: row.try_get("id").ok()?,
                name: row.try_get("name").ok()?,
                runner: row.try_get("runner").ok()?,
                directory: row.try_get("directory").ok().flatten(),
                configpath: row.try_get("configpath").ok().flatten(),
                playtime_hours: row.try_get::<f64, _>("playtime").unwrap_or(0.0),
            })
        })
        .collect()
}

/// A game's YAML: `game.exe`, `game.prefix`, `game.args`, `wine.version`.
fn lutris_config(
    dirs: &LauncherDirs,
    data: &Path,
    configpath: &str,
) -> Option<serde_yaml_ng::Value> {
    let name = format!("{configpath}.yml");
    // Newer Lutris keeps them next to pga.db, older ones in the config folder.
    std::iter::once(data.join("games").join(&name))
        .chain(
            dirs.lutris_config
                .iter()
                .map(|c| c.join("games").join(&name)),
        )
        .find_map(|path| serde_yaml_ng::from_slice(&std::fs::read(path).ok()?).ok())
}

fn yaml_text(value: &serde_yaml_ng::Value, section: &str, key: &str) -> Option<String> {
    value
        .get(section)?
        .get(key)?
        .as_str()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_owned)
}

async fn lutris(dirs: &LauncherDirs) -> Vec<ExternalGame> {
    let mut games = Vec::new();
    for data in &dirs.lutris_data {
        let db = data.join("pga.db");
        if !db.is_file() {
            continue;
        }
        for row in lutris_rows(&db).await {
            let Some(config) = row
                .configpath
                .as_deref()
                .and_then(|c| lutris_config(dirs, data, c))
            else {
                continue;
            };
            let Some(exe) = yaml_text(&config, "game", "exe") else {
                continue;
            };
            let prefix = yaml_text(&config, "game", "prefix");
            // Relative paths are relative to the prefix or the game folder.
            let exe = if Path::new(&exe).is_absolute() {
                PathBuf::from(&exe)
            } else {
                [prefix.as_deref(), row.directory.as_deref()]
                    .into_iter()
                    .flatten()
                    .map(|base| Path::new(base).join(&exe))
                    .find(|p| p.is_file())
                    .unwrap_or_else(|| PathBuf::from(&exe))
            };
            if !exe.is_file() {
                continue;
            }
            let wine = row.runner == "wine";
            let wine_binary = yaml_text(&config, "wine", "version")
                .map(|v| data.join("runners/wine").join(v).join("bin/wine"))
                .filter(|b| wine && b.is_file())
                .map(|b| b.to_string_lossy().into_owned());
            games.push(ExternalGame {
                id: format!("lutris:{}", row.id),
                launcher: ExternalLauncher::Lutris,
                title: row.name,
                executable_path: exe.to_string_lossy().into_owned(),
                windows: wine,
                proton: false,
                wine_prefix: prefix.filter(|p| wine && Path::new(p).is_dir()),
                wine_binary,
                arguments: yaml_text(&config, "game", "args").unwrap_or_default(),
                #[allow(clippy::cast_possible_truncation)] // hours of play
                playtime_minutes: (row.playtime_hours.max(0.0) * 60.0).round() as i64,
                install_path: None,
                launch_url: None,
                in_library: false,
            });
        }
    }
    games
}

/// The programs added to Bottles' bottles. A bottle is a plain Wine prefix;
/// its runner is a Wine build in `runners/` (a `sys-` runner is the system's).
fn bottles(dirs: &LauncherDirs) -> Vec<ExternalGame> {
    let mut games = Vec::new();
    for data in &dirs.bottles {
        let Ok(entries) = std::fs::read_dir(data.join("bottles")) else {
            continue;
        };
        for entry in entries.flatten() {
            let folder = entry.path();
            let Some(config) = std::fs::read(folder.join("bottle.yml"))
                .ok()
                .and_then(|b| serde_yaml_ng::from_slice::<serde_yaml_ng::Value>(&b).ok())
            else {
                continue;
            };
            let text = |key: &str| {
                config
                    .get(key)
                    .and_then(serde_yaml_ng::Value::as_str)
                    .map(str::trim)
                    .filter(|s| !s.is_empty())
                    .map(str::to_owned)
            };
            // A bottle with a custom path keeps its prefix there.
            let prefix = text("Path")
                .map(PathBuf::from)
                .filter(|p| p.is_absolute() && p.is_dir())
                .unwrap_or_else(|| folder.clone());
            let bottle = folder
                .file_name()
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_default();
            let wine_binary = text("Runner")
                .filter(|r| !r.starts_with("sys-") && !r.contains(['/', '\\']))
                .map(|r| data.join("runners").join(r).join("bin/wine"))
                .filter(|b| b.is_file())
                .map(|b| b.to_string_lossy().into_owned());
            let Some(programs) = config
                .get("External_Programs")
                .and_then(serde_yaml_ng::Value::as_mapping)
            else {
                continue;
            };
            for (key, program) in programs {
                let field = |name: &str| {
                    program
                        .get(name)
                        .and_then(serde_yaml_ng::Value::as_str)
                        .map(str::trim)
                        .filter(|s| !s.is_empty())
                        .map(str::to_owned)
                };
                let (Some(id), Some(name), Some(exe)) =
                    (key.as_str(), field("name"), field("path"))
                else {
                    continue;
                };
                if program
                    .get("removed")
                    .and_then(serde_yaml_ng::Value::as_bool)
                    == Some(true)
                    || !Path::new(&exe).is_file()
                {
                    continue;
                }
                games.push(ExternalGame {
                    id: format!("bottles:{bottle}/{id}"),
                    launcher: ExternalLauncher::Bottles,
                    title: name,
                    windows: is_windows_exe(&exe),
                    proton: false,
                    wine_prefix: Some(prefix.to_string_lossy().into_owned()),
                    wine_binary: wine_binary.clone(),
                    arguments: field("arguments").unwrap_or_default(),
                    executable_path: exe,
                    playtime_minutes: 0,
                    install_path: None,
                    launch_url: None,
                    in_library: false,
                });
            }
        }
    }
    games
}

/// Every game the other launchers know, marked when it is in the library already.
pub async fn scan(pool: &SqlitePool, dirs: &LauncherDirs) -> AppResult<Vec<ExternalGame>> {
    let file_dirs = dirs.clone();
    let mut games = tokio::task::spawn_blocking(move || {
        let mut games = heroic(&file_dirs);
        games.extend(bottles(&file_dirs));
        games.extend(windows::games(
            &windows::Registry::read(),
            &file_dirs.drive_roots,
        ));
        games
    })
    .await
    .unwrap_or_default();
    games.extend(lutris(dirs).await);
    let known = db::library::executable_paths(pool).await?;
    let mut seen = HashSet::new();
    games.retain(|g| seen.insert(g.executable_path.clone()));
    for game in &mut games {
        game.in_library = known.contains(&game.executable_path);
    }
    games.sort_by_key(|g| g.title.to_lowercase());
    Ok(games)
}

/// Adds the chosen games as manual games with their prefix, Wine build,
/// arguments and playtime. Returns how many were added.
pub async fn add(pool: &SqlitePool, dirs: &LauncherDirs, ids: &[String]) -> AppResult<Vec<String>> {
    let wanted: HashSet<&str> = ids.iter().map(String::as_str).collect();
    let mut added = Vec::new();
    for game in scan(pool, dirs).await? {
        if game.in_library || !wanted.contains(game.id.as_str()) {
            continue;
        }
        let item = db::library::add_manual(
            pool,
            &ManualGameInput {
                title: game.title.clone(),
                executable_path: game.executable_path.clone(),
                size_bytes: None,
            },
        )
        .await?;
        let compatibility_tool = match (game.windows, game.proton) {
            (false, _) => CompatibilityTool::Native,
            (true, true) => CompatibilityTool::Proton,
            (true, false) => CompatibilityTool::Wine,
        };
        let options = LaunchOptions {
            arguments: game.arguments.clone(),
            compatibility_tool,
            wine_prefix: game.wine_prefix.clone(),
            wine_binary: game.wine_binary.clone(),
            launch_url: game.launch_url.clone(),
            ..LaunchOptions::default()
        };
        let patch = LibraryUpdate {
            launch_options: Some(options),
            ..LibraryUpdate::default()
        };
        db::library::update(pool, &item.game.id, &patch).await?;
        if let Some(folder) = &game.install_path {
            // The game's folder, not the executable's (tracking, size, backups).
            sqlx::query("UPDATE library_entries SET install_path = ? WHERE game_id = ?")
                .bind(folder)
                .bind(&item.game.id)
                .execute(pool)
                .await?;
        }
        if game.playtime_minutes > 0 {
            sqlx::query("UPDATE library_entries SET playtime_offset_minutes = ? WHERE game_id = ?")
                .bind(game.playtime_minutes)
                .bind(&item.game.id)
                .execute(pool)
                .await?;
        }
        added.push(item.game.id);
    }
    Ok(added)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn dirs(home: &Path) -> LauncherDirs {
        LauncherDirs {
            drive_roots: Vec::new(),
            ..LauncherDirs::for_home(Some(home), None)
        }
    }

    fn write(path: &Path, text: &str) {
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, text).unwrap();
    }

    async fn lutris_db(path: &Path) {
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        let options = SqliteConnectOptions::new()
            .filename(path)
            .create_if_missing(true);
        let pool = SqlitePool::connect_with(options).await.unwrap();
        sqlx::query(
            "CREATE TABLE games (id INTEGER PRIMARY KEY, name TEXT, slug TEXT, runner TEXT, directory TEXT, \
             installed INTEGER, configpath TEXT, playtime REAL)",
        )
        .execute(&pool)
        .await
        .unwrap();
        for (id, name, runner, config, playtime) in [
            (1, "Lantern", "wine", "lantern-1", 2.5),
            (2, "Tidewater", "linux", "tidewater-2", 0.0),
            (3, "Old Steam", "steam", "steam-3", 1.0),
        ] {
            sqlx::query("INSERT INTO games VALUES (?, ?, '', ?, ?, 1, ?, ?)")
                .bind(id)
                .bind(name)
                .bind(runner)
                .bind(path.parent().unwrap().to_string_lossy())
                .bind(config)
                .bind(playtime)
                .execute(&pool)
                .await
                .unwrap();
        }
        pool.close().await;
    }

    #[tokio::test]
    #[allow(clippy::too_many_lines)] // two launchers set up and imported in one go
    async fn heroic_and_lutris_games_come_with_prefix_wine_and_playtime() {
        let home = tempfile::tempdir().unwrap();
        let h = home.path();
        let s = |p: &Path| p.to_string_lossy().into_owned();

        // Heroic: one Windows game with Wine-GE, one with Proton, one gone.
        let game_exe = h.join("Games/Ember/ember.exe");
        let proton_exe = h.join("Games/Oak/oak.exe");
        let prefix = h.join("Games/Heroic/Prefixes/Ember");
        let wine = h.join(".config/heroic/tools/wine/Wine-GE/bin/wine");
        for file in [&game_exe, &proton_exe, &wine] {
            write(file, "x");
        }
        std::fs::create_dir_all(&prefix).unwrap();
        write(
            &h.join(".config/heroic/sideload_apps/library.json"),
            &serde_json::json!({ "games": [
                { "app_name": "ember1", "title": "Ember", "is_installed": true,
                  "install": { "executable": s(&game_exe), "platform": "Windows" } },
                { "app_name": "oak2", "title": "Oak", "install": { "executable": s(&proton_exe), "platform": "Windows" } },
                { "app_name": "gone3", "title": "Gone", "install": { "executable": "/nowhere/x.exe" } }
            ] })
            .to_string(),
        );
        write(
            &h.join(".config/heroic/GamesConfig/ember1.json"),
            &serde_json::json!({ "ember1": { "winePrefix": s(&prefix), "launcherArgs": "-dx11",
                "wineVersion": { "bin": s(&wine), "name": "Wine-GE", "type": "wine" } } })
            .to_string(),
        );
        write(
            &h.join(".config/heroic/GamesConfig/oak2.json"),
            &serde_json::json!({ "oak2": { "winePrefix": s(&prefix),
                "wineVersion": { "bin": "/p/proton", "name": "GE-Proton", "type": "proton" } } })
            .to_string(),
        );

        // Lutris: a Wine game (exe relative to the prefix), a Linux game, a Steam game.
        let data = h.join(".local/share/lutris");
        lutris_db(&data.join("pga.db")).await;
        let lutris_prefix = h.join("Games/lantern");
        write(&lutris_prefix.join("drive_c/Lantern/lantern.exe"), "x");
        let lutris_wine = data.join("runners/wine/wine-ge-8-26/bin/wine");
        write(&lutris_wine, "x");
        write(
            &data.join("games/lantern-1.yml"),
            &format!(
                "game:\n  exe: drive_c/Lantern/lantern.exe\n  prefix: {}\n  args: --safe\nwine:\n  version: wine-ge-8-26\n",
                s(&lutris_prefix)
            ),
        );
        let tide = h.join("Games/tide/tide.x86_64");
        write(&tide, "x");
        write(
            &h.join(".config/lutris/games/tidewater-2.yml"),
            &format!("game:\n  exe: {}\n", s(&tide)),
        );

        let pool = db::open_in_memory().await.unwrap();
        let found = scan(&pool, &dirs(h)).await.unwrap();
        let titles: Vec<&str> = found.iter().map(|g| g.title.as_str()).collect();
        assert_eq!(titles, ["Ember", "Lantern", "Oak", "Tidewater"]);
        let ember = &found[0];
        assert_eq!(
            ember.wine_prefix.as_deref().map(Path::new),
            Some(prefix.as_path())
        );
        assert_eq!(
            ember.wine_binary.as_deref().map(Path::new),
            Some(wine.as_path())
        );
        assert_eq!((ember.arguments.as_str(), ember.proton), ("-dx11", false));
        assert!(found[2].proton && found[2].wine_binary.is_none());
        let lantern = &found[1];
        assert_eq!(lantern.playtime_minutes, 150);
        assert_eq!(
            lantern.wine_binary.as_deref().map(Path::new),
            Some(lutris_wine.as_path())
        );
        assert!(!found[3].windows && found[3].wine_prefix.is_none());

        let ids: Vec<String> = found.iter().map(|g| g.id.clone()).collect();
        let added = add(&pool, &dirs(h), &ids).await.unwrap();
        assert_eq!(added.len(), 4);
        let library = db::library::list(&pool).await.unwrap();
        let lantern = library.iter().find(|i| i.game.title == "Lantern").unwrap();
        assert_eq!(lantern.entry.playtime_minutes, 150);
        let options = &lantern.entry.launch_options;
        assert_eq!(options.compatibility_tool, CompatibilityTool::Wine);
        assert_eq!(
            options.wine_prefix.as_deref().map(Path::new),
            Some(lutris_prefix.as_path())
        );
        assert_eq!(options.arguments, "--safe");
        let oak = library.iter().find(|i| i.game.title == "Oak").unwrap();
        assert_eq!(
            oak.entry.launch_options.compatibility_tool,
            CompatibilityTool::Proton
        );

        // A second scan marks them; adding again adds nothing.
        assert!(scan(&pool, &dirs(h))
            .await
            .unwrap()
            .iter()
            .all(|g| g.in_library));
        assert_eq!(
            add(&pool, &dirs(h), &ids).await.unwrap(),
            Vec::<String>::new()
        );
    }

    #[tokio::test]
    async fn bottles_and_xbox_games_import_with_prefix_and_link() {
        let home = tempfile::tempdir().unwrap();
        let h = home.path();
        let s = |p: &Path| p.to_string_lossy().into_owned();

        // Bottles: a bottle with its own runner, one removed program, one missing.
        let data = h.join(".local/share/bottles");
        let bottle = data.join("bottles/Gaming");
        let exe = bottle.join("drive_c/Games/Fern/fern.exe");
        write(&exe, "x");
        let runner = data.join("runners/soda-7.0-9/bin/wine");
        write(&runner, "x");
        write(
            &bottle.join("bottle.yml"),
            &format!(
                "Name: Gaming\nPath: Gaming\nRunner: soda-7.0-9\nExternal_Programs:\n  a1:\n    name: Fern\n    path: {}\n    arguments: -windowed\n  b2:\n    name: Removed\n    path: {}\n    removed: true\n  c3:\n    name: Missing\n    path: /nowhere/x.exe\n",
                s(&exe),
                s(&exe)
            ),
        );

        // Xbox app on a "drive".
        let drive = h.join("drive");
        let mut root_file = b"RGBX".to_vec();
        root_file.extend(1u32.to_le_bytes());
        root_file.extend("XboxGames".encode_utf16().flat_map(u16::to_le_bytes));
        root_file.extend([0, 0]);
        std::fs::create_dir_all(&drive).unwrap();
        std::fs::write(drive.join(".GamingRoot"), root_file).unwrap();
        let game = drive.join("XboxGames/Pentiment");
        write(&game.join("Content/Pentiment.exe"), "x");
        write(
            &game.join("Content/MicrosoftGame.config"),
            r#"<Game><Identity Name="Obsidian.Pentiment" Publisher="CN=Microsoft Corporation, O=Microsoft Corporation, L=Redmond, S=Washington, C=US"/>
<Executable Name="Pentiment.exe" Id="Game"/><ShellVisuals DefaultDisplayName="Pentiment"/></Game>"#,
        );

        let dirs = LauncherDirs {
            drive_roots: vec![drive.clone()],
            ..LauncherDirs::for_home(Some(h), None)
        };
        let pool = db::open_in_memory().await.unwrap();
        let found = scan(&pool, &dirs).await.unwrap();
        let ids: Vec<&str> = found.iter().map(|g| g.id.as_str()).collect();
        assert_eq!(ids, ["bottles:Gaming/a1", "xbox:Obsidian.Pentiment"]);
        let fern = &found[0];
        assert_eq!(
            fern.wine_prefix.as_deref().map(Path::new),
            Some(bottle.as_path())
        );
        assert_eq!(
            fern.wine_binary.as_deref().map(Path::new),
            Some(runner.as_path())
        );
        assert_eq!((fern.arguments.as_str(), fern.windows), ("-windowed", true));

        let ids: Vec<String> = found.iter().map(|g| g.id.clone()).collect();
        assert_eq!(add(&pool, &dirs, &ids).await.unwrap().len(), 2);
        let library = db::library::list(&pool).await.unwrap();
        let pentiment = library
            .iter()
            .find(|i| i.game.title == "Pentiment")
            .unwrap();
        assert_eq!(
            pentiment.entry.launch_options.launch_url.as_deref(),
            Some("shell:AppsFolder\\Obsidian.Pentiment_8wekyb3d8bbwe!Game")
        );
        assert_eq!(
            pentiment.entry.launch_via,
            Some(crate::models::LaunchVia::XboxApp)
        );
        assert_eq!(
            pentiment.entry.install_path.as_deref().map(Path::new),
            Some(game.as_path())
        );
        let fern = library.iter().find(|i| i.game.title == "Fern").unwrap();
        assert_eq!(
            fern.entry.launch_options.compatibility_tool,
            CompatibilityTool::Wine
        );
        assert_eq!(fern.entry.launch_options.launch_url, None);
    }
}
