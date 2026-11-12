//! A game's Proton prefix (`<app data>/compat/<game id>`, Windows files in
//! `pfx/`): its size, resetting it and running winetricks on it (Linux).

use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use crate::error::{AppError, AppResult};
use crate::models::PrefixInfo;

/// The prefix folder of a game. Game ids never contain path parts, but the
/// folder is deleted on reset, so make sure.
pub fn dir(root: &Path, game_id: &str) -> AppResult<PathBuf> {
    let plain = !game_id.is_empty()
        && game_id != "."
        && game_id != ".."
        && !game_id.contains(['/', '\\', '\0']);
    if !plain {
        return Err(AppError::InvalidInput(tr!(
            "Ungültige Spiel-ID.",
            "Invalid game ID."
        )));
    }
    Ok(root.join(game_id))
}

/// Size of a folder without following links: a prefix links to `/` (`z:`)
/// and umu adds `pfx/pfx → pfx`.
fn size(dir: &Path) -> u64 {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return 0;
    };
    entries
        .flatten()
        .map(|entry| match entry.file_type() {
            Ok(kind) if kind.is_dir() => size(&entry.path()),
            Ok(kind) if kind.is_file() => entry.metadata().map_or(0, |m| m.len()),
            _ => 0,
        })
        .sum()
}

/// Blocking (walks the prefix); run in `spawn_blocking`. `proton` is the
/// game's build (its bundled winetricks counts).
pub fn info(root: &Path, game_id: &str, proton: Option<&Path>) -> AppResult<PrefixInfo> {
    let dir = dir(root, game_id)?;
    let exists = dir.join("pfx").is_dir();
    Ok(PrefixInfo {
        exists,
        size_bytes: exists.then(|| i64::try_from(size(&dir)).unwrap_or(i64::MAX)),
        winetricks: find_winetricks(proton).is_some(),
    })
}

/// Where GE-Proton keeps winetricks and its helpers (cabextract, unzip, zenity).
fn bundled_tools(proton: &Path) -> PathBuf {
    proton.join("protonfixes/files")
}

/// winetricks: the one GE-Proton brings along, else the system's.
/// `(program, folder with the bundled helpers)`.
fn find_winetricks(proton: Option<&Path>) -> Option<(PathBuf, Option<PathBuf>)> {
    if let Some(tools) = proton.map(bundled_tools) {
        let program = tools.join("bin/winetricks");
        if program.is_file() {
            return Some((program, Some(tools)));
        }
    }
    crate::sidecar::which("winetricks").map(|program| (program, None))
}

fn no_prefix() -> AppError {
    AppError::InvalidInput(tr!(
        "Für dieses Spiel gibt es noch keinen Prefix. Er entsteht beim ersten Start mit Proton.",
        "This game has no prefix yet. It is created when the game first starts with Proton."
    ))
}

/// The folder to show: `pfx` (with `drive_c`).
pub fn folder(root: &Path, game_id: &str) -> AppResult<PathBuf> {
    let pfx = dir(root, game_id)?.join("pfx");
    if pfx.is_dir() {
        Ok(pfx)
    } else {
        Err(no_prefix())
    }
}

/// Deletes the prefix; the next start creates a fresh one. Blocking.
pub fn reset(root: &Path, game_id: &str) -> AppResult<()> {
    let dir = dir(root, game_id)?;
    if !dir.exists() {
        return Ok(());
    }
    std::fs::remove_dir_all(&dir)?;
    Ok(())
}

/// Wine of a Proton build (`files/` since Proton 5.13, `dist/` before).
fn wine_dir(proton: &Path) -> PathBuf {
    let files = proton.join("files");
    if files.join("bin").is_dir() {
        files
    } else {
        proton.join("dist")
    }
}

/// winetricks (with its window) for the prefix, using the game's Proton build.
pub fn winetricks_command(root: &Path, game_id: &str, proton: &Path) -> AppResult<Command> {
    let pfx = folder(root, game_id)?;
    let (program, tools) = find_winetricks(Some(proton)).ok_or_else(|| {
        AppError::Launch(tr!(
            "winetricks fehlt: Diese Proton-Version bringt es nicht mit und auf dem System ist es nicht installiert. Mit GE-Proton ist es dabei.",
            "winetricks is missing: this Proton version doesn't bring it and it isn't installed on the system. GE-Proton includes it."
        ))
    })?;
    let wine = wine_dir(proton);
    let mut command = Command::new(program);
    if let Some(tools) = tools {
        // GE-Proton's helpers first; the system's tools stay reachable.
        let mut paths = vec![tools.join("bin")];
        paths.extend(
            std::env::var_os("PATH")
                .map(|p| std::env::split_paths(&p).collect::<Vec<_>>())
                .unwrap_or_default(),
        );
        if let Ok(path) = std::env::join_paths(paths) {
            command.env("PATH", path);
        }
        let mut libs = vec![tools.join("lib/x86_64-linux-gnu"), tools.join("lib")];
        libs.extend(
            std::env::var_os("LD_LIBRARY_PATH")
                .map(|p| std::env::split_paths(&p).collect::<Vec<_>>())
                .unwrap_or_default(),
        );
        if let Ok(libs) = std::env::join_paths(libs) {
            command.env("LD_LIBRARY_PATH", libs);
        }
    }
    command
        .arg("--gui")
        .env("WINEPREFIX", &pfx)
        .env("WINE", wine.join("bin/wine"))
        .env("WINESERVER", wine.join("bin/wineserver"))
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    Ok(command)
}

