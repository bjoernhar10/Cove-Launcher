//! Shortcuts to a game on the desktop and in the app menu.
//!
//! A shortcut starts Cove with `--launch <game id>` (Flatpak: `flatpak run
//! <app id> --launch …`), so playtime, saves and Proton work as from the play
//! button. A running Cove gets the request through the single-instance plugin
//! (`lib.rs`), a closed one starts and then launches the game.
//!
//! Linux: a `.desktop` file (menu: `~/.local/share/applications`). Windows: a
//! `.lnk` (menu: Start menu → Programs → Cove). macOS: a tiny `.app` whose
//! script starts Cove (menu: `~/Applications/Cove`). The icon is the game's
//! cover, cut to a square. Cove remembers what it created (`shortcuts` table)
//! and only ever deletes a file that still carries its own marker.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use sqlx::{Row, SqlitePool};

use crate::error::{AppError, AppResult};
use crate::models::{GameShortcut, LibraryItem, ShortcutPlace};

/// The command line argument a shortcut passes.
pub const LAUNCH_ARG: &str = "--launch";
/// Icon size in pixels (square).
const ICON_SIZE: u32 = 256;
/// Written into every shortcut, so Cove only removes its own files.
const MARKER: &str = "X-Cove-Game";
/// macOS bundle ids of game shortcuts start with this.
const BUNDLE_PREFIX: &str = "app.cove.desktop.game.";

/// How a shortcut starts Cove.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CoveCommand {
    pub program: String,
    pub args: Vec<String>,
}

impl CoveCommand {
    /// This Cove: the Flatpak, the `AppImage` or the executable.
    pub fn current() -> Option<Self> {
        if crate::flatpak::active() {
            let id = std::env::var("FLATPAK_ID").ok()?;
            return Some(Self {
                program: "flatpak".into(),
                args: vec!["run".into(), id],
            });
        }
        let program = std::env::var_os("APPIMAGE")
            .map(PathBuf::from)
            .or_else(|| std::env::current_exe().ok())?;
        Some(Self {
            program: program.to_string_lossy().into_owned(),
            args: Vec::new(),
        })
    }
}

/// Where shortcuts go; `None` when this system has no such place.
#[derive(Debug, Clone)]
pub struct ShortcutDirs {
    pub desktop: Option<PathBuf>,
    pub menu: Option<PathBuf>,
    /// Cove's own folder for the converted icons.
    pub icons: PathBuf,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    Desktop,
    Lnk,
    App,
}

impl Kind {
    pub const fn native() -> Self {
        if cfg!(windows) {
            Self::Lnk
        } else if cfg!(target_os = "macos") {
            Self::App
        } else {
            Self::Desktop
        }
    }
}

pub struct Shortcuts {
    pool: SqlitePool,
    dirs: ShortcutDirs,
    command: Option<CoveCommand>,
    kind: Kind,
}

/// The game id from Cove's command line (`--launch <id>` or `--launch=<id>`).
pub fn launch_arg(args: &[String]) -> Option<String> {
    let mut iter = args.iter();
    while let Some(arg) = iter.next() {
        if arg == LAUNCH_ARG {
            return iter.next().filter(|id| valid_id(id)).cloned();
        }
        if let Some(id) = arg.strip_prefix("--launch=") {
            return valid_id(id).then(|| id.to_owned());
        }
    }
    None
}

/// Game ids go into file names and command lines: only plain characters.
fn valid_id(id: &str) -> bool {
    !id.is_empty()
        && id.len() <= 200
        && !id.starts_with('-')
        && id
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.' | ':'))
}

fn place_str(place: ShortcutPlace) -> &'static str {
    match place {
        ShortcutPlace::Desktop => "desktop",
        ShortcutPlace::Menu => "menu",
    }
}

impl Shortcuts {
    pub fn new(
        pool: SqlitePool,
        dirs: ShortcutDirs,
        command: Option<CoveCommand>,
        kind: Kind,
    ) -> Self {
        Self {
            pool,
            dirs,
            command,
            kind,
        }
    }

    /// The places this system offers.
    pub fn places(&self) -> Vec<ShortcutPlace> {
        let mut places = Vec::new();
        if self.dirs.desktop.is_some() {
            places.push(ShortcutPlace::Desktop);
        }
        if self.dirs.menu.is_some() {
            places.push(ShortcutPlace::Menu);
        }
        places
    }

