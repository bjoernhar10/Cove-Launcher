//! Games installed by Windows launchers: Ubisoft Connect, the EA app,
//! Battle.net and the Xbox app (PC Game Pass).
//!
//! Everything is read from what the launchers leave on the machine, without
//! a login: their uninstall entries and Ubisoft's install list in the
//! registry, and the Xbox app's game folders (`.GamingRoot` on each drive,
//! `MicrosoftGame.config` per game). Ubisoft, Battle.net and Xbox games start
//! through their launcher's link (DRM, login, overlay); EA games start their
//! executable, which brings up the EA app by itself. Nothing is changed.

use std::path::{Path, PathBuf};

use sha2::{Digest, Sha256};

use crate::models::{ExternalGame, ExternalLauncher};

/// One uninstall entry (`…\CurrentVersion\Uninstall\<key>`).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct UninstallEntry {
    pub key: String,
    pub display_name: Option<String>,
    pub install_location: Option<String>,
    pub uninstall_string: Option<String>,
    pub display_icon: Option<String>,
}

/// What the registry tells about installed games.
#[derive(Debug, Clone, Default)]
pub struct Registry {
    pub uninstall: Vec<UninstallEntry>,
    /// Ubisoft Connect: `Launcher\Installs\<id>` → `InstallDir`.
    pub ubisoft_installs: Vec<(String, String)>,
}

impl Registry {
    /// This machine's entries (empty outside Windows).
    pub fn read() -> Self {
        #[cfg(windows)]
        {
            read_windows()
        }
        #[cfg(not(windows))]
        {
            Self::default()
        }
    }
}

#[cfg(windows)]
fn read_windows() -> Registry {
    use winreg::enums::{HKEY_CURRENT_USER, HKEY_LOCAL_MACHINE, KEY_READ};
    use winreg::RegKey;

    const UNINSTALL: &str = r"SOFTWARE\Microsoft\Windows\CurrentVersion\Uninstall";
    const UNINSTALL_32: &str = r"SOFTWARE\WOW6432Node\Microsoft\Windows\CurrentVersion\Uninstall";
    const UBISOFT: &str = r"SOFTWARE\WOW6432Node\Ubisoft\Launcher\Installs";

    let machine = RegKey::predef(HKEY_LOCAL_MACHINE);
    let user = RegKey::predef(HKEY_CURRENT_USER);
    let mut registry = Registry::default();
    for (root, path) in [
        (&machine, UNINSTALL),
        (&machine, UNINSTALL_32),
        (&user, UNINSTALL),
    ] {
        let Ok(list) = root.open_subkey_with_flags(path, KEY_READ) else {
            continue;
        };
        for key in list.enum_keys().flatten() {
            let Ok(entry) = list.open_subkey_with_flags(&key, KEY_READ) else {
                continue;
            };
            let value = |name: &str| entry.get_value::<String, _>(name).ok();
            registry.uninstall.push(UninstallEntry {
                display_name: value("DisplayName"),
                install_location: value("InstallLocation"),
                uninstall_string: value("UninstallString"),
                display_icon: value("DisplayIcon"),
                key,
            });
        }
    }
    if let Ok(list) = machine.open_subkey_with_flags(UBISOFT, KEY_READ) {
        for id in list.enum_keys().flatten() {
            if let Ok(dir) = list
                .open_subkey_with_flags(&id, KEY_READ)
                .and_then(|k| k.get_value::<String, _>("InstallDir"))
            {
                registry.ubisoft_installs.push((id, dir));
            }
        }
    }
    registry
}

/// Drive roots that may hold the Xbox app's game folders (`C:\` …).
pub fn drive_roots() -> Vec<PathBuf> {
    if cfg!(windows) {
        (b'C'..=b'Z')
            .map(|letter| PathBuf::from(format!("{}:\\", char::from(letter))))
            .filter(|root| root.is_dir())
            .collect()
    } else {
        Vec::new()
    }
}

fn game(
    id: String,
    launcher: ExternalLauncher,
    title: String,
    exe: &Path,
    folder: &Path,
    launch_url: Option<String>,
) -> ExternalGame {
    ExternalGame {
        id,
        launcher,
        title,
        executable_path: exe.to_string_lossy().into_owned(),
        windows: false,
        proton: false,
        wine_prefix: None,
        wine_binary: None,
        arguments: String::new(),
        playtime_minutes: 0,
        install_path: Some(folder.to_string_lossy().into_owned()),
        launch_url,
        in_library: false,
    }
}

