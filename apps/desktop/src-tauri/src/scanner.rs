//! Folder scan: finds launchable games and proposes one executable per game.
//!
//! Heuristic, in short:
//! 1. Walk the folder (max depth 4), skipping hidden, redistributable and engine folders.
//! 2. Collect candidates: Windows `.exe`, Linux ELF binaries and `.sh` scripts with the
//!    executable bit, `.AppImage` files and `.desktop` entries.
//! 3. Drop installers, uninstallers, crash reporters, updaters, anti-cheat and helpers by name.
//! 4. Group candidates per game folder (each top-level sub folder is one game; `AppImages` and
//!    `.desktop` files directly in the folder are games of their own) and keep the best-scoring
//!    candidate per group: name similarity with the folder, shallow depth, size, 64-bit dirs,
//!    native over Windows binaries on Linux.
//!
//! The result is only a suggestion list — the user confirms what gets added.

use std::collections::{BTreeMap, HashSet};
use std::fs;
use std::io::Read;
use std::path::{Path, PathBuf};

use crate::error::{AppError, AppResult};
use crate::models::{ExecutableKind, ScanCandidate};

const MAX_DEPTH: usize = 4;
/// Upper bound of files visited for size calculation per game folder.
const MAX_SIZE_FILES: usize = 200_000;

/// Substrings (lowercase) that mark non-game executables.
const IGNORED_NAME_PARTS: &[&str] = &[
    "unins",
    "uninstall",
    "setup",
    "install",
    "crash",
    "reporter",
    "bugreport",
    "errorreport",
    "redist",
    "dxsetup",
    "directx",
    "dotnet",
    "prereq",
    "easyanticheat",
    "eac_",
    "battleye",
    "beservice",
    "updater",
    "update",
    "patcher",
    "helper",
    "cefprocess",
    "subprocess",
    "notification",
    "overlay",
    "diagnostic",
    "unitypackage",
    "unrealpak",
    "shadercompile",
    "ue4prereq",
    "physx",
    "oalinst",
    "vulkan",
    "touchup",
];

/// Folder names (lowercase) that never contain the game executable.
const IGNORED_DIRS: &[&str] = &[
    "_commonredist",
    "commonredist",
    "redist",
    "redistributables",
    "__installer",
    "installer",
    "installers",
    "directx",
    "dotnetfx",
    "vcredist",
    "support",
    "easyanticheat",
    "battleye",
    "crashreporter",
    "crashpad",
    "engine",
    "node_modules",
    "__macosx",
    "steamworks shared",
    "shadercache",
    "compatdata",
    "prerequisites",
];

/// Sub folders that belong to the game in the scanned folder itself (not separate games).
const GENERIC_DIRS: &[&str] = &[
    "bin", "bin32", "bin64", "binaries", "x64", "x86", "x86_64", "win64", "win32", "linux",
    "linux64", "game", "data", "lib", "lib64",
];

/// Tokens removed from titles derived from file or folder names.
const TITLE_NOISE: &[&str] = &[
    "x86", "x64", "x86_64", "amd64", "win64", "win32", "linux", "linux64", "windows", "appimage",
    "gog", "steam", "shipping", "64", "32", "bit",
];

#[derive(Debug, Clone, PartialEq)]
struct Found {
    path: PathBuf,
    kind: ExecutableKind,
    /// Depth relative to its group folder (0 = directly inside).
    depth: usize,
    size: u64,
    /// `Name=` of a desktop entry.
    display_name: Option<String>,
}

/// True if a file or folder name looks like a non-game helper (installer, crash reporter, …).
pub fn is_ignored_name(name: &str) -> bool {
    let lower = name.to_lowercase();
    let stem = lower.rsplit_once('.').map_or(lower.as_str(), |(s, _)| s);
    IGNORED_NAME_PARTS.iter().any(|part| stem.contains(part))
}

pub fn is_ignored_dir(name: &str) -> bool {
    let lower = name.to_lowercase();
    lower.starts_with('.') || IGNORED_DIRS.contains(&lower.as_str())
}

