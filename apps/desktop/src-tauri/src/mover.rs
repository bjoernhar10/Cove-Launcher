//! "Spiel verschieben": moves a game folder into another games folder.
//! On the same drive the folder is simply renamed; otherwise it is copied,
//! the copy is checked file by file, the old folder is deleted and the paths
//! in the database are updated. Progress goes out as `game-move-progress`.

use std::collections::HashSet;
use std::io::{Read, Write};
use std::path::{Component, Path, PathBuf};
use std::sync::{Arc, Mutex, PoisonError};
use std::time::{Duration, Instant};

use sqlx::SqlitePool;

use crate::db;
use crate::error::{AppError, AppResult};
use crate::folders;
use crate::models::{GameId, GameMoveProgress, LibraryItem, MoveStatus, StoreSource};

pub type MoveEmit = Arc<dyn Fn(GameMoveProgress) + Send + Sync>;

const REPORT_EVERY: Duration = Duration::from_millis(250);
const BUFFER: usize = 1 << 20;

pub struct Mover {
    pool: SqlitePool,
    active: Mutex<HashSet<GameId>>,
    emit: MoveEmit,
}

/// The install folder of a game Cove may move, or why not.
pub fn movable(item: &LibraryItem) -> AppResult<PathBuf> {
    let entry = &item.entry;
    match entry.source {
        StoreSource::Manual => {}
        StoreSource::Steam => {
            return Err(AppError::InvalidInput(tr!(
                "Steam-Spiele verschiebst du in Steam: Eigenschaften → Installierte Dateien → Installationsordner verschieben.",
                "Move Steam games in Steam: Properties → Installed Files → Move install folder."
            )))
        }
        StoreSource::Epic | StoreSource::Gog | StoreSource::Amazon if entry.installed_by_cove => {}
        StoreSource::Epic | StoreSource::Gog | StoreSource::Amazon => {
            return Err(AppError::InvalidInput(tr!(
                "Dieses Spiel wird von seinem Store-Client verwaltet und kann nur dort verschoben werden.",
                "This game is managed by its store client and can only be moved there."
            )))
        }
    }
    let folder = entry
        .install_path
        .as_deref()
        .filter(|_| entry.installed)
        .map(PathBuf::from)
        .filter(|p| p.is_dir())
        .ok_or_else(|| {
            AppError::InvalidInput(tr!(
                "Der Spielordner wurde nicht gefunden.",
                "The game folder wasn't found."
            ))
        })?;
    // Never move a drive, the home folder or something like /usr.
    let depth = folder
        .components()
        .filter(|c| matches!(c, Component::Normal(_)))
        .count();
    let home = std::env::var_os("HOME")
        .or_else(|| std::env::var_os("USERPROFILE"))
        .map(PathBuf::from);
    let outside = entry
        .executable_path
        .as_deref()
        .is_some_and(|exe| !Path::new(exe).starts_with(&folder));
    if depth < 2 || home.as_deref() == Some(folder.as_path()) || outside {
        return Err(AppError::InvalidInput(tr!(
            "Dieser Ordner kann nicht verschoben werden.",
            "This folder can't be moved."
        )));
    }
    Ok(folder)
}

#[derive(Debug)]
enum Kind {
    Dir,
    File(u64),
    #[cfg_attr(not(unix), allow(dead_code))]
    Link(PathBuf),
}

fn walk(root: &Path, rel: &Path, out: &mut Vec<(PathBuf, Kind)>) -> std::io::Result<()> {
    for entry in std::fs::read_dir(root.join(rel))? {
        let entry = entry?;
        let path = rel.join(entry.file_name());
        let meta = std::fs::symlink_metadata(entry.path())?;
        if cfg!(unix) && meta.file_type().is_symlink() {
            out.push((path, Kind::Link(std::fs::read_link(entry.path())?)));
        } else if entry.path().is_dir() {
            out.push((path.clone(), Kind::Dir));
            walk(root, &path, out)?;
        } else {
            out.push((path, Kind::File(std::fs::metadata(entry.path())?.len())));
        }
    }
    Ok(())
}