fn clean(value: Option<&str>) -> Option<String> {
    value
        .map(|v| v.trim().trim_matches('"').trim())
        .filter(|v| !v.is_empty())
        .map(str::to_owned)
}

/// The folder's name, the title when a launcher names none.
fn folder_title(folder: &Path) -> Option<String> {
    folder
        .file_name()
        .map(|n| n.to_string_lossy().trim().to_owned())
        .filter(|n| !n.is_empty())
}

/// `DisplayIcon` when it is an executable inside the game's folder (`path,0`).
fn icon_exe(entry: &UninstallEntry, folder: &Path) -> Option<PathBuf> {
    let icon = clean(entry.display_icon.as_deref())?;
    let path = PathBuf::from(
        icon.rsplit_once(',')
            .map_or(icon.as_str(), |(p, _)| p)
            .trim_matches('"'),
    );
    let exe = path
        .extension()
        .is_some_and(|e| e.eq_ignore_ascii_case("exe"));
    (exe && path.starts_with(folder) && path.is_file()).then_some(path)
}

/// Ubisoft Connect: its install list, titles from the `Uplay Install <id>` entries.
fn ubisoft(registry: &Registry) -> Vec<ExternalGame> {
    let mut games = Vec::new();
    for (id, dir) in &registry.ubisoft_installs {
        if id.is_empty() || !id.chars().all(|c| c.is_ascii_digit()) {
            continue;
        }
        let folder = PathBuf::from(dir.trim().trim_end_matches(['/', '\\']));
        if !folder.is_dir() {
            continue;
        }
        let entry = registry
            .uninstall
            .iter()
            .find(|e| e.key == format!("Uplay Install {id}"));
        let Some(title) = entry
            .and_then(|e| clean(e.display_name.as_deref()))
            .or_else(|| folder_title(&folder))
        else {
            continue;
        };
        let Some(exe) = entry
            .and_then(|e| icon_exe(e, &folder))
            .or_else(|| crate::scanner::main_windows_exe(&folder, &title))
        else {
            continue;
        };
        games.push(game(
            format!("ubisoft:{id}"),
            ExternalLauncher::Ubisoft,
            title,
            &exe,
            &folder,
            Some(format!("uplay://launch/{id}/0")),
        ));
    }
    games
}

/// Battle.net's launch codes for its uninstall entries' `--uid=` (the client
/// starts a game with `battlenet://<code>`). Unknown products start their
/// executable instead.
const BATTLE_NET_CODES: &[(&str, &str)] = &[
    ("wow", "WoW"),
    ("diablo3", "D3"),
    ("prometheus", "Pro"),
    ("hs_beta", "WTCG"),
    ("heroes", "Hero"),
    ("s2", "S2"),
    ("s1", "S1"),
    ("w3", "W3"),
    ("osi", "OSI"),
    ("fenris", "Fen"),
    ("odin", "ODIN"),
    ("viper", "VIPR"),
    ("zeus", "ZEUS"),
    ("fore", "FORE"),
    ("auks", "AUKS"),
    ("lazr", "LAZR"),
    ("wlby", "WLBY"),
    ("anbs", "ANBS"),
    ("rtro", "RTRO"),
];

fn battle_net(registry: &Registry) -> Vec<ExternalGame> {
    let mut games = Vec::new();
    for entry in &registry.uninstall {
        let Some(uninstall) = entry.uninstall_string.as_deref() else {
            continue;
        };
        let lower = uninstall.to_ascii_lowercase();
        if !lower.contains("battle.net") {
            continue;
        }
        let Some(uid) = lower
            .split_whitespace()
            .find_map(|part| part.strip_prefix("--uid="))
            .map(|u| u.trim_matches('"').to_owned())
            .filter(|u| !u.is_empty() && u != "battle.net")
        else {
            continue;
        };
        let (Some(title), Some(folder)) = (
            clean(entry.display_name.as_deref()),
            clean(entry.install_location.as_deref()).map(PathBuf::from),
        ) else {
            continue;
        };
        if !folder.is_dir() {
            continue;
        }
        let Some(exe) =
            icon_exe(entry, &folder).or_else(|| crate::scanner::main_windows_exe(&folder, &title))
        else {
            continue;
        };
        let url = BATTLE_NET_CODES
            .iter()
            .find(|(id, _)| uid == *id)
            .map(|(_, code)| format!("battlenet://{code}"));
        games.push(game(
            format!("battlenet:{uid}"),
            ExternalLauncher::BattleNet,
            title,
            &exe,
            &folder,
            url,
        ));
    }
    games
}