    /// The game's shortcuts that still exist (rows of deleted files are dropped).
    pub async fn list(&self, game_id: &str) -> AppResult<Vec<GameShortcut>> {
        let rows =
            sqlx::query("SELECT place, path FROM shortcuts WHERE game_id = ? ORDER BY place")
                .bind(game_id)
                .fetch_all(&self.pool)
                .await?;
        let mut shortcuts = Vec::new();
        for row in rows {
            let place: String = row.try_get("place")?;
            let path: String = row.try_get("path")?;
            let place = match place.as_str() {
                "desktop" => ShortcutPlace::Desktop,
                _ => ShortcutPlace::Menu,
            };
            if Path::new(&path).exists() {
                shortcuts.push(GameShortcut { place, path });
            } else {
                self.forget(game_id, place).await?;
            }
        }
        Ok(shortcuts)
    }

    /// Creates (or replaces) the game's shortcut in `place`. `cover` is the
    /// cover image's bytes; without one the shortcut gets Cove's icon.
    pub async fn create(
        &self,
        item: &LibraryItem,
        place: ShortcutPlace,
        cover: Option<Vec<u8>>,
    ) -> AppResult<GameShortcut> {
        let game_id = item.game.id.clone();
        if !valid_id(&game_id) {
            return Err(AppError::InvalidInput(tr!(
                "Für dieses Spiel kann Cove keine Verknüpfung anlegen.",
                "Cove can't create a shortcut for this game."
            )));
        }
        let command = self.command.clone().ok_or_else(|| {
            AppError::InvalidInput(tr!(
                "Cove findet sein eigenes Programm nicht und kann deshalb keine Verknüpfung anlegen.",
                "Cove can't find its own program, so it can't create a shortcut."
            ))
        })?;
        let dir = match place {
            ShortcutPlace::Desktop => self.dirs.desktop.clone(),
            ShortcutPlace::Menu => self.dirs.menu.clone(),
        }
        .ok_or_else(|| {
            AppError::InvalidInput(tr!(
                "Diesen Ort gibt es auf diesem System nicht.",
                "This place doesn't exist on this system."
            ))
        })?;
        // An old shortcut of this game in this place goes first (the title may have changed).
        self.remove(&game_id, place).await?;

        let title = item.game.title.clone();
        let kind = self.kind;
        let icons = self.dirs.icons.clone();
        let path = tokio::task::spawn_blocking(move || -> AppResult<PathBuf> {
            std::fs::create_dir_all(&dir)?;
            let icon = cover.and_then(|bytes| {
                write_icon(&bytes, &icons, &game_id, kind)
                    .map_err(|e| eprintln!("[shortcuts] icon for {game_id}: {e}"))
                    .ok()
            });
            let path = shortcut_path(&dir, &title, &game_id, place, kind);
            if path.exists() && !is_ours(&path, kind) {
                return Err(AppError::InvalidInput(tr!(
                    "Dort liegt schon eine Datei namens „{}“.",
                    "There already is a file called \"{}\" there.",
                    path.file_name().unwrap_or_default().to_string_lossy()
                )));
            }
            match kind {
                Kind::Desktop => {
                    write_desktop_file(&path, &title, &game_id, &command, icon.as_deref())?;
                }
                Kind::Lnk => write_lnk(&path, &title, &game_id, &command, icon.as_deref())?,
                Kind::App => write_app(&path, &title, &game_id, &command, icon.as_deref())?,
            }
            Ok(path)
        })
        .await
        .map_err(|e| AppError::Io(std::io::Error::other(e.to_string())))??;

        let path = path.to_string_lossy().into_owned();
        sqlx::query(
            "INSERT INTO shortcuts (game_id, place, path, created_at) VALUES (?, ?, ?, ?)
             ON CONFLICT (game_id, place) DO UPDATE SET path = excluded.path, created_at = excluded.created_at",
        )
        .bind(&item.game.id)
        .bind(place_str(place))
        .bind(&path)
        .bind(chrono::Utc::now().to_rfc3339())
        .execute(&self.pool)
        .await?;
        Ok(GameShortcut { place, path })
    }