/// Starts winetricks in the background.
pub fn winetricks(root: &Path, game_id: &str, proton: &Path) -> AppResult<()> {
    let mut command = winetricks_command(root, game_id, proton)?;
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        command.process_group(0);
    }
    let mut child = command.spawn().map_err(|e| {
        AppError::Launch(tr!(
            "winetricks konnte nicht gestartet werden: {e}",
            "winetricks could not be started: {e}"
        ))
    })?;
    std::thread::spawn(move || {
        let _ = child.wait();
    });
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_plain_game_ids_name_a_prefix() {
        let root = Path::new("/data/compat");
        assert_eq!(dir(root, "epic-sugar").unwrap(), root.join("epic-sugar"));
        for bad in ["", ".", "..", "../x", "a/b", "a\\b"] {
            assert!(dir(root, bad).is_err(), "{bad}");
        }
    }

    #[test]
    fn size_reset_and_missing_prefixes() {
        let root = tempfile::tempdir().unwrap();
        let info_now = || info(root.path(), "g", None).unwrap();
        assert!(!info_now().exists);
        assert_eq!(info_now().size_bytes, None);
        assert!(folder(root.path(), "g").is_err());

        let drive = root.path().join("g/pfx/drive_c");
        std::fs::create_dir_all(&drive).unwrap();
        std::fs::write(drive.join("save.dat"), [0u8; 300]).unwrap();
        std::fs::write(root.path().join("g/version"), b"10").unwrap();
        #[cfg(unix)]
        {
            // Links are not followed: z: points at / and umu's pfx/pfx at itself.
            std::os::unix::fs::symlink("/", root.path().join("g/pfx/z")).unwrap();
            std::os::unix::fs::symlink(".", root.path().join("g/pfx/pfx")).unwrap();
        }
        let now = info_now();
        assert!(now.exists);
        assert_eq!(now.size_bytes, Some(302));
        assert_eq!(folder(root.path(), "g").unwrap(), root.path().join("g/pfx"));

        reset(root.path(), "g").unwrap();
        assert!(!root.path().join("g").exists());
        reset(root.path(), "g").unwrap(); // nothing left: still fine
    }

    #[test]
    fn ge_proton_brings_its_own_winetricks() {
        let root = tempfile::tempdir().unwrap();
        let proton = tempfile::tempdir().unwrap();
        let tools = proton.path().join("protonfixes/files");
        std::fs::create_dir_all(tools.join("bin")).unwrap();
        std::fs::write(tools.join("bin/winetricks"), b"#!/bin/sh\n").unwrap();
        std::fs::create_dir_all(proton.path().join("files/bin")).unwrap();
        std::fs::create_dir_all(root.path().join("g/pfx")).unwrap();
        assert!(
            info(root.path(), "g", Some(proton.path()))
                .unwrap()
                .winetricks
        );

        let command = winetricks_command(root.path(), "g", proton.path()).unwrap();
        assert_eq!(
            command.get_program(),
            tools.join("bin/winetricks").as_os_str()
        );
        let path = command
            .get_envs()
            .find(|(k, _)| *k == "PATH")
            .and_then(|(_, v)| v)
            .map(|v| std::env::split_paths(v).next())
            .unwrap();
        assert_eq!(path, Some(tools.join("bin")));
    }

    #[test]
    fn winetricks_uses_the_wine_of_the_proton_build() {
        let root = tempfile::tempdir().unwrap();
        let proton = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(proton.path().join("files/bin")).unwrap();
        std::fs::create_dir_all(root.path().join("g/pfx")).unwrap();
        let Ok(command) = winetricks_command(root.path(), "g", proton.path()) else {
            // winetricks is not installed here: the error says how to get it.
            return;
        };
        let env: Vec<(String, String)> = command
            .get_envs()
            .filter_map(|(k, v)| Some((k.to_str()?.to_owned(), v?.to_str()?.to_owned())))
            .collect();
        let var = |key: &str| {
            env.iter()
                .find(|(k, _)| k == key)
                .map(|(_, v)| PathBuf::from(v))
        };
        assert_eq!(var("WINEPREFIX"), Some(root.path().join("g/pfx")));
        assert_eq!(var("WINE"), Some(proton.path().join("files/bin/wine")));
        assert_eq!(
            var("WINESERVER"),
            Some(proton.path().join("files/bin/wineserver"))
        );
    }
}
