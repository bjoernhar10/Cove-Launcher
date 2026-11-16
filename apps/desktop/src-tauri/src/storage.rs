//! "Speicherplatz": how much room installed games take, per drive.
//!
//! Game folders are measured on demand (links are not followed, so a Proton
//! prefix's `z:` or a symlinked library is never counted twice); the result
//! goes into `library_entries.install_size_bytes`.

use std::path::{Path, PathBuf};

use futures_util::StreamExt;
use sqlx::SqlitePool;

use crate::db;
use crate::error::AppResult;
use crate::models::{DriveUsage, StorageGame, StorageOverview};

/// Folders measured at the same time.
const PARALLEL: usize = 4;

/// Bytes in `dir`, without following links.
pub fn folder_bytes(dir: &Path) -> u64 {
    let mut total = 0;
    let mut stack = vec![dir.to_path_buf()];
    while let Some(dir) = stack.pop() {
        let Ok(entries) = std::fs::read_dir(&dir) else {
            continue;
        };
        for entry in entries.flatten() {
            let Ok(kind) = entry.file_type() else {
                continue;
            };
            if kind.is_dir() {
                stack.push(entry.path());
            } else if kind.is_file() {
                total += entry.metadata().map_or(0, |m| m.len());
            }
        }
    }
    total
}

/// Only a game's own folder is measured: not a folder another game lies in
/// or shares (ROM folders, a manual game started from `~/bin`), not the home
/// folder or one above it, not a drive's root.
fn measurable(path: &Path, all: &[PathBuf], home: Option<&Path>) -> bool {
    let shared = all
        .iter()
        .filter(|other| other.starts_with(path))
        .nth(1)
        .is_some();
    let too_high = home.is_some_and(|home| home.starts_with(path)) || path.components().count() < 3;
    !shared && !too_high
}

struct Disk {
    name: String,
    mount: PathBuf,
    total: u64,
    free: u64,
}

fn disks() -> Vec<Disk> {
    sysinfo::Disks::new_with_refreshed_list()
        .list()
        .iter()
        .map(|d| Disk {
            name: d.name().to_string_lossy().into_owned(),
            mount: d.mount_point().to_path_buf(),
            total: d.total_space(),
            free: d.available_space(),
        })
        .collect()
}

/// The disk holding `path`: the longest mount point it lies under.
fn disk_of<'a>(disks: &'a [Disk], path: &Path) -> Option<&'a Disk> {
    let dir = crate::folders::nearest_existing(path)?;
    disks
        .iter()
        .filter(|d| crate::folders::on_disk(&dir, &d.mount))
        .max_by_key(|d| d.mount.as_os_str().len())
}