/// Minimal `.desktop` entry used for scanning.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DesktopEntry {
    pub name: String,
    /// Exec line with field codes (%f, %U, …) removed.
    pub exec: String,
    pub working_dir: Option<String>,
}

/// Parses the `[Desktop Entry]` group. Returns None for non-applications or hidden entries.
pub fn parse_desktop_entry(content: &str) -> Option<DesktopEntry> {
    let mut in_group = false;
    let (mut name, mut exec, mut kind, mut path) = (None, None, None, None);
    let mut hidden = false;
    for line in content.lines().map(str::trim) {
        if line.starts_with('[') {
            in_group = line == "[Desktop Entry]";
            continue;
        }
        if !in_group || line.starts_with('#') {
            continue;
        }
        let Some((key, value)) = line.split_once('=') else {
            continue;
        };
        match key.trim() {
            "Name" => name = Some(value.trim().to_owned()),
            "Exec" => exec = Some(strip_field_codes(value.trim())),
            "Type" => kind = Some(value.trim().to_owned()),
            "Path" => path = Some(value.trim().to_owned()).filter(|p| !p.is_empty()),
            "NoDisplay" | "Hidden" => hidden |= value.trim().eq_ignore_ascii_case("true"),
            _ => {}
        }
    }
    if hidden || kind.as_deref() != Some("Application") {
        return None;
    }
    let exec = exec.filter(|e| !e.is_empty())?;
    Some(DesktopEntry {
        name: name.unwrap_or_default(),
        exec,
        working_dir: path,
    })
}