/// EA app games: their uninstall entry runs EA's `__Installer\Touchup.exe`.
fn ea(registry: &Registry) -> Vec<ExternalGame> {
    let mut games = Vec::new();
    for entry in &registry.uninstall {
        let is_ea = entry.uninstall_string.as_deref().is_some_and(|u| {
            let lower = u.to_ascii_lowercase();
            lower.contains("__installer") && lower.contains("touchup.exe")
        });
        if !is_ea {
            continue;
        }
        let (Some(title), Some(folder)) = (
            clean(entry.display_name.as_deref()),
            clean(entry.install_location.as_deref()).map(PathBuf::from),
        ) else {
            continue;
        };
        if !folder.is_dir() {
            continue;
        }
        let Some(exe) =
            icon_exe(entry, &folder).or_else(|| crate::scanner::main_windows_exe(&folder, &title))
        else {
            continue;
        };
        games.push(game(
            format!("ea:{}", entry.key),
            ExternalLauncher::Ea,
            title,
            &exe,
            &folder,
            None,
        ));
    }
    games
}

/// The folders listed in a drive's `.GamingRoot` (relative to the drive):
/// `RGBX`, a 32-bit count, then UTF-16 names ending in a zero.
fn gaming_root(bytes: &[u8]) -> Vec<String> {
    if bytes.len() < 8 || &bytes[..4] != b"RGBX" {
        return Vec::new();
    }
    let units: Vec<u16> = bytes[8..]
        .chunks_exact(2)
        .map(|c| u16::from_le_bytes([c[0], c[1]]))
        .collect();
    units
        .split(|&u| u == 0)
        .filter_map(|name| String::from_utf16(name).ok())
        .map(|name| name.trim().trim_matches(['\\', '/']).to_owned())
        .filter(|name| !name.is_empty() && !name.contains(".."))
        .collect()
}

/// The value of `name="…"` in the first `<tag …>` of an XML text.
fn xml_attr(xml: &str, tag: &str, name: &str) -> Option<String> {
    let open = format!("<{tag}");
    // The whole tag: `<Executable` must not match `<ExecutableList>`.
    let start = xml.match_indices(&open).map(|(i, _)| i).find(|&i| {
        xml[i + open.len()..]
            .chars()
            .next()
            .is_some_and(|c| c.is_whitespace() || c == '/' || c == '>')
    })?;
    let rest = &xml[start..];
    let element = &rest[..rest.find('>')?];
    let key = format!("{name}=\"");
    let mut search = element;
    loop {
        let at = search.find(&key)?;
        // A whole attribute name: `Name=` must not match `DisplayName=`.
        let before = search[..at].chars().last();
        let value_start = at + key.len();
        if before.is_some_and(char::is_whitespace) {
            let value = &search[value_start..];
            let value = &value[..value.find('"')?];
            return Some(
                value
                    .replace("&quot;", "\"")
                    .replace("&apos;", "'")
                    .replace("&lt;", "<")
                    .replace("&gt;", ">")
                    .replace("&amp;", "&"),
            );
        }
        search = &search[value_start..];
    }
}

/// The publisher part of a package family name: the first 8 bytes of the
/// SHA-256 of the publisher (UTF-16LE) in Windows' base32 (13 characters).
pub fn publisher_id(publisher: &str) -> String {
    const ALPHABET: &[u8; 32] = b"0123456789abcdefghjkmnpqrstvwxyz";
    let utf16: Vec<u8> = publisher
        .encode_utf16()
        .flat_map(u16::to_le_bytes)
        .collect();
    let hash = Sha256::digest(&utf16);
    let mut bits = [0u8; 9];
    bits[..8].copy_from_slice(&hash[..8]);
    // 64 bits plus one zero bit = 13 groups of 5.
    let value = u128::from_be_bytes({
        let mut wide = [0u8; 16];
        wide[8..].copy_from_slice(&bits[..8]);
        wide
    }) << 1;
    (0..13)
        .rev()
        .map(|group| {
            char::from(ALPHABET[usize::try_from((value >> (group * 5)) & 31).unwrap_or(0)])
        })
        .collect()
}

/// Xbox app games in the folders the drives' `.GamingRoot` files list.
fn xbox(roots: &[PathBuf]) -> Vec<ExternalGame> {
    let mut games = Vec::new();
    for root in roots {
        let Ok(bytes) = std::fs::read(root.join(".GamingRoot")) else {
            continue;
        };
        for library in gaming_root(&bytes) {
            let Ok(entries) = std::fs::read_dir(root.join(&library)) else {
                continue;
            };
            for entry in entries.flatten() {
                let folder = entry.path();
                if let Some(game) = xbox_game(&folder) {
                    games.push(game);
                }
            }
        }
    }
    games
}