fn copy_file(
    from: &Path,
    to: &Path,
    copied: &mut u64,
    report: &mut dyn FnMut(u64),
) -> std::io::Result<()> {
    let mut reader = std::fs::File::open(from)?;
    let mut writer = std::fs::File::create(to)?;
    let mut buffer = vec![0u8; BUFFER];
    loop {
        let n = reader.read(&mut buffer)?;
        if n == 0 {
            break;
        }
        writer.write_all(&buffer[..n])?;
        *copied += n as u64;
        report(*copied);
    }
    writer.flush()?;
    std::fs::set_permissions(to, std::fs::metadata(from)?.permissions())
}

#[cfg_attr(not(unix), allow(clippy::unnecessary_wraps))] // symlinks only exist on Unix
fn link(target: &Path, at: &Path) -> std::io::Result<()> {
    #[cfg(unix)]
    {
        std::os::unix::fs::symlink(target, at)
    }
    #[cfg(not(unix))]
    {
        let _ = (target, at);
        Ok(())
    }
}

/// Copies `source` to `target` and checks every file's size. Leaves nothing
/// behind on failure.
fn copy_tree(
    source: &Path,
    target: &Path,
    report: &mut dyn FnMut(MoveStatus, u64, u64),
) -> std::io::Result<()> {
    let mut items = Vec::new();
    walk(source, Path::new(""), &mut items)?;
    let total: u64 = items
        .iter()
        .map(|(_, k)| if let Kind::File(size) = k { *size } else { 0 })
        .sum();
    let result = (|| {
        std::fs::create_dir_all(target)?;
        let mut copied = 0u64;
        for (rel, kind) in &items {
            let to = target.join(rel);
            match kind {
                Kind::Dir => std::fs::create_dir_all(&to)?,
                Kind::Link(dest) => link(dest, &to)?,
                Kind::File(_) => copy_file(&source.join(rel), &to, &mut copied, &mut |c| {
                    report(MoveStatus::Copying, c, total);
                })?,
            }
        }
        report(MoveStatus::Verifying, copied, total);
        for (rel, kind) in &items {
            let ok = match kind {
                Kind::Dir => target.join(rel).is_dir(),
                Kind::Link(_) => std::fs::symlink_metadata(target.join(rel)).is_ok(),
                Kind::File(size) => {
                    std::fs::metadata(target.join(rel)).is_ok_and(|m| m.len() == *size)
                }
            };
            if !ok {
                return Err(std::io::Error::other(format!(
                    "Kopie unvollständig: {}",
                    rel.display()
                )));
            }
        }
        Ok(())
    })();
    if result.is_err() {
        let _ = std::fs::remove_dir_all(target);
    }
    result
}

/// Moves the folder: rename on the same drive, else copy + check + delete.
fn relocate(
    source: &Path,
    target: &Path,
    report: &mut dyn FnMut(MoveStatus, u64, u64),
) -> std::io::Result<()> {
    if let Some(parent) = target.parent() {
        std::fs::create_dir_all(parent)?;
    }
    if std::fs::rename(source, target).is_ok() {
        return Ok(());
    }
    copy_tree(source, target, report)?;
    if let Err(error) = std::fs::remove_dir_all(source) {
        // The game works from the new folder; leftovers only cost space.
        eprintln!(
            "[cove] Alter Spielordner {} nicht ganz gelöscht: {error}",
            source.display()
        );
    }
    Ok(())
}

/// The executable's path inside the moved folder.
fn rebase(executable: Option<&str>, from: &Path, to: &Path) -> Option<String> {
    let exe = Path::new(executable?);
    let rel = exe.strip_prefix(from).ok()?;
    Some(to.join(rel).to_string_lossy().into_owned())
}

impl Mover {
    pub fn new(pool: SqlitePool, emit: MoveEmit) -> Self {
        Self {
            pool,
            active: Mutex::new(HashSet::new()),
            emit,
        }
    }