    /// Removes the game's shortcut in `place` (only a file Cove wrote).
    pub async fn remove(&self, game_id: &str, place: ShortcutPlace) -> AppResult<()> {
        let path: Option<String> =
            sqlx::query_scalar("SELECT path FROM shortcuts WHERE game_id = ? AND place = ?")
                .bind(game_id)
                .bind(place_str(place))
                .fetch_optional(&self.pool)
                .await?;
        if let Some(path) = path {
            let kind = self.kind;
            let icons = self.dirs.icons.clone();
            let id = game_id.to_owned();
            let others = self.list(game_id).await?.len();
            tokio::task::spawn_blocking(move || {
                let path = PathBuf::from(path);
                if is_ours(&path, kind) {
                    let removed = if kind == Kind::App {
                        std::fs::remove_dir_all(&path)
                    } else {
                        std::fs::remove_file(&path)
                    };
                    if let Err(error) = removed {
                        eprintln!("[shortcuts] {}: {error}", path.display());
                    }
                }
                // The icon is shared by both places: gone with the last shortcut.
                if others <= 1 {
                    for ext in ["png", "ico", "icns"] {
                        let _ = std::fs::remove_file(icons.join(format!("{id}.{ext}")));
                    }
                }
            })
            .await
            .map_err(|e| AppError::Io(std::io::Error::other(e.to_string())))?;
        }
        self.forget(game_id, place).await
    }

    /// Removes all shortcuts of a game (it left the library).
    pub async fn remove_all(&self, game_id: &str) {
        for place in [ShortcutPlace::Desktop, ShortcutPlace::Menu] {
            if let Err(error) = self.remove(game_id, place).await {
                eprintln!("[shortcuts] {game_id}: {error}");
            }
        }
    }

    async fn forget(&self, game_id: &str, place: ShortcutPlace) -> AppResult<()> {
        sqlx::query("DELETE FROM shortcuts WHERE game_id = ? AND place = ?")
            .bind(game_id)
            .bind(place_str(place))
            .execute(&self.pool)
            .await?;
        Ok(())
    }
}

fn shortcut_path(
    dir: &Path,
    title: &str,
    game_id: &str,
    place: ShortcutPlace,
    kind: Kind,
) -> PathBuf {
    let name = crate::folders::folder_name(title);
    let name = if name.is_empty() {
        game_id.to_owned()
    } else {
        name
    };
    match (kind, place) {
        // Menu entries are found by their Name; the file name only has to be unique.
        (Kind::Desktop, ShortcutPlace::Menu) => {
            dir.join(format!("cove-{}.desktop", game_id.replace(':', "-")))
        }
        (Kind::Desktop, ShortcutPlace::Desktop) => dir.join(format!("{name}.desktop")),
        (Kind::Lnk, _) => dir.join(format!("{name}.lnk")),
        (Kind::App, _) => dir.join(format!("{name}.app")),
    }
}

/// Whether a file at `path` is a shortcut Cove wrote.
fn is_ours(path: &Path, kind: Kind) -> bool {
    match kind {
        Kind::Desktop => std::fs::read_to_string(path)
            .is_ok_and(|text| text.lines().any(|l| l.starts_with(&format!("{MARKER}=")))),
        Kind::App => std::fs::read_to_string(path.join("Contents/Info.plist"))
            .is_ok_and(|text| text.contains(BUNDLE_PREFIX)),
        // A .lnk keeps its arguments; Cove's always contain the launch argument.
        Kind::Lnk => std::fs::read(path).is_ok_and(|bytes| {
            let wanted: Vec<u8> = LAUNCH_ARG
                .encode_utf16()
                .flat_map(u16::to_le_bytes)
                .collect();
            bytes.windows(wanted.len()).any(|w| w == wanted.as_slice())
        }),
    }
}

/// The cover as a square icon next to Cove's data: PNG (Linux), ICO (Windows), ICNS (macOS).
fn write_icon(cover: &[u8], icons: &Path, game_id: &str, kind: Kind) -> Result<PathBuf, String> {
    let image = image::load_from_memory(cover).map_err(|e| e.to_string())?;
    let side = image.width().min(image.height());
    // Covers are portrait: keep the upper middle, where titles and faces usually are.
    let x = (image.width() - side) / 2;
    let y = (image.height() - side) / 4;
    let square = image
        .crop_imm(x, y, side, side)
        .resize_exact(ICON_SIZE, ICON_SIZE, image::imageops::FilterType::Lanczos3)
        .to_rgba8();
    std::fs::create_dir_all(icons).map_err(|e| e.to_string())?;
    let ext = match kind {
        Kind::Desktop => "png",
        Kind::Lnk => "ico",
        Kind::App => "icns",
    };
    let path = icons.join(format!("{game_id}.{ext}"));
    match kind {
        Kind::Desktop => square.save_with_format(&path, image::ImageFormat::Png),
        Kind::Lnk => square.save_with_format(&path, image::ImageFormat::Ico),
        Kind::App => return write_icns(&square, &path).map(|()| path),
    }
    .map_err(|e| e.to_string())?;
    Ok(path)
}

