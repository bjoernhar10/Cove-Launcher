//! Games folders: the folder name Cove creates per game, free space, write
//! access and the "Program Files" warning shown in the install dialog.

use std::path::{Path, PathBuf};

use crate::error::{AppError, AppResult};
use crate::models::FolderInfo;

/// Windows names that can't be used as a folder name.
const RESERVED: [&str; 22] = [
    "CON", "PRN", "AUX", "NUL", "COM1", "COM2", "COM3", "COM4", "COM5", "COM6", "COM7", "COM8",
    "COM9", "LPT1", "LPT2", "LPT3", "LPT4", "LPT5", "LPT6", "LPT7", "LPT8", "LPT9",
];
const MAX_NAME: usize = 80;

/// Folder name for a game: "Half-Life 2: Episode One" → "Half-Life 2 Episode One".
/// Keeps letters, digits, spaces, `-`, `_`, `.`, `'`, `(` and `)`, works on every OS.
pub fn folder_name(title: &str) -> String {
    let kept: String = title
        .replace('&', " and ")
        .chars()
        .map(|c| if c.is_whitespace() { ' ' } else { c })
        .filter(|c| c.is_alphanumeric() || " -_.'()".contains(*c))
        .collect();
    let mut name = kept.split_whitespace().collect::<Vec<_>>().join(" ");
    if name.chars().count() > MAX_NAME {
        name = name.chars().take(MAX_NAME).collect();
    }
    // Windows drops trailing dots and spaces; a leading dot hides the folder on Linux.
    let name = name.trim_matches(|c: char| c == '.' || c == ' ').to_owned();
    if name.is_empty() {
        return "Game".into();
    }
    if RESERVED.iter().any(|r| r.eq_ignore_ascii_case(&name)) {
        return format!("{name} Game");
    }
    name
}

/// `root/<folder name>`, with " 2", " 3" … when that folder exists already.
pub fn unique_dir(root: &Path, title: &str) -> PathBuf {
    let base = folder_name(title);
    let mut candidate = root.join(&base);
    let mut n = 2;
    while candidate.exists() {
        candidate = root.join(format!("{base} {n}"));
        n += 1;
    }
    candidate
}

/// The folder itself or its closest existing parent.
fn nearest_existing(path: &Path) -> Option<PathBuf> {
    path.ancestors().find(|p| p.is_dir()).map(|p| {
        p.canonicalize()
            .map_or_else(|_| p.to_path_buf(), without_verbatim)
    })
}

/// `canonicalize` on Windows returns `\\?\C:\…`, while disks and the
/// "Program Files" check use plain `C:\…` paths.
fn without_verbatim(path: PathBuf) -> PathBuf {
    let text = path.to_string_lossy();
    if let Some(rest) = text.strip_prefix(r"\\?\UNC\") {
        return PathBuf::from(format!(r"\\{rest}"));
    }
    if let Some(rest) = text.strip_prefix(r"\\?\") {
        return PathBuf::from(rest);
    }
    path
}

/// Is `dir` on the disk mounted at `mount`? Windows paths ignore case.
fn on_disk(dir: &Path, mount: &Path) -> bool {
    if cfg!(windows) {
        dir.to_string_lossy()
            .to_lowercase()
            .starts_with(&mount.to_string_lossy().to_lowercase())
    } else {
        dir.starts_with(mount)
    }
}

/// Can Cove create files there? Tested on the closest existing folder with a
/// temporary file (permissions alone don't tell on Windows).
pub fn writable(path: &Path) -> bool {
    let Some(dir) = nearest_existing(path) else {
        return false;
    };
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_nanos());
    let probe = dir.join(format!(".cove-write-test-{}-{nanos}", std::process::id()));
    match std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&probe)
    {
        Ok(file) => {
            drop(file);
            let _ = std::fs::remove_file(&probe);
            true
        }
        Err(_) => false,
    }
}

/// Is `path` below one of `roots` (case-insensitive, `/` and `\` alike)?
fn below_any(path: &str, roots: &[String]) -> bool {
    let norm = |p: &str| {
        let mut p = p.replace('/', "\\").to_lowercase();
        while p.ends_with('\\') {
            p.pop();
        }
        p
    };
    let path = norm(path);
    roots
        .iter()
        .map(|r| norm(r))
        .filter(|r| !r.is_empty())
        .any(|r| path == r || path.starts_with(&format!("{r}\\")))
}

/// Windows: below "Program Files", which needs admin rights.
pub fn needs_admin(path: &Path) -> bool {
    if !cfg!(windows) {
        return false;
    }
    let roots: Vec<String> = ["ProgramFiles", "ProgramFiles(x86)", "ProgramW6432"]
        .iter()
        .filter_map(std::env::var_os)
        .map(|p| p.to_string_lossy().into_owned())
        .collect();
    below_any(&path.to_string_lossy(), &roots)
}

/// Free space on the drive holding `path` (the disk with the longest matching mount point).
pub fn free_bytes(path: &Path) -> Option<u64> {
    let dir = nearest_existing(path)?;
    let disks = sysinfo::Disks::new_with_refreshed_list();
    disks
        .list()
        .iter()
        .filter(|d| on_disk(&dir, d.mount_point()))
        .max_by_key(|d| d.mount_point().as_os_str().len())
        .map(sysinfo::Disk::available_space)
}