fn strip_field_codes(exec: &str) -> String {
    let mut out = String::with_capacity(exec.len());
    let mut chars = exec.chars().peekable();
    while let Some(c) = chars.next() {
        if c == '%' {
            if chars.next() == Some('%') {
                out.push('%');
            }
        } else {
            out.push(c);
        }
    }
    out.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// Human title from a file or folder name: "the_long-commute_v1.2" → "The Long Commute".
pub fn prettify_title(raw: &str) -> String {
    let lower = raw.to_lowercase();
    let mut stem = raw;
    for ext in [
        ".appimage",
        ".exe",
        ".desktop",
        ".sh",
        ".x86_64",
        ".x86",
        ".bin",
    ] {
        if lower.ends_with(ext) {
            stem = &raw[..raw.len() - ext.len()];
            break;
        }
    }

    // Split camelCase ("HollowLantern" → "Hollow Lantern") and separators.
    let mut spaced = String::with_capacity(stem.len() + 8);
    let mut prev: Option<char> = None;
    for c in stem.chars() {
        if matches!(c, '_' | '-' | '.' | '+') {
            spaced.push(' ');
        } else {
            if let Some(p) = prev {
                if p.is_lowercase() && c.is_uppercase() {
                    spaced.push(' ');
                }
            }
            spaced.push(c);
        }
        prev = Some(c);
    }

    let is_version = |w: &str| {
        let w = w.strip_prefix(['v', 'V']).unwrap_or(w);
        !w.is_empty() && w.chars().all(|c| c.is_ascii_digit())
    };
    let words: Vec<String> = spaced
        .split_whitespace()
        .filter(|w| !TITLE_NOISE.contains(&w.to_lowercase().as_str()) && !is_version(w))
        .map(|w| {
            if w.chars().all(|c| !c.is_uppercase()) {
                let mut chars = w.chars();
                chars
                    .next()
                    .map_or_else(String::new, |f| f.to_uppercase().chain(chars).collect())
            } else {
                w.to_owned()
            }
        })
        .collect();
    if words.is_empty() {
        raw.to_owned()
    } else {
        words.join(" ")
    }
}

fn normalize(name: &str) -> String {
    prettify_title(name)
        .to_lowercase()
        .chars()
        .filter(char::is_ascii_alphanumeric)
        .collect()
}

/// Score for picking the main executable of a game folder; higher is better.
fn score(found: &Found, group_name: &str, prefer_native: bool) -> i64 {
    let mut score: i64 = match found.kind {
        ExecutableKind::DesktopEntry => 40,
        ExecutableKind::Appimage => 35,
        ExecutableKind::WindowsExe | ExecutableKind::LinuxBinary => 20,
        ExecutableKind::Script => 15,
    };
    if prefer_native && found.kind != ExecutableKind::WindowsExe {
        score += 10;
    }

    let file_name = found
        .path
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();
    let candidate = normalize(&file_name);
    let group = normalize(group_name);
    if !candidate.is_empty() && !group.is_empty() {
        if candidate == group {
            score += 50;
        } else if candidate.contains(&group) || group.contains(&candidate) {
            score += 30;
        }
    }

    score -= 5 * i64::try_from(found.depth).unwrap_or(10);
    score += i64::try_from((found.size / 20_000_000).min(10)).unwrap_or(0);

    let path = found.path.to_string_lossy().to_lowercase();
    if ["win64", "x86_64", "x64", "linux64"]
        .iter()
        .any(|d| path.contains(d))
    {
        score += 5;
    }
    if file_name.to_lowercase().contains("launcher") {
        score -= 8;
    }
    score
}

#[cfg(unix)]
fn is_executable(meta: &fs::Metadata) -> bool {
    use std::os::unix::fs::PermissionsExt;
    meta.permissions().mode() & 0o111 != 0
}

#[cfg(not(unix))]
fn is_executable(_meta: &fs::Metadata) -> bool {
    false
}

fn is_elf(path: &Path) -> bool {
    let mut magic = [0u8; 4];
    fs::File::open(path)
        .and_then(|mut f| f.read_exact(&mut magic))
        .is_ok()
        && magic == *b"\x7fELF"
}

/// Classifies a single file. None = not a launch candidate.
fn classify(path: &Path, meta: &fs::Metadata, depth: usize) -> Option<Found> {
    let name = path.file_name()?.to_string_lossy();
    if is_ignored_name(&name) {
        return None;
    }
    let lower = name.to_lowercase();
    let ext = Path::new(&lower)
        .extension()
        .map(|e| e.to_string_lossy().into_owned())
        .unwrap_or_default();
    let unix_like = cfg!(unix);
    let found = |kind, display_name| {
        Some(Found {
            path: path.to_path_buf(),
            kind,
            depth,
            size: meta.len(),
            display_name,
        })
    };

    match ext.as_str() {
        "exe" if meta.len() >= 10_000 => found(ExecutableKind::WindowsExe, None),
        "appimage" if unix_like => found(ExecutableKind::Appimage, None),
        "desktop" if unix_like => {
            let entry = parse_desktop_entry(&fs::read_to_string(path).ok()?)?;
            found(
                ExecutableKind::DesktopEntry,
                Some(entry.name).filter(|n| !n.is_empty()),
            )
        }
        "sh" if unix_like && depth <= 1 && is_executable(meta) => {
            found(ExecutableKind::Script, None)
        }
        "" | "x86_64" | "x86" | "bin" | "64" if unix_like => {
            let shared_lib = lower.starts_with("lib") || lower.contains(".so");
            if !shared_lib && meta.len() >= 4096 && is_executable(meta) && is_elf(path) {
                found(ExecutableKind::LinuxBinary, None)
            } else {
                None
            }
        }
        _ => None,
    }
}

fn walk(dir: &Path, depth: usize, out: &mut Vec<(PathBuf, usize, fs::Metadata)>) {
    let Ok(entries) = fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        // symlink_metadata: never follow links (avoids loops and scanning outside the folder)
        let Ok(meta) = fs::symlink_metadata(&path) else {
            continue;
        };
        if meta.is_dir() {
            let name = entry.file_name().to_string_lossy().into_owned();
            if depth < MAX_DEPTH && !is_ignored_dir(&name) {
                walk(&path, depth + 1, out);
            }
        } else if meta.is_file() {
            out.push((path, depth, meta));
        }
    }
}

