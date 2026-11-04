//! Installs a downloaded file: unpacks archives (zip, 7z, tar, tar.gz/xz-less)
//! into the game folder, or moves a single file there, then finds the
//! executable. Blocking code; run it in `spawn_blocking`.

use std::collections::HashSet;
use std::fs;
use std::path::{Component, Path, PathBuf};

use crate::error::{AppError, AppResult};
use crate::models::{CompatibilityTool, ExecutableKind, ScanCandidate};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ArchiveKind {
    Zip,
    SevenZ,
    Tar,
    TarGz,
    /// Not an archive (`AppImage`, installer, binary, …).
    Plain,
}

#[allow(clippy::case_sensitive_file_extension_comparisons)] // compared in lower case
pub fn archive_kind(file_name: &str) -> ArchiveKind {
    let lower = file_name.to_lowercase();
    if lower.ends_with(".zip") {
        ArchiveKind::Zip
    } else if lower.ends_with(".7z") {
        ArchiveKind::SevenZ
    } else if lower.ends_with(".tar.gz") || lower.ends_with(".tgz") {
        ArchiveKind::TarGz
    } else if lower.ends_with(".tar") {
        ArchiveKind::Tar
    } else {
        ArchiveKind::Plain
    }
}

fn extract_error(e: impl std::fmt::Display) -> AppError {
    AppError::Download(format!("Das Archiv konnte nicht entpackt werden ({e})."))
}

/// Relative archive paths only: no `..`, no absolute paths (zip slip).
pub fn safe_relative(name: &str) -> Option<PathBuf> {
    let path = Path::new(name);
    let mut clean = PathBuf::new();
    for component in path.components() {
        match component {
            Component::Normal(part) => clean.push(part),
            Component::CurDir => {}
            Component::ParentDir | Component::RootDir | Component::Prefix(_) => return None,
        }
    }
    (!clean.as_os_str().is_empty()).then_some(clean)
}

/// Unpacks or moves `file` into `dest` (created). The source file is left alone.
pub fn unpack(file: &Path, file_name: &str, dest: &Path) -> AppResult<()> {
    fs::create_dir_all(dest)?;
    match archive_kind(file_name) {
        ArchiveKind::Zip => {
            unzip(file, dest)?;
        }
        ArchiveKind::SevenZ => {
            sevenz_rust2::decompress_file_with_extract_fn(
                file,
                dest,
                |entry, reader, _default_dest| {
                    let Some(relative) = safe_relative(entry.name()) else {
                        return Ok(true); // skip unsafe entries
                    };
                    sevenz_rust2::default_entry_extract_fn(entry, reader, &dest.join(relative))
                },
            )
            .map_err(extract_error)?;
        }
        ArchiveKind::Tar | ArchiveKind::TarGz => {
            let reader = fs::File::open(file)?;
            let unpack_all = |mut archive: tar::Archive<Box<dyn std::io::Read>>| -> AppResult<()> {
                for entry in archive.entries().map_err(extract_error)? {
                    let mut entry = entry.map_err(extract_error)?;
                    // `unpack_in` refuses paths outside `dest`.
                    entry.unpack_in(dest).map_err(extract_error)?;
                }
                Ok(())
            };
            if archive_kind(file_name) == ArchiveKind::TarGz {
                unpack_all(tar::Archive::new(Box::new(flate2::read::GzDecoder::new(
                    reader,
                ))))?;
            } else {
                unpack_all(tar::Archive::new(Box::new(reader)))?;
            }
        }
        ArchiveKind::Plain => {
            let target = dest.join(file_name);
            if fs::rename(file, &target).is_err() {
                fs::copy(file, &target)?;
            }
            if is_runnable_name(file_name) {
                make_executable(&target)?;
            }
        }
    }
    fix_permissions(dest, 0);
    Ok(())
}

/// Entry by entry: unsafe paths and symlinks are skipped, Unix modes kept.
fn unzip(file: &Path, dest: &Path) -> AppResult<()> {
    let mut archive = zip::ZipArchive::new(fs::File::open(file)?).map_err(extract_error)?;
    for index in 0..archive.len() {
        let mut entry = archive.by_index(index).map_err(extract_error)?;
        let Some(relative) = entry
            .enclosed_name()
            .and_then(|p| safe_relative(&p.to_string_lossy()))
        else {
            continue;
        };
        if entry.is_symlink() {
            continue;
        }
        let target = dest.join(relative);
        if entry.is_dir() {
            fs::create_dir_all(&target)?;
            continue;
        }
        if let Some(parent) = target.parent() {
            fs::create_dir_all(parent)?;
        }
        let mut out = fs::File::create(&target)?;
        std::io::copy(&mut entry, &mut out)?;
        #[cfg(unix)]
        if let Some(mode) = entry.unix_mode() {
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(&target, fs::Permissions::from_mode(mode & 0o777 | 0o600))?;
        }
    }
    Ok(())
}