#[cfg(target_os = "macos")]
fn write_icns(square: &image::RgbaImage, path: &Path) -> Result<(), String> {
    let image = icns::Image::from_data(
        icns::PixelFormat::RGBA,
        square.width(),
        square.height(),
        square.as_raw().clone(),
    )
    .map_err(|e| e.to_string())?;
    let mut family = icns::IconFamily::new();
    family.add_icon(&image).map_err(|e| e.to_string())?;
    let file = std::fs::File::create(path).map_err(|e| e.to_string())?;
    family
        .write(std::io::BufWriter::new(file))
        .map_err(|e| e.to_string())
}

#[cfg(not(target_os = "macos"))]
fn write_icns(_square: &image::RgbaImage, _path: &Path) -> Result<(), String> {
    Err("ICNS icons are only written on macOS".into())
}

/// One argument of a desktop entry's `Exec` key (quoted when needed, `%` doubled).
fn exec_arg(arg: &str) -> String {
    const RESERVED: &[char] = &[
        ' ', '\t', '\n', '"', '\'', '\\', '>', '<', '~', '|', '&', ';', '$', '*', '?', '#', '(',
        ')', '`',
    ];
    let arg = arg.replace('%', "%%");
    if !arg.contains(RESERVED) && !arg.is_empty() {
        return arg;
    }
    let mut quoted = String::from("\"");
    for c in arg.chars() {
        if matches!(c, '"' | '`' | '$' | '\\') {
            quoted.push('\\');
        }
        quoted.push(c);
    }
    quoted.push('"');
    // The string escape rule comes on top: every backslash is written twice.
    quoted.replace('\\', "\\\\")
}

/// A value of a desktop entry line: no line breaks or control characters.
fn entry_value(text: &str) -> String {
    text.chars()
        .map(|c| if c.is_control() { ' ' } else { c })
        .collect()
}

fn command_line(command: &CoveCommand, game_id: &str) -> Vec<String> {
    let mut parts = vec![command.program.clone()];
    parts.extend(command.args.iter().cloned());
    parts.push(LAUNCH_ARG.into());
    parts.push(game_id.into());
    parts
}

pub fn desktop_entry(
    title: &str,
    game_id: &str,
    command: &CoveCommand,
    icon: Option<&Path>,
) -> String {
    let exec = command_line(command, game_id)
        .iter()
        .map(|a| exec_arg(a))
        .collect::<Vec<_>>()
        .join(" ");
    // Without its own icon the shortcut shows Cove's.
    let icon = icon.map_or_else(
        || std::env::var("FLATPAK_ID").unwrap_or_else(|_| "cove".into()),
        |p| p.to_string_lossy().into_owned(),
    );
    format!(
        "[Desktop Entry]\nType=Application\nVersion=1.0\nName={}\nComment={}\nExec={exec}\nIcon={}\nTerminal=false\nCategories=Game;\nStartupNotify=false\n{MARKER}={game_id}\n",
        entry_value(title),
        entry_value(&tr!("Mit Cove spielen", "Play with Cove")),
        entry_value(&icon),
    )
}

fn write_desktop_file(
    path: &Path,
    title: &str,
    game_id: &str,
    command: &CoveCommand,
    icon: Option<&Path>,
) -> AppResult<()> {
    std::fs::write(path, desktop_entry(title, game_id, command, icon))?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o755))?;
    }
    // GNOME starts desktop files only once they are marked as trusted.
    if !crate::flatpak::active() {
        let _ = std::process::Command::new("gio")
            .args(["set", "-t", "string"])
            .arg(path)
            .args(["metadata::trusted", "true"])
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .status();
    }
    Ok(())
}