    fn active(&self) -> std::sync::MutexGuard<'_, HashSet<GameId>> {
        self.active.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// Checks everything that can be checked up front, then moves in the background.
    pub fn start(self: &Arc<Self>, item: &LibraryItem, folder: &str) -> AppResult<()> {
        let source = movable(item)?;
        let root = folders::require_install_folder(folder)?;
        let canonical = |p: &Path| p.canonicalize().unwrap_or_else(|_| p.to_path_buf());
        let (source_c, root_c) = (canonical(&source), canonical(&root));
        if root_c.starts_with(&source_c) {
            return Err(AppError::InvalidInput(tr!(
                "Der Zielordner liegt im Spielordner selbst.",
                "The target folder is inside the game folder."
            )));
        }
        if source_c.parent() == Some(root_c.as_path()) {
            return Err(AppError::InvalidInput(tr!(
                "Das Spiel liegt schon in diesem Ordner.",
                "The game is already in this folder."
            )));
        }
        let game_id = item.game.id.clone();
        if !self.active().insert(game_id.clone()) {
            return Err(AppError::InvalidInput(tr!(
                "Dieses Spiel wird gerade verschoben.",
                "This game is being moved already."
            )));
        }
        let target = folders::unique_dir(&root, &item.game.title);
        let executable = item.entry.executable_path.clone();
        let this = self.clone();
        tauri::async_runtime::spawn(async move {
            let result = this
                .run(&game_id, &source, &target, executable.as_deref())
                .await;
            this.active().remove(&game_id);
            let error = result.err().map(|e| {
                eprintln!("[cove] Verschieben von {game_id}: {e}");
                e.user_message()
            });
            (this.emit)(GameMoveProgress {
                game_id,
                status: if error.is_some() {
                    MoveStatus::Failed
                } else {
                    MoveStatus::Completed
                },
                copied_bytes: 0,
                total_bytes: 0,
                error,
            });
        });
        Ok(())
    }