fn xbox_game(folder: &Path) -> Option<ExternalGame> {
    let content = folder.join("Content");
    let config = std::fs::read_to_string(content.join("MicrosoftGame.config")).ok()?;
    let name = xml_attr(&config, "Identity", "Name")?;
    let publisher = xml_attr(&config, "Identity", "Publisher")?;
    let exe_name = xml_attr(&config, "Executable", "Name")?;
    let app_id = xml_attr(&config, "Executable", "Id")?;
    let safe = |v: &str| {
        !v.is_empty()
            && v.chars()
                .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '-' | '_'))
    };
    if !safe(&name) || !safe(&app_id) || exe_name.contains("..") {
        return None;
    }
    let exe = content.join(exe_name.replace('\\', "/"));
    let title = xml_attr(&config, "ShellVisuals", "DefaultDisplayName")
        .filter(|t| !t.trim().is_empty() && !t.starts_with("ms-resource:"))
        .or_else(|| folder_title(folder))?;
    let family = format!("{name}_{}", publisher_id(&publisher));
    Some(game(
        format!("xbox:{name}"),
        ExternalLauncher::Xbox,
        title,
        &exe,
        folder,
        Some(format!("shell:AppsFolder\\{family}!{app_id}")),
    ))
}

/// Every game the Windows launchers installed.
pub fn games(registry: &Registry, roots: &[PathBuf]) -> Vec<ExternalGame> {
    let mut games = ubisoft(registry);
    games.extend(battle_net(registry));
    games.extend(ea(registry));
    games.extend(xbox(roots));
    games
}

#[cfg(test)]
mod tests {
    use super::*;