#[cfg(windows)]
fn write_lnk(
    path: &Path,
    title: &str,
    game_id: &str,
    command: &CoveCommand,
    icon: Option<&Path>,
) -> AppResult<()> {
    let link_error = |e: mslnk::MSLinkError| AppError::Io(std::io::Error::other(e.to_string()));
    let mut link = mslnk::ShellLink::new(&command.program).map_err(link_error)?;
    let mut args: Vec<String> = command.args.clone();
    args.push(LAUNCH_ARG.into());
    args.push(game_id.into());
    link.set_arguments(Some(args.join(" ")));
    link.set_name(Some(title.to_owned()));
    if let Some(dir) = Path::new(&command.program).parent() {
        link.set_working_dir(Some(dir.to_string_lossy().into_owned()));
    }
    if let Some(icon) = icon {
        link.set_icon_location(Some(icon.to_string_lossy().into_owned()));
    }
    link.create_lnk(path).map_err(link_error)
}

#[cfg(not(windows))]
fn write_lnk(
    _path: &Path,
    _title: &str,
    _game_id: &str,
    _command: &CoveCommand,
    _icon: Option<&Path>,
) -> AppResult<()> {
    Err(AppError::InvalidInput(
        "Windows shortcuts are only written on Windows".into(),
    ))
}

/// Characters that would break the Info.plist (XML).
fn xml(text: &str) -> String {
    text.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}

/// A single-quoted shell word.
fn sh(text: &str) -> String {
    format!("'{}'", text.replace('\'', r"'\''"))
}