/// Measures every installed game and sums them up per drive. Drives without
/// games show up when they hold one of the games folders.
pub async fn overview(pool: &SqlitePool, games_folders: &[String]) -> AppResult<StorageOverview> {
    let installed: Vec<(String, PathBuf)> = db::library::list(pool)
        .await?
        .into_iter()
        .filter(|item| item.entry.installed)
        .filter_map(|item| {
            let path = item.entry.install_path.filter(|p| !p.is_empty())?;
            Some((item.game.id, PathBuf::from(path)))
        })
        .collect();
    let home =
        std::env::var_os(if cfg!(windows) { "USERPROFILE" } else { "HOME" }).map(PathBuf::from);
    let paths: Vec<PathBuf> = installed.iter().map(|(_, p)| p.clone()).collect();
    let measured: Vec<(String, PathBuf, Option<u64>)> = futures_util::stream::iter(installed)
        .map(|(id, path)| {
            let measurable = measurable(&path, &paths, home.as_deref());
            (id, path, measurable)
        })
        .map(|(id, path, measurable)| async move {
            if !measurable {
                return (id, path, None);
            }
            let dir = path.clone();
            let size =
                tokio::task::spawn_blocking(move || dir.is_dir().then(|| folder_bytes(&dir)))
                    .await
                    .ok()
                    .flatten();
            (id, path, size)
        })
        .buffer_unordered(PARALLEL)
        .collect()
        .await;

    let all = tokio::task::spawn_blocking(disks).await.unwrap_or_default();
    let mut drives: Vec<DriveUsage> = Vec::new();
    let mut games = Vec::with_capacity(measured.len());
    let drive_index = |disk: &Disk, drives: &mut Vec<DriveUsage>| {
        let mount = disk.mount.to_string_lossy().into_owned();
        if let Some(i) = drives.iter().position(|d| d.mount_point == mount) {
            return i;
        }
        drives.push(DriveUsage {
            name: disk.name.clone(),
            mount_point: mount,
            total_bytes: i64::try_from(disk.total).unwrap_or(i64::MAX),
            free_bytes: i64::try_from(disk.free).unwrap_or(i64::MAX),
            games_bytes: 0,
        });
        drives.len() - 1
    };
    for (game_id, path, size) in measured {
        let bytes = size.map(|s| i64::try_from(s).unwrap_or(i64::MAX));
        if let Some(bytes) = bytes {
            db::library::set_install_size(pool, &game_id, bytes).await?;
        }
        let mount_point = disk_of(&all, &path).map(|disk| {
            let i = drive_index(disk, &mut drives);
            drives[i].games_bytes += bytes.unwrap_or(0);
            drives[i].mount_point.clone()
        });
        games.push(StorageGame {
            game_id,
            size_bytes: bytes,
            mount_point,
        });
    }
    for folder in games_folders {
        if let Some(disk) = disk_of(&all, Path::new(folder)) {
            drive_index(disk, &mut drives);
        }
    }
    games.sort_by_key(|g| std::cmp::Reverse(g.size_bytes));
    drives.sort_by(|a, b| a.mount_point.cmp(&b.mount_point));
    Ok(StorageOverview { drives, games })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn counts_files_but_not_links() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join("game/data")).unwrap();
        std::fs::write(dir.path().join("game/run"), [0_u8; 100]).unwrap();
        std::fs::write(dir.path().join("game/data/pak"), [0_u8; 1000]).unwrap();
        #[cfg(unix)]
        std::os::unix::fs::symlink(dir.path(), dir.path().join("game/data/loop")).unwrap();
        assert_eq!(folder_bytes(&dir.path().join("game")), 1100);
        assert_eq!(folder_bytes(&dir.path().join("missing")), 0);
    }

    #[test]
    fn measures_only_a_game_s_own_folder() {
        let all: Vec<PathBuf> = [
            "/home/u/Games/Lantern",
            "/home/u/roms/snes",
            "/home/u/roms/snes",
            "/home/u/Games/Lantern/mods/Extra",
        ]
        .iter()
        .map(PathBuf::from)
        .collect();
        let home = Some(Path::new("/home/u"));
        assert!(measurable(
            Path::new("/home/u/Games/Lantern/mods/Extra"),
            &all,
            home
        ));
        assert!(
            !measurable(Path::new("/home/u/Games/Lantern"), &all, home),
            "holds another game"
        );
        assert!(
            !measurable(Path::new("/home/u/roms/snes"), &all, home),
            "shared by ROMs"
        );
        assert!(!measurable(
            Path::new("/home/u"),
            &[PathBuf::from("/home/u")],
            home
        ));
        assert!(!measurable(
            Path::new("/opt"),
            &[PathBuf::from("/opt")],
            home
        ));
    }

    #[tokio::test]
    async fn measures_installed_games_per_drive() {
        let pool = db::open_in_memory().await.unwrap();
        let dir = tempfile::tempdir().unwrap();
        let game = dir.path().join("Lantern");
        std::fs::create_dir_all(&game).unwrap();
        std::fs::write(game.join("Lantern.x86_64"), [0_u8; 2048]).unwrap();
        let item = db::library::add_manual(
            &pool,
            &crate::models::ManualGameInput {
                title: "Lantern".into(),
                executable_path: game.join("Lantern.x86_64").to_string_lossy().into_owned(),
                size_bytes: None,
            },
        )
        .await
        .unwrap();
        let folders = vec![dir.path().to_string_lossy().into_owned()];
        let overview = overview(&pool, &folders).await.unwrap();
        let measured = overview
            .games
            .iter()
            .find(|g| g.game_id == item.game.id)
            .expect("installed game listed");
        assert_eq!(measured.size_bytes, Some(2048));
        let stored = db::library::require(&pool, &item.game.id).await.unwrap();
        assert_eq!(stored.entry.install_size_bytes, Some(2048));
        if let Some(mount) = &measured.mount_point {
            let drive = overview
                .drives
                .iter()
                .find(|d| &d.mount_point == mount)
                .unwrap();
            assert!(drive.games_bytes >= 2048 && drive.total_bytes > 0);
        }
    }
}