/// A games folder must be a full path.
pub fn require_absolute(path: &str) -> AppResult<PathBuf> {
    let trimmed = path.trim();
    let dir = PathBuf::from(trimmed);
    if trimmed.is_empty() || !dir.is_absolute() {
        return Err(AppError::InvalidInput(tr!(
            "Bitte wähle einen vollständigen Ordnerpfad.",
            "Please choose a full folder path."
        )));
    }
    Ok(dir)
}

pub fn info(path: &str) -> AppResult<FolderInfo> {
    let dir = require_absolute(path)?;
    Ok(FolderInfo {
        path: dir.to_string_lossy().into_owned(),
        free_bytes: free_bytes(&dir).and_then(|b| i64::try_from(b).ok()),
        writable: writable(&dir),
        needs_admin: needs_admin(&dir),
    })
}

/// Checked right before a download is queued into `path`.
pub fn require_install_folder(path: &str) -> AppResult<PathBuf> {
    let dir = require_absolute(path)?;
    if !writable(&dir) {
        return Err(AppError::InvalidInput(tr!(
            "Cove darf in diesen Ordner nicht schreiben. Wähle einen anderen Ordner.",
            "Cove can't write to this folder. Choose a different folder."
        )));
    }
    Ok(dir)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn windows_verbatim_paths_become_plain() {
        assert_eq!(
            without_verbatim(PathBuf::from(r"\\?\C:\Users\me\Games")),
            PathBuf::from(r"C:\Users\me\Games")
        );
        assert_eq!(
            without_verbatim(PathBuf::from(r"\\?\UNC\nas\games")),
            PathBuf::from(r"\\nas\games")
        );
        assert_eq!(
            without_verbatim(PathBuf::from("/home/me")),
            PathBuf::from("/home/me")
        );
        assert!(on_disk(Path::new("/home/me/Games"), Path::new("/home")));
        assert!(!on_disk(Path::new("/home/me"), Path::new("/mnt")));
    }

    #[test]
    fn folder_names_drop_special_characters() {
        assert_eq!(folder_name("Saltmarsh"), "Saltmarsh");
        assert_eq!(
            folder_name("Half-Life 2: Episode One"),
            "Half-Life 2 Episode One"
        );
        assert_eq!(folder_name("Ember & Oak"), "Ember and Oak");
        assert_eq!(
            folder_name("Baldur's Gate 3 (Deluxe)"),
            "Baldur's Gate 3 (Deluxe)"
        );
        assert_eq!(folder_name("What?/\\*<>|\"Now"), "WhatNow");
        assert_eq!(folder_name("Ōkami HD"), "Ōkami HD");
        assert_eq!(folder_name("  ...hidden.  "), "hidden");
        assert_eq!(folder_name("???"), "Game");
        assert_eq!(folder_name("con"), "con Game");
        assert_eq!(folder_name(&"x".repeat(200)).len(), MAX_NAME);
    }

    #[test]
    fn unique_dirs_count_up() {
        let root = tempfile::tempdir().unwrap();
        assert_eq!(
            unique_dir(root.path(), "Lantern"),
            root.path().join("Lantern")
        );
        std::fs::create_dir(root.path().join("Lantern")).unwrap();
        assert_eq!(
            unique_dir(root.path(), "Lantern"),
            root.path().join("Lantern 2")
        );
    }

    #[test]
    fn program_files_needs_admin() {
        let roots = vec![
            "C:\\Program Files".to_owned(),
            "C:\\Program Files (x86)".to_owned(),
        ];
        assert!(below_any("C:\\Program Files\\Games", &roots));
        assert!(below_any("c:/program files (x86)/X", &roots));
        assert!(below_any("C:\\Program Files\\", &roots));
        assert!(!below_any("C:\\Program Files Games", &roots));
        assert!(!below_any("D:\\Games", &roots));
    }

    #[test]
    fn folder_info_for_new_and_existing_folders() {
        let root = tempfile::tempdir().unwrap();
        let info = info(&root.path().join("new/Games").to_string_lossy()).unwrap();
        assert!(info.writable, "a missing folder is checked on its parent");
        assert!(!info.needs_admin);
        assert!(info.free_bytes.is_some_and(|b| b > 0));
        assert!(matches!(
            super::info("Games"),
            Err(AppError::InvalidInput(_))
        ));
        assert!(matches!(super::info("  "), Err(AppError::InvalidInput(_))));
    }

    #[cfg(unix)]
    #[test]
    fn read_only_folders_are_not_writable() {
        use std::os::unix::fs::PermissionsExt;
        let root = tempfile::tempdir().unwrap();
        let locked = root.path().join("locked");
        std::fs::create_dir(&locked).unwrap();
        std::fs::set_permissions(&locked, std::fs::Permissions::from_mode(0o555)).unwrap();
        // root ignores permissions; only check when they apply.
        if !writable(&locked) {
            assert!(matches!(
                require_install_folder(&locked.to_string_lossy()),
                Err(AppError::InvalidInput(_))
            ));
        }
        std::fs::set_permissions(&locked, std::fs::Permissions::from_mode(0o755)).unwrap();
        assert!(require_install_folder(&locked.to_string_lossy()).is_ok());
    }
}