fn write_app(
    path: &Path,
    title: &str,
    game_id: &str,
    command: &CoveCommand,
    icon: Option<&Path>,
) -> AppResult<()> {
    let contents = path.join("Contents");
    std::fs::create_dir_all(contents.join("MacOS"))?;
    std::fs::create_dir_all(contents.join("Resources"))?;
    let icon_key = if let Some(icon) = icon {
        std::fs::copy(icon, contents.join("Resources/icon.icns"))?;
        "<key>CFBundleIconFile</key><string>icon</string>"
    } else {
        ""
    };
    let bundle_id = format!("{BUNDLE_PREFIX}{}", game_id.replace([':', '_'], "-"));
    std::fs::write(
        contents.join("Info.plist"),
        format!(
            r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0"><dict>
<key>CFBundleExecutable</key><string>launch</string>
<key>CFBundleIdentifier</key><string>{}</string>
<key>CFBundleName</key><string>{}</string>
<key>CFBundlePackageType</key><string>APPL</string>
<key>LSUIElement</key><true/>
{icon_key}
</dict></plist>
"#,
            xml(&bundle_id),
            xml(title)
        ),
    )?;
    let script_path = contents.join("MacOS/launch");
    let line = command_line(command, game_id)
        .iter()
        .map(|a| sh(a))
        .collect::<Vec<_>>()
        .join(" ");
    std::fs::write(&script_path, format!("#!/bin/sh\nexec {line}\n"))?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&script_path, std::fs::Permissions::from_mode(0o755))?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::ManualGameInput;

    fn args(list: &[&str]) -> Vec<String> {
        list.iter().map(|s| (*s).to_owned()).collect()
    }

    #[test]
    fn the_launch_argument_takes_only_plain_ids() {
        assert_eq!(
            launch_arg(&args(&["cove", "--launch", "steam-620"])),
            Some("steam-620".into())
        );
        assert_eq!(
            launch_arg(&args(&["cove", "--launch=manual-ember"])),
            Some("manual-ember".into())
        );
        assert_eq!(launch_arg(&args(&["cove", "--launch", "../x y"])), None);
        assert_eq!(launch_arg(&args(&["cove", "--launch", "--other"])), None);
        assert_eq!(launch_arg(&args(&["cove"])), None);
    }

    #[test]
    fn exec_lines_are_quoted_like_the_desktop_entry_spec_wants() {
        assert_eq!(exec_arg("/usr/bin/cove"), "/usr/bin/cove");
        assert_eq!(
            exec_arg("/home/a b/Cove.AppImage"),
            "\"/home/a b/Cove.AppImage\""
        );
        assert_eq!(exec_arg("100%"), "100%%");
        assert_eq!(exec_arg("a\"$b"), "\"a\\\\\"\\\\$b\"");
        let command = CoveCommand {
            program: "/opt/My Apps/Cove.AppImage".into(),
            args: Vec::new(),
        };
        let entry = desktop_entry(
            "Ember\nand Oak",
            "manual-ember",
            &command,
            Some(Path::new("/i/x.png")),
        );
        assert!(entry.contains("Name=Ember and Oak\n"));
        assert!(entry.contains("Exec=\"/opt/My Apps/Cove.AppImage\" --launch manual-ember\n"));
        assert!(entry.contains("Icon=/i/x.png\n"));
        assert!(entry.contains("X-Cove-Game=manual-ember\n"));
    }

    fn png_cover() -> Vec<u8> {
        let image = image::RgbaImage::from_pixel(60, 90, image::Rgba([40, 80, 160, 255]));
        let mut bytes = std::io::Cursor::new(Vec::new());
        image.write_to(&mut bytes, image::ImageFormat::Png).unwrap();
        bytes.into_inner()
    }

    #[tokio::test]
    async fn shortcuts_are_created_listed_and_only_removed_when_ours() {
        let pool = crate::db::open_in_memory().await.unwrap();
        let tmp = tempfile::tempdir().unwrap();
        let exe = tmp.path().join("ember.sh");
        std::fs::write(&exe, b"#!/bin/sh\n").unwrap();
        let item = crate::db::library::add_manual(
            &pool,
            &ManualGameInput {
                title: "Ember & Oak".into(),
                executable_path: exe.to_string_lossy().into_owned(),
                size_bytes: None,
            },
        )
        .await
        .unwrap();
        let dirs = ShortcutDirs {
            desktop: Some(tmp.path().join("Desktop")),
            menu: Some(tmp.path().join("applications")),
            icons: tmp.path().join("icons"),
        };
        let command = CoveCommand {
            program: "/usr/bin/cove".into(),
            args: Vec::new(),
        };
        let shortcuts = Shortcuts::new(pool, dirs, Some(command), Kind::Desktop);
        assert_eq!(
            shortcuts.places(),
            [ShortcutPlace::Desktop, ShortcutPlace::Menu]
        );

        let desktop = shortcuts
            .create(&item, ShortcutPlace::Desktop, Some(png_cover()))
            .await
            .unwrap();
        assert!(Path::new(&desktop.path).ends_with("Desktop/Ember and Oak.desktop"));
        let text = std::fs::read_to_string(&desktop.path).unwrap();
        assert!(text.contains(&format!("Exec=/usr/bin/cove --launch {}", item.game.id)));
        let icon = tmp
            .path()
            .join("icons")
            .join(format!("{}.png", item.game.id));
        let decoded = image::open(&icon).unwrap();
        assert_eq!((decoded.width(), decoded.height()), (ICON_SIZE, ICON_SIZE));

        let menu = shortcuts
            .create(&item, ShortcutPlace::Menu, None)
            .await
            .unwrap();
        assert!(
            Path::new(&menu.path).ends_with(format!("applications/cove-{}.desktop", item.game.id))
        );
        assert_eq!(shortcuts.list(&item.game.id).await.unwrap().len(), 2);

        // A deleted file is dropped from the list.
        std::fs::remove_file(&menu.path).unwrap();
        assert_eq!(
            shortcuts.list(&item.game.id).await.unwrap(),
            std::slice::from_ref(&desktop)
        );

        // A file the user replaced is not Cove's: it stays.
        std::fs::write(&desktop.path, "[Desktop Entry]\nName=Mine\n").unwrap();
        shortcuts.remove_all(&item.game.id).await;
        assert!(Path::new(&desktop.path).exists());
        assert_eq!(shortcuts.list(&item.game.id).await.unwrap(), []);
        assert!(!icon.exists());
        // ... and Cove doesn't write over it either.
        let error = shortcuts
            .create(&item, ShortcutPlace::Desktop, None)
            .await
            .unwrap_err();
        assert!(matches!(error, AppError::InvalidInput(_)));
    }

    #[test]
    fn macos_apps_start_cove_through_a_script() {
        let tmp = tempfile::tempdir().unwrap();
        let app = tmp.path().join("Ember.app");
        let command = CoveCommand {
            program: "/Applications/Cove.app/Contents/MacOS/cove".into(),
            args: Vec::new(),
        };
        write_app(&app, "Ember <&> Oak", "manual-ember", &command, None).unwrap();
        let plist = std::fs::read_to_string(app.join("Contents/Info.plist")).unwrap();
        assert!(plist.contains("<string>app.cove.desktop.game.manual-ember</string>"));
        assert!(plist.contains("Ember &lt;&amp;&gt; Oak"));
        let script = std::fs::read_to_string(app.join("Contents/MacOS/launch")).unwrap();
        assert_eq!(
            script,
            "#!/bin/sh\nexec '/Applications/Cove.app/Contents/MacOS/cove' '--launch' 'manual-ember'\n"
        );
        assert!(is_ours(&app, Kind::App));
    }
}