    async fn run(
        &self,
        game_id: &str,
        source: &Path,
        target: &Path,
        executable: Option<&str>,
    ) -> AppResult<()> {
        let emit = self.emit.clone();
        let id = game_id.to_owned();
        let (from, to) = (source.to_path_buf(), target.to_path_buf());
        tokio::task::spawn_blocking(move || {
            let mut last: Option<(Instant, MoveStatus)> = None;
            let mut report = |status: MoveStatus, copied: u64, total: u64| {
                let due = last.is_none_or(|(at, s)| s != status || at.elapsed() >= REPORT_EVERY);
                if due {
                    last = Some((Instant::now(), status));
                    emit(GameMoveProgress {
                        game_id: id.clone(),
                        status,
                        copied_bytes: i64::try_from(copied).unwrap_or(i64::MAX),
                        total_bytes: i64::try_from(total).unwrap_or(i64::MAX),
                        error: None,
                    });
                }
            };
            relocate(&from, &to, &mut report)
        })
        .await
        .map_err(|e| {
            eprintln!("[cove] Verschieben: {e}");
            AppError::Io(std::io::Error::other("move task failed"))
        })??;
        let exe = rebase(executable, source, target);
        db::library::set_install_location(
            &self.pool,
            game_id,
            &target.to_string_lossy(),
            exe.as_deref(),
        )
        .await
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::ManualGameInput;

    fn tree(root: &Path) {
        std::fs::create_dir_all(root.join("bin/data")).unwrap();
        std::fs::write(root.join("bin/game"), b"\x7fELF-game").unwrap();
        std::fs::write(root.join("bin/data/level.pak"), vec![7u8; 3 * BUFFER + 11]).unwrap();
        #[cfg(unix)]
        std::os::unix::fs::symlink("bin/game", root.join("start")).unwrap();
    }

    #[test]
    fn copies_checks_and_reports_progress() {
        let dir = tempfile::tempdir().unwrap();
        let (from, to) = (dir.path().join("a/Lantern"), dir.path().join("b/Lantern"));
        tree(&from);
        let mut seen = Vec::new();
        copy_tree(&from, &to, &mut |s, c, t| seen.push((s, c, t))).unwrap();
        assert_eq!(std::fs::read(to.join("bin/game")).unwrap(), b"\x7fELF-game");
        assert_eq!(
            std::fs::metadata(to.join("bin/data/level.pak"))
                .unwrap()
                .len(),
            3 * BUFFER as u64 + 11
        );
        #[cfg(unix)]
        assert_eq!(
            std::fs::read_link(to.join("start")).unwrap(),
            Path::new("bin/game")
        );
        let total = 3 * BUFFER as u64 + 11 + 9;
        assert!(seen.iter().any(|(s, _, _)| *s == MoveStatus::Copying));
        assert_eq!(seen.last(), Some(&(MoveStatus::Verifying, total, total)));
        assert!(from.is_dir(), "copy_tree keeps the source");
    }

    #[test]
    fn relocate_renames_and_removes_the_old_folder() {
        let dir = tempfile::tempdir().unwrap();
        let (from, to) = (dir.path().join("a/Lantern"), dir.path().join("b/c/Lantern"));
        tree(&from);
        relocate(&from, &to, &mut |_, _, _| {}).unwrap();
        assert!(!from.exists());
        assert!(to.join("bin/game").is_file());
    }

    #[test]
    fn executables_follow_the_folder() {
        assert_eq!(
            rebase(
                Some("/a/Lantern/bin/game"),
                Path::new("/a/Lantern"),
                Path::new("/b/Lantern")
            ),
            Some(
                Path::new("/b/Lantern")
                    .join("bin/game")
                    .to_string_lossy()
                    .into_owned()
            )
        );
        assert_eq!(rebase(None, Path::new("/a"), Path::new("/b")), None);
    }

    #[tokio::test]
    async fn only_safe_folders_of_cove_games_move() {
        let pool = db::open_in_memory().await.unwrap();
        let dir = tempfile::tempdir().unwrap();
        let folder = dir.path().join("Games/Lantern");
        tree(&folder);
        let item = db::library::add_manual(
            &pool,
            &ManualGameInput {
                title: "Lantern".into(),
                executable_path: folder.join("bin/game").to_string_lossy().into_owned(),
                size_bytes: None,
            },
        )
        .await
        .unwrap();
        let mut item = item;
        item.entry.install_path = Some(folder.to_string_lossy().into_owned());
        item.entry.installed = true;
        assert_eq!(movable(&item).unwrap(), folder);

        let mut steam = item.clone();
        steam.entry.source = StoreSource::Steam;
        assert!(matches!(movable(&steam), Err(AppError::InvalidInput(_))));

        let mut root = item.clone();
        root.entry.install_path = Some("/".into());
        root.entry.executable_path = None;
        assert!(movable(&root).is_err());

        let mut outside = item.clone();
        outside.entry.executable_path = Some("/usr/bin/game".into());
        assert!(movable(&outside).is_err());

        let mut missing = item;
        missing.entry.installed = false;
        assert!(movable(&missing).is_err());
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn moves_a_game_and_updates_its_paths() {
        let pool = db::open_in_memory().await.unwrap();
        let dir = tempfile::tempdir().unwrap();
        let folder = dir.path().join("Games/Lantern");
        tree(&folder);
        let added = db::library::add_manual(
            &pool,
            &ManualGameInput {
                title: "Lantern".into(),
                executable_path: folder.join("bin/game").to_string_lossy().into_owned(),
                size_bytes: None,
            },
        )
        .await
        .unwrap();
        db::library::set_install_location(
            &pool,
            &added.game.id,
            &folder.to_string_lossy(),
            Some(&folder.join("bin/game").to_string_lossy()),
        )
        .await
        .unwrap();
        let item = db::library::require(&pool, &added.game.id).await.unwrap();
        let events: Arc<Mutex<Vec<GameMoveProgress>>> = Arc::default();
        let sink = events.clone();
        let mover = Arc::new(Mover::new(
            pool.clone(),
            Arc::new(move |e| sink.lock().unwrap().push(e)),
        ));
        let other = dir.path().join("Other");
        assert!(mover
            .start(&item, &dir.path().join("Games").to_string_lossy())
            .is_err());
        assert!(mover
            .start(&item, &folder.join("bin").to_string_lossy())
            .is_err());
        mover.start(&item, &other.to_string_lossy()).unwrap();
        for _ in 0..200 {
            if events
                .lock()
                .unwrap()
                .iter()
                .any(|e| e.status == MoveStatus::Completed)
            {
                break;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
        assert!(
            events
                .lock()
                .unwrap()
                .iter()
                .any(|e| e.status == MoveStatus::Completed),
            "{:?}",
            events.lock().unwrap()
        );
        let after = db::library::require(&pool, &added.game.id).await.unwrap();
        let new_folder = other.join("Lantern");
        assert_eq!(
            after.entry.install_path.as_deref(),
            Some(&*new_folder.to_string_lossy())
        );
        assert_eq!(
            after.entry.executable_path.as_deref(),
            Some(&*new_folder.join("bin/game").to_string_lossy())
        );
        assert!(new_folder.join("bin/data/level.pak").is_file());
        assert!(!folder.exists());
    }
}