    fn write(path: &Path, text: &[u8]) {
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, text).unwrap();
    }

    fn big_exe(path: &Path) {
        write(path, &vec![0u8; 20_000]);
    }

    fn s(path: &Path) -> String {
        path.to_string_lossy().into_owned()
    }

    #[test]
    fn publisher_ids_match_windows() {
        // Microsoft's own apps end in `_8wekyb3d8bbwe`.
        assert_eq!(
            publisher_id(
                "CN=Microsoft Corporation, O=Microsoft Corporation, L=Redmond, S=Washington, C=US"
            ),
            "8wekyb3d8bbwe"
        );
    }

    #[test]
    fn xml_attributes_need_the_whole_name() {
        let xml = r#"<Game><ShellVisualsList/><ShellVisuals DefaultDisplayName="Tom &amp; Jerry" Name="x"/></Game>"#;
        assert_eq!(
            xml_attr(xml, "ShellVisuals", "DefaultDisplayName").as_deref(),
            Some("Tom & Jerry")
        );
        assert_eq!(xml_attr(xml, "ShellVisuals", "Name").as_deref(), Some("x"));
        assert_eq!(xml_attr(xml, "ShellVisuals", "Id"), None);
    }

    #[test]
    #[allow(clippy::too_many_lines)] // four launchers set up in one go
    fn launchers_games_come_with_their_links() {
        let tmp = tempfile::tempdir().unwrap();
        let t = tmp.path();

        // Ubisoft: one with an uninstall entry and icon, one without, one gone.
        let ac = t.join("Ubisoft/games/Assassin's Creed Origins");
        big_exe(&ac.join("ACOrigins.exe"));
        big_exe(&ac.join("UbisoftGameLauncher.exe"));
        let anno = t.join("Ubisoft/games/Anno 1800");
        big_exe(&anno.join("Bin/Win64/Anno1800.exe"));
        // Battle.net: a known product and an unknown one.
        let ow = t.join("Overwatch");
        big_exe(&ow.join("_retail_/Overwatch.exe"));
        let odd = t.join("Odd Game");
        big_exe(&odd.join("Odd.exe"));
        // EA app.
        let bf = t.join("EA Games/Battlefield 2042");
        big_exe(&bf.join("BF2042.exe"));
        // Not a launcher game.
        let other = t.join("Other");
        big_exe(&other.join("other.exe"));

        let entry =
            |key: &str, name: &str, folder: &Path, uninstall: &str, icon: Option<String>| {
                UninstallEntry {
                    key: key.into(),
                    display_name: Some(name.into()),
                    install_location: Some(s(folder)),
                    uninstall_string: Some(uninstall.into()),
                    display_icon: icon,
                }
            };
        let registry = Registry {
            uninstall: vec![
                entry(
                    "Uplay Install 3539",
                    "Assassin's Creed Origins",
                    &ac,
                    "\"C:\\Ubisoft\\upc.exe\" uplay://uninstall/3539",
                    Some(format!("{},0", s(&ac.join("ACOrigins.exe")))),
                ),
                entry(
                    "Overwatch",
                    "Overwatch",
                    &ow,
                    "\"C:\\ProgramData\\Battle.net\\Agent\\Blizzard Uninstaller.exe\" --lang=enUS --uid=prometheus --displayname=\"Overwatch\"",
                    None,
                ),
                entry(
                    "Odd",
                    "Odd Game",
                    &odd,
                    "\"C:\\ProgramData\\Battle.net\\Agent\\Blizzard Uninstaller.exe\" --uid=oddity",
                    None,
                ),
                entry(
                    "{1234}",
                    "Battlefield™ 2042",
                    &bf,
                    &format!("\"{}\" uninstall_game -locale en_US", s(&bf.join("__Installer/Touchup.exe"))),
                    None,
                ),
                entry("Other", "Other", &other, "\"C:\\Other\\unins000.exe\"", None),
            ],
            ubisoft_installs: vec![
                ("3539".into(), format!("{}/", s(&ac))),
                ("4553".into(), s(&anno)),
                ("77".into(), s(&t.join("gone"))),
                ("../x".into(), s(&anno)),
            ],
        };

        // Xbox app: a .GamingRoot listing "XboxGames".
        let mut root_file = b"RGBX".to_vec();
        root_file.extend(1u32.to_le_bytes());
        root_file.extend("XboxGames".encode_utf16().flat_map(u16::to_le_bytes));
        root_file.extend([0, 0]);
        write(&t.join(".GamingRoot"), &root_file);
        let grounded = t.join("XboxGames/Grounded");
        big_exe(&grounded.join("Content/Maine-WinGDK-Shipping.exe"));
        write(
            &grounded.join("Content/MicrosoftGame.config"),
            br#"<?xml version="1.0"?><Game configVersion="1">
<Identity Name="Microsoft.Maine" Publisher="CN=Microsoft Corporation, O=Microsoft Corporation, L=Redmond, S=Washington, C=US" Version="1.0.0.0"/>
<ExecutableList><Executable Name="Maine-WinGDK-Shipping.exe" Id="Game" /></ExecutableList>
<ShellVisuals DefaultDisplayName="ms-resource:Title" /></Game>"#,
        );

        let mut found = games(&registry, &[t.to_path_buf()]);
        found.sort_by(|a, b| a.id.cmp(&b.id));
        let summary: Vec<(&str, &str, Option<&str>)> = found
            .iter()
            .map(|g| (g.id.as_str(), g.title.as_str(), g.launch_url.as_deref()))
            .collect();
        assert_eq!(
            summary,
            [
                ("battlenet:oddity", "Odd Game", None),
                ("battlenet:prometheus", "Overwatch", Some("battlenet://Pro")),
                ("ea:{1234}", "Battlefield™ 2042", None),
                (
                    "ubisoft:3539",
                    "Assassin's Creed Origins",
                    Some("uplay://launch/3539/0")
                ),
                ("ubisoft:4553", "Anno 1800", Some("uplay://launch/4553/0")),
                (
                    "xbox:Microsoft.Maine",
                    "Grounded",
                    Some("shell:AppsFolder\\Microsoft.Maine_8wekyb3d8bbwe!Game")
                ),
            ]
        );
        let path = |id: &str| {
            let g = found.iter().find(|g| g.id == id).unwrap();
            (
                PathBuf::from(&g.executable_path),
                g.install_path.clone().map(PathBuf::from),
            )
        };
        assert_eq!(
            path("ubisoft:3539"),
            (ac.join("ACOrigins.exe"), Some(ac.clone()))
        );
        assert_eq!(path("ubisoft:4553").0, anno.join("Bin/Win64/Anno1800.exe"));
        assert_eq!(
            path("battlenet:prometheus").0,
            ow.join("_retail_/Overwatch.exe")
        );
        assert_eq!(path("ea:{1234}").0, bf.join("BF2042.exe"));
        assert_eq!(
            path("xbox:Microsoft.Maine"),
            (
                grounded.join("Content/Maine-WinGDK-Shipping.exe"),
                Some(grounded.clone())
            )
        );
        for game in &found {
            assert!(game
                .launch_url
                .as_deref()
                .is_none_or(|u| crate::launcher::launch_url_via(u).is_some()));
        }
    }
}