fn folder_size(dir: &Path) -> u64 {
    let mut total = 0;
    let mut visited = 0;
    let mut stack = vec![dir.to_path_buf()];
    while let Some(current) = stack.pop() {
        let Ok(entries) = fs::read_dir(&current) else {
            continue;
        };
        for entry in entries.flatten() {
            visited += 1;
            if visited > MAX_SIZE_FILES {
                return total;
            }
            let Ok(meta) = fs::symlink_metadata(entry.path()) else {
                continue;
            };
            if meta.is_dir() {
                stack.push(entry.path());
            } else {
                total += meta.len();
            }
        }
    }
    total
}

/// Scans `root` and returns one suggestion per detected game, sorted by title.
/// `known` = executable paths already in the library.
pub fn scan(root: &Path, known: &HashSet<String>) -> AppResult<Vec<ScanCandidate>> {
    if !root.is_dir() {
        return Err(AppError::InvalidInput(format!(
            "„{}“ ist kein Ordner.",
            root.display()
        )));
    }
    let prefer_native = cfg!(target_os = "linux");
    let mut files = Vec::new();
    walk(root, 0, &mut files);

    let root_name = root
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();
    let has_generic_dir = fs::read_dir(root).is_ok_and(|entries| {
        entries.flatten().any(|e| {
            e.path().is_dir()
                && GENERIC_DIRS.contains(&e.file_name().to_string_lossy().to_lowercase().as_str())
        })
    });

    // group key → (group folder, group display name, candidates)
    let mut groups: BTreeMap<String, (PathBuf, String, Vec<Found>)> = BTreeMap::new();
    for (path, depth, meta) in files {
        let relative = path.strip_prefix(root).unwrap_or(&path);
        let top = relative
            .components()
            .next()
            .map(|c| c.as_os_str().to_string_lossy().into_owned());
        let at_root = depth == 0;
        let (key, folder, name, group_depth) = match top {
            Some(top) if !at_root && !has_generic_dir => {
                (top.clone(), root.join(&top), top, depth - 1)
            }
            _ => (String::new(), root.to_path_buf(), root_name.clone(), depth),
        };
        let Some(found) = classify(&path, &meta, group_depth) else {
            continue;
        };

        // AppImages and desktop entries lying directly in the scanned folder are games of their own.
        if at_root
            && matches!(
                found.kind,
                ExecutableKind::Appimage | ExecutableKind::DesktopEntry
            )
        {
            let file_name = path
                .file_name()
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_default();
            groups.insert(
                format!("file:{file_name}"),
                (path.clone(), file_name, vec![found]),
            );
            continue;
        }
        groups
            .entry(key)
            .or_insert_with(|| (folder, name, Vec::new()))
            .2
            .push(found);
    }

    let mut candidates: Vec<ScanCandidate> = groups
        .into_values()
        .filter_map(|(folder, group_name, found)| {
            let best = found
                .into_iter()
                .max_by_key(|f| score(f, &group_name, prefer_native))?;
            let title = best
                .display_name
                .clone()
                .unwrap_or_else(|| prettify_title(&group_name));
            let size = if folder.is_dir() {
                folder_size(&folder)
            } else {
                best.size
            };
            let executable_path = best.path.to_string_lossy().into_owned();
            Some(ScanCandidate {
                title,
                already_in_library: known.contains(&executable_path),
                executable_path,
                size_bytes: i64::try_from(size).unwrap_or(i64::MAX),
                kind: best.kind,
            })
        })
        .collect();
    candidates.sort_by_key(|c| c.title.to_lowercase());
    Ok(candidates)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ignores_helper_executables() {
        for name in [
            "unins000.exe",
            "Uninstall.exe",
            "setup.exe",
            "UnityCrashHandler64.exe",
            "CrashReportClient.exe",
            "vcredist_x64.exe",
            "VC_redist.x64.exe",
            "DXSETUP.exe",
            "EasyAntiCheat_Setup.exe",
            "BEService_x64.exe",
            "dotNetFx40_Full_setup.exe",
            "GameUpdater.exe",
            "CefSharp.BrowserSubprocess.exe",
            "UE4PrereqSetup_x64.exe",
        ] {
            assert!(is_ignored_name(name), "{name} should be ignored");
        }
        for name in [
            "Tidewater.exe",
            "hollow_lantern.x86_64",
            "start.sh",
            "Kite-Season.AppImage",
            "Game.exe",
        ] {
            assert!(!is_ignored_name(name), "{name} should be kept");
        }
    }

    #[test]
    fn ignores_redist_and_hidden_dirs() {
        assert!(is_ignored_dir("_CommonRedist"));
        assert!(is_ignored_dir("Engine"));
        assert!(is_ignored_dir(".cache"));
        assert!(!is_ignored_dir("Binaries"));
    }

    #[test]
    fn prettifies_titles() {
        assert_eq!(prettify_title("the_long_commute"), "The Long Commute");
        assert_eq!(
            prettify_title("HollowLantern-Win64-Shipping.exe"),
            "Hollow Lantern"
        );
        assert_eq!(
            prettify_title("Kite-Season-1.4.2-x86_64.AppImage"),
            "Kite Season"
        );
        assert_eq!(prettify_title("paper.orbit.v2"), "Paper Orbit");
        assert_eq!(prettify_title("FTL"), "FTL");
        assert_eq!(prettify_title("1234"), "1234");
    }

    #[test]
    fn parses_desktop_entries() {
        let entry = parse_desktop_entry(
            "[Desktop Entry]\nType=Application\nName=Paper Orbit\nName[de]=Papierbahn\nExec=\"/opt/paper orbit/run\" --flag %U\nPath=/opt/paper orbit\n\n[Desktop Action New]\nExec=other\n",
        )
        .unwrap();
        assert_eq!(entry.name, "Paper Orbit");
        assert_eq!(entry.exec, "\"/opt/paper orbit/run\" --flag");
        assert_eq!(entry.working_dir.as_deref(), Some("/opt/paper orbit"));

        assert!(parse_desktop_entry("[Desktop Entry]\nType=Link\nExec=x\n").is_none());
        assert!(
            parse_desktop_entry("[Desktop Entry]\nType=Application\nExec=x\nNoDisplay=true\n")
                .is_none()
        );
        assert!(parse_desktop_entry("[Desktop Entry]\nType=Application\nName=No exec\n").is_none());
        assert_eq!(strip_field_codes("run %f 100%% %U"), "run 100%");
    }

    fn found(path: &str, kind: ExecutableKind, depth: usize, size: u64) -> Found {
        Found {
            path: PathBuf::from(path),
            kind,
            depth,
            size,
            display_name: None,
        }
    }

    #[test]
    fn scoring_prefers_matching_shallow_native_binaries() {
        let group = "Hollow Lantern";
        let matching = found(
            "/g/Hollow Lantern/HollowLantern.exe",
            ExecutableKind::WindowsExe,
            0,
            300_000,
        );
        let other = found(
            "/g/Hollow Lantern/tools/Editor.exe",
            ExecutableKind::WindowsExe,
            1,
            90_000_000,
        );
        assert!(score(&matching, group, false) > score(&other, group, false));

        let deep = found(
            "/g/Hollow Lantern/Binaries/Win64/HollowLantern.exe",
            ExecutableKind::WindowsExe,
            2,
            300_000,
        );
        assert!(score(&matching, group, false) > score(&deep, group, false));

        let exe = found("/g/Tide/Tide.exe", ExecutableKind::WindowsExe, 0, 1_000_000);
        let elf = found(
            "/g/Tide/Tide.x86_64",
            ExecutableKind::LinuxBinary,
            0,
            1_000_000,
        );
        assert!(score(&elf, "Tide", true) > score(&exe, "Tide", true));

        let launcher = found(
            "/g/Tide/TideLauncher.exe",
            ExecutableKind::WindowsExe,
            0,
            1_000_000,
        );
        assert!(score(&exe, "Tide", false) > score(&launcher, "Tide", false));
    }

    #[cfg(unix)]
    mod fs_scan {
        use super::super::*;
        use std::os::unix::fs::PermissionsExt;

        fn file(root: &Path, rel: &str, size: u64, content: &[u8], exec: bool) {
            let path = root.join(rel);
            fs::create_dir_all(path.parent().unwrap()).unwrap();
            fs::write(&path, content).unwrap();
            let f = fs::OpenOptions::new().write(true).open(&path).unwrap();
            f.set_len(size.max(content.len() as u64)).unwrap();
            if exec {
                fs::set_permissions(&path, fs::Permissions::from_mode(0o755)).unwrap();
            }
        }

        const ELF: &[u8] = b"\x7fELF\x02\x01\x01";

        #[test]
        fn scans_a_games_folder() {
            let tmp = tempfile::tempdir().unwrap();
            let root = tmp.path();
            // Native Linux game with helpers next to it
            file(root, "Tidewater/tidewater.x86_64", 8_000_000, ELF, true);
            file(
                root,
                "Tidewater/UnityCrashHandler64.exe",
                1_000_000,
                b"MZ",
                false,
            );
            file(root, "Tidewater/libsteam_api.so", 400_000, ELF, true);
            file(
                root,
                "Tidewater/Tidewater_Data/level0",
                5_000_000,
                b"",
                false,
            );
            // Windows game (Unreal layout) with redistributables
            file(
                root,
                "Hollow Lantern/HollowLantern.exe",
                300_000,
                b"MZ",
                false,
            );
            file(
                root,
                "Hollow Lantern/Engine/Binaries/Win64/CrashReportClient.exe",
                2_000_000,
                b"MZ",
                false,
            );
            file(
                root,
                "Hollow Lantern/_CommonRedist/vcredist_x64.exe",
                9_000_000,
                b"MZ",
                false,
            );
            file(root, "Hollow Lantern/unins000.exe", 3_000_000, b"MZ", false);
            // Only installers → no suggestion
            file(root, "Setup Stuff/setup.exe", 1_000_000, b"MZ", false);
            // AppImage and desktop entry directly in the folder
            file(
                root,
                "Kite-Season-1.4.2-x86_64.AppImage",
                50_000_000,
                ELF,
                true,
            );
            file(
                root,
                "paper-orbit.desktop",
                0,
                b"[Desktop Entry]\nType=Application\nName=Paper Orbit\nExec=/opt/paper/run %U\n",
                false,
            );
            // Non-executable ELF and plain files are ignored
            file(root, "Notes/readme", 10_000, ELF, false);
            file(root, "readme.txt", 100, b"hi", false);
            // Hidden folder is skipped
            file(root, ".cache/thing.exe", 500_000, b"MZ", false);

            let known: HashSet<String> = [root
                .join("Hollow Lantern/HollowLantern.exe")
                .to_string_lossy()
                .into_owned()]
            .into();
            let result = scan(root, &known).unwrap();
            let summary: Vec<(&str, ExecutableKind, bool)> = result
                .iter()
                .map(|c| (c.title.as_str(), c.kind, c.already_in_library))
                .collect();
            assert_eq!(
                summary,
                [
                    ("Hollow Lantern", ExecutableKind::WindowsExe, true),
                    ("Kite Season", ExecutableKind::Appimage, false),
                    ("Paper Orbit", ExecutableKind::DesktopEntry, false),
                    ("Tidewater", ExecutableKind::LinuxBinary, false),
                ]
            );
            let tide = result.iter().find(|c| c.title == "Tidewater").unwrap();
            assert!(tide.executable_path.ends_with("tidewater.x86_64"));
            assert!(
                tide.size_bytes >= 14_000_000,
                "folder size includes data files"
            );
        }

        #[test]
        fn scanning_a_single_game_folder() {
            let tmp = tempfile::tempdir().unwrap();
            let root = tmp.path().join("Ember_and_Oak");
            file(&root, "bin/x86_64/ember", 6_000_000, ELF, true);
            file(&root, "start.sh", 100, b"#!/bin/sh\n", true);
            file(&root, "uninstall.sh", 100, b"#!/bin/sh\n", true);
            let result = scan(&root, &HashSet::new()).unwrap();
            assert_eq!(result.len(), 1);
            assert_eq!(result[0].title, "Ember And Oak");
        }

        #[test]
        fn rejects_missing_folder() {
            assert!(matches!(
                scan(Path::new("/definitely/not/here"), &HashSet::new()),
                Err(AppError::InvalidInput(_))
            ));
        }
    }
}