#[allow(clippy::case_sensitive_file_extension_comparisons)] // compared in lower case
fn is_runnable_name(name: &str) -> bool {
    let lower = name.to_lowercase();
    [".appimage", ".sh", ".x86_64", ".x86", ".run", ".bin"]
        .iter()
        .any(|e| lower.ends_with(e))
        || !lower.contains('.')
}

#[cfg(unix)]
fn make_executable(path: &Path) -> AppResult<()> {
    use std::os::unix::fs::PermissionsExt;
    let mut permissions = fs::metadata(path)?.permissions();
    permissions.set_mode(permissions.mode() | 0o755);
    fs::set_permissions(path, permissions)?;
    Ok(())
}

#[cfg(not(unix))]
#[allow(clippy::unnecessary_wraps)] // same signature as the Unix version
fn make_executable(_path: &Path) -> AppResult<()> {
    Ok(())
}

fn is_elf(path: &Path) -> bool {
    use std::io::Read;
    let mut magic = [0u8; 4];
    fs::File::open(path)
        .and_then(|mut f| f.read_exact(&mut magic))
        .is_ok()
        && magic == *b"\x7fELF"
}

/// Archives made on Windows lose the executable bit: restore it for ELF
/// binaries and scripts so the game can start (and the scanner sees them).
#[allow(clippy::case_sensitive_file_extension_comparisons)] // compared in lower case
fn fix_permissions(dir: &Path, depth: usize) {
    if depth > 4 || !cfg!(unix) {
        return;
    }
    let Ok(entries) = fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            fix_permissions(&path, depth + 1);
        } else {
            let name = entry.file_name().to_string_lossy().to_lowercase();
            let script =
                name.ends_with(".sh") || name.ends_with(".appimage") || name.ends_with(".x86_64");
            if script || is_elf(&path) {
                let _ = make_executable(&path);
            }
        }
    }
}

pub fn dir_size(dir: &Path) -> u64 {
    let Ok(entries) = fs::read_dir(dir) else {
        return 0;
    };
    entries
        .flatten()
        .map(|e| {
            let path = e.path();
            if path.is_dir() {
                dir_size(&path)
            } else {
                e.metadata().map_or(0, |m| m.len())
            }
        })
        .sum()
}

/// Picks the game's executable: native first (Linux binary, `AppImage`, script,
/// desktop entry), then a Windows .exe (run with Proton on Linux).
pub fn pick_executable(candidates: &[ScanCandidate]) -> Option<(String, CompatibilityTool)> {
    let native_rank = |kind: ExecutableKind| -> Option<u8> {
        if cfg!(windows) {
            return (kind == ExecutableKind::WindowsExe).then_some(0);
        }
        match kind {
            ExecutableKind::LinuxBinary => Some(0),
            ExecutableKind::Appimage => Some(1),
            ExecutableKind::Script => Some(2),
            ExecutableKind::DesktopEntry => Some(3),
            ExecutableKind::WindowsExe => None,
        }
    };
    let native = candidates
        .iter()
        .filter_map(|c| native_rank(c.kind).map(|rank| (rank, c)))
        .min_by_key(|(rank, c)| (*rank, std::cmp::Reverse(c.size_bytes)));
    if let Some((_, c)) = native {
        return Some((c.executable_path.clone(), CompatibilityTool::Native));
    }
    candidates
        .iter()
        .filter(|c| c.kind == ExecutableKind::WindowsExe)
        .max_by_key(|c| c.size_bytes)
        .map(|c| {
            (
                c.executable_path.clone(),
                if cfg!(windows) {
                    CompatibilityTool::Native
                } else {
                    CompatibilityTool::Proton
                },
            )
        })
}

/// Finds the executable in a freshly installed folder.
pub fn find_executable(dir: &Path) -> Option<(String, CompatibilityTool)> {
    let candidates = crate::scanner::scan(dir, &HashSet::new()).ok()?;
    pick_executable(&candidates)
}

/// A free folder name below `root` for the game.
pub fn unique_dir(root: &Path, name: &str) -> PathBuf {
    let base = crate::db::slugify(name);
    let base = if base.is_empty() {
        "spiel".to_owned()
    } else {
        base
    };
    let mut candidate = root.join(&base);
    let mut n = 2;
    while candidate.exists() {
        candidate = root.join(format!("{base}-{n}"));
        n += 1;
    }
    candidate
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    fn elf(size: usize) -> Vec<u8> {
        let mut bytes = b"\x7fELF".to_vec();
        bytes.resize(size, 0);
        bytes
    }

    #[test]
    fn rejects_paths_outside_the_folder() {
        assert_eq!(
            safe_relative("game/bin/run"),
            Some(PathBuf::from("game/bin/run"))
        );
        assert_eq!(safe_relative("./a"), Some(PathBuf::from("a")));
        assert_eq!(safe_relative("../evil"), None);
        assert_eq!(safe_relative("/etc/passwd"), None);
        assert_eq!(safe_relative("a/../../b"), None);
    }

    #[test]
    fn detects_archive_kinds() {
        assert_eq!(archive_kind("Game-1.0.ZIP"), ArchiveKind::Zip);
        assert_eq!(archive_kind("g.7z"), ArchiveKind::SevenZ);
        assert_eq!(archive_kind("g.tar.gz"), ArchiveKind::TarGz);
        assert_eq!(archive_kind("g.tgz"), ArchiveKind::TarGz);
        assert_eq!(archive_kind("g.tar"), ArchiveKind::Tar);
        assert_eq!(archive_kind("Game.AppImage"), ArchiveKind::Plain);
    }

    #[test]
    fn unpacks_zip_and_finds_the_binary_even_without_exec_bit() {
        let dir = tempfile::tempdir().unwrap();
        let archive = dir.path().join("lantern.zip");
        let mut zip = zip::ZipWriter::new(std::fs::File::create(&archive).unwrap());
        let options = zip::write::SimpleFileOptions::default();
        zip.start_file("Lantern/Lantern.x86_64", options).unwrap();
        zip.write_all(&elf(8192)).unwrap();
        zip.start_file("Lantern/data/level1.dat", options).unwrap();
        zip.write_all(b"level").unwrap();
        zip.start_file("../escape.txt", options).unwrap();
        zip.write_all(b"nope").unwrap();
        zip.finish().unwrap();

        let dest = dir.path().join("install");
        unpack(&archive, "lantern.zip", &dest).unwrap();
        assert!(dest.join("Lantern/data/level1.dat").is_file());
        assert!(!dir.path().join("escape.txt").exists());
        if cfg!(target_os = "linux") {
            let (exe, compat) = find_executable(&dest).unwrap();
            assert!(exe.ends_with("Lantern/Lantern.x86_64"), "{exe}");
            assert_eq!(compat, CompatibilityTool::Native);
        }
    }

    #[test]
    fn unpacks_tar_gz_and_7z() {
        let dir = tempfile::tempdir().unwrap();
        // tar.gz
        let tgz = dir.path().join("g.tar.gz");
        {
            let encoder = flate2::write::GzEncoder::new(
                std::fs::File::create(&tgz).unwrap(),
                flate2::Compression::fast(),
            );
            let mut builder = tar::Builder::new(encoder);
            let content = b"#!/bin/sh\necho hi\n";
            let mut header = tar::Header::new_gnu();
            header.set_size(content.len() as u64);
            header.set_mode(0o644);
            header.set_cksum();
            builder
                .append_data(&mut header, "game/start.sh", &content[..])
                .unwrap();
            builder.into_inner().unwrap().finish().unwrap();
        }
        let dest = dir.path().join("tgz");
        unpack(&tgz, "g.tar.gz", &dest).unwrap();
        assert!(dest.join("game/start.sh").is_file());

        // 7z
        let src = dir.path().join("src");
        std::fs::create_dir_all(src.join("game")).unwrap();
        std::fs::write(src.join("game/readme.txt"), b"hello").unwrap();
        let seven = dir.path().join("g.7z");
        sevenz_rust2::compress_to_path(&src, &seven).unwrap();
        let dest = dir.path().join("7z");
        unpack(&seven, "g.7z", &dest).unwrap();
        assert_eq!(
            std::fs::read(dest.join("game/readme.txt")).unwrap(),
            b"hello"
        );
    }

    #[test]
    fn single_files_are_moved_and_made_runnable() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("download");
        std::fs::write(&file, elf(8192)).unwrap();
        let dest = dir.path().join("game");
        unpack(&file, "Game.AppImage", &dest).unwrap();
        let target = dest.join("Game.AppImage");
        assert!(target.is_file() && !file.exists());
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            assert_ne!(
                std::fs::metadata(&target).unwrap().permissions().mode() & 0o111,
                0
            );
        }
        assert_eq!(unique_dir(dir.path(), "Game"), dir.path().join("game-2"));
    }

    #[test]
    fn prefers_native_executables() {
        let c = |path: &str, kind, size| ScanCandidate {
            title: "x".into(),
            executable_path: path.into(),
            size_bytes: size,
            kind,
            already_in_library: false,
        };
        let both = [
            c("/g/win.exe", ExecutableKind::WindowsExe, 900),
            c("/g/run", ExecutableKind::LinuxBinary, 10),
        ];
        let windows_only = [
            c("/g/setup.exe", ExecutableKind::WindowsExe, 100),
            c("/g/game.exe", ExecutableKind::WindowsExe, 900),
        ];
        if cfg!(windows) {
            assert_eq!(pick_executable(&both).unwrap().0, "/g/win.exe");
        } else {
            assert_eq!(
                pick_executable(&both).unwrap(),
                ("/g/run".into(), CompatibilityTool::Native)
            );
            assert_eq!(
                pick_executable(&windows_only).unwrap(),
                ("/g/game.exe".into(), CompatibilityTool::Proton)
            );
        }
        assert_eq!(pick_executable(&[]), None);
    }
}
