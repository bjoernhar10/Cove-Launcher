//! Mods: files that go into a game's folder, switched on and off by Cove.
//!
//! A mod is an archive (zip, 7z, tar), a folder or a single file (`.pak`,
//! `.dll` …). Cove unpacks it once into its own folder (`<app data>/mods/
//! <game>/<mod>`) and from there copies the switched-on mods into the game
//! folder, in their order: a later mod wins over an earlier one for the same
//! file. Game files a mod replaces are moved to `<game>/.cove-originals` and
//! come back when no mod needs that place any more.
//!
//! Every change runs the same two steps (`apply`): take out what Cove put in
//! (only files that are still exactly as Cove wrote them; a file the game or
//! an update changed since stays), put the originals back, then copy the
//! switched-on mods in again. What is in the game folder is recorded in
//! `mod_files` / `mod_dirs`. Copies, not links: a game that changes a mod
//! file must never change Cove's copy. Paths are checked like archive entries
//! (no `..`, no absolute paths), links are never followed.

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::UNIX_EPOCH;

use sqlx::{Row, SqlitePool};

use crate::downloads::extract::{archive_kind, safe_relative, unpack, ArchiveKind};
use crate::error::{AppError, AppResult};
use crate::models::{now_iso, LibraryItem, ModInfo};

/// Game files that mods replaced, inside the game folder (same drive: moving is instant).
pub const ORIGINALS: &str = ".cove-originals";

pub struct Mods {
    pool: SqlitePool,
    /// `<app data>/mods`
    root: PathBuf,
}

/// A file Cove copied into the game folder.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Placed {
    path: String,
    mod_id: String,
    size: u64,
    modified: i64,
}

fn io_error(error: impl std::fmt::Display) -> AppError {
    AppError::Io(std::io::Error::other(error.to_string()))
}

fn modified_ns(meta: &fs::Metadata) -> i64 {
    meta.modified()
        .ok()
        .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
        .and_then(|d| i64::try_from(d.as_nanos()).ok())
        .unwrap_or(0)
}

/// Relative path with `/`, the same on every system.
fn key(path: &Path) -> String {
    path.components()
        .map(|c| c.as_os_str().to_string_lossy())
        .collect::<Vec<_>>()
        .join("/")
}

/// Every regular file below `dir` (links skipped), relative to it.
fn files(dir: &Path) -> Vec<PathBuf> {
    let mut out = Vec::new();
    let mut stack = vec![PathBuf::new()];
    while let Some(relative) = stack.pop() {
        let Ok(entries) = fs::read_dir(dir.join(&relative)) else {
            continue;
        };
        for entry in entries.flatten() {
            let Ok(kind) = entry.file_type() else {
                continue;
            };
            let path = relative.join(entry.file_name());
            if kind.is_dir() {
                stack.push(path);
            } else if kind.is_file() {
                out.push(path);
            }
        }
    }
    out.sort();
    out
}

fn move_file(from: &Path, to: &Path) -> std::io::Result<()> {
    if let Some(parent) = to.parent() {
        fs::create_dir_all(parent)?;
    }
    if fs::rename(from, to).is_err() {
        fs::copy(from, to)?;
        fs::remove_file(from)?;
    }
    Ok(())
}

/// The game folder mods may go into: an existing folder that is the game's
/// own (not the home folder or above, not a drive's root).
fn game_folder(item: &LibraryItem) -> AppResult<PathBuf> {
    let refuse = || {
        AppError::InvalidInput(tr!(
            "Mods gehen nur bei installierten Spielen mit eigenem Spielordner.",
            "Mods only work for installed games with a folder of their own."
        ))
    };
    let path = item
        .entry
        .install_path
        .as_deref()
        .filter(|_| item.entry.installed)
        .ok_or_else(refuse)?;
    let path = PathBuf::from(path);
    let home = std::env::var_os("HOME")
        .or_else(|| std::env::var_os("USERPROFILE"))
        .map(PathBuf::from);
    if !path.is_absolute()
        || !path.is_dir()
        || !crate::storage::measurable(&path, std::slice::from_ref(&path), home.as_deref())
    {
        return Err(refuse());
    }
    Ok(path)
}

/// A target folder inside the game folder: empty (the game folder) or relative.
fn checked_target(target: &str) -> AppResult<String> {
    let target = target.trim().trim_end_matches(['/', '\\']);
    if target.is_empty() {
        return Ok(String::new());
    }
    safe_relative(&target.replace('\\', "/"))
        .map(|p| key(&p))
        .filter(|t| !t.starts_with(ORIGINALS))
        .ok_or_else(|| {
            AppError::InvalidInput(tr!(
                "Der Zielordner muss ein Ordner im Spielordner sein, etwa „Data“ oder „BepInEx/plugins“.",
                "The target must be a folder inside the game folder, like \"Data\" or \"BepInEx/plugins\"."
            ))
        })
}

/// Copies a folder (links skipped).
fn copy_dir(from: &Path, to: &Path) -> std::io::Result<()> {
    for relative in files(from) {
        let dest = to.join(&relative);
        if let Some(parent) = dest.parent() {
            fs::create_dir_all(parent)?;
        }
        fs::copy(from.join(&relative), dest)?;
    }
    Ok(())
}

/// An archive's single wrapper folder ("MyMod-1.2/Data/…") is left out unless
/// the game has a folder of that name (then it is where the files go).
fn strip_wrapper(staging: &Path, game_target: &Path) -> std::io::Result<()> {
    let entries: Vec<_> = fs::read_dir(staging)?.flatten().collect();
    let [only] = entries.as_slice() else {
        return Ok(());
    };
    if !only.file_type()?.is_dir() || game_target.join(only.file_name()).is_dir() {
        return Ok(());
    }
    let inner = only.path();
    let moved = staging.with_extension("inner");
    fs::rename(&inner, &moved)?;
    fs::remove_dir(staging)?;
    fs::rename(&moved, staging)
}

/// Takes out what Cove put into the game folder and puts the originals back.
fn purge(game: &Path, placed: &[Placed], dirs: &[String]) {
    for file in placed {
        let path = game.join(&file.path);
        let unchanged = fs::symlink_metadata(&path)
            .is_ok_and(|m| m.is_file() && m.len() == file.size && modified_ns(&m) == file.modified);
        // A file the game or an update wrote since is theirs now.
        if unchanged {
            if let Err(error) = fs::remove_file(&path) {
                eprintln!("[mods] {}: {error}", path.display());
            }
        }
    }
    let originals = game.join(ORIGINALS);
    for relative in files(&originals) {
        let dest = game.join(&relative);
        let from = originals.join(&relative);
        let result = if dest.exists() {
            // The game has a newer file there: the old original is outdated.
            fs::remove_file(&from)
        } else {
            move_file(&from, &dest)
        };
        if let Err(error) = result {
            eprintln!("[mods] {}: {error}", from.display());
        }
    }
    let _ = remove_empty(&originals);
    // Folders Cove made, deepest first, if nothing else is in them.
    let mut dirs: Vec<&String> = dirs.iter().collect();
    dirs.sort_by_key(|d| std::cmp::Reverse(d.len()));
    for dir in dirs {
        let _ = fs::remove_dir(game.join(dir));
    }
}

fn remove_empty(dir: &Path) -> std::io::Result<()> {
    if let Ok(entries) = fs::read_dir(dir) {
        for entry in entries.flatten() {
            if entry.file_type()?.is_dir() {
                remove_empty(&entry.path())?;
            }
        }
    }
    fs::remove_dir(dir)
}

/// Copies the switched-on mods (in order) into the game folder.
fn deploy(
    game: &Path,
    mods: &[(String, PathBuf, String)],
) -> std::io::Result<(Vec<Placed>, Vec<String>)> {
    // Later mods win: the last one for a path is copied.
    let mut plan: BTreeMap<String, (String, PathBuf, PathBuf)> = BTreeMap::new();
    for (mod_id, staging, target) in mods {
        for relative in files(staging) {
            let dest = Path::new(target).join(&relative);
            plan.insert(key(&dest), (mod_id.clone(), staging.join(&relative), dest));
        }
    }
    let mut placed = Vec::new();
    let mut made = Vec::new();
    for (path, (mod_id, source, relative)) in plan {
        let dest = game.join(&relative);
        // Folders on the way that aren't there yet are Cove's (and go again).
        let mut missing = Vec::new();
        let mut parent = relative.parent();
        while let Some(dir) = parent.filter(|d| !d.as_os_str().is_empty()) {
            if game.join(dir).exists() {
                break;
            }
            missing.push(key(dir));
            parent = dir.parent();
        }
        match fs::symlink_metadata(&dest) {
            // A folder (or link) where the mod has a file: left alone.
            Ok(meta) if !meta.is_file() => continue,
            Ok(_) => move_file(&dest, &game.join(ORIGINALS).join(&relative))?,
            Err(_) => {}
        }
        if let Some(parent) = dest.parent() {
            fs::create_dir_all(parent)?;
        }
        made.extend(missing);
        fs::copy(&source, &dest)?;
        let meta = fs::metadata(&dest)?;
        placed.push(Placed {
            path,
            mod_id,
            size: meta.len(),
            modified: modified_ns(&meta),
        });
    }
    made.sort();
    made.dedup();
    Ok((placed, made))
}

impl Mods {
    pub fn new(pool: SqlitePool, root: PathBuf) -> Self {
        Self { pool, root }
    }

    fn staging(&self, game_id: &str, mod_id: &str) -> PathBuf {
        self.root.join(game_id).join(mod_id)
    }

    pub async fn list(&self, game_id: &str) -> AppResult<Vec<ModInfo>> {
        let rows = sqlx::query(
            "SELECT id, name, source, target, enabled, position, files, size_bytes, added_at \
             FROM mods WHERE game_id = ? ORDER BY position",
        )
        .bind(game_id)
        .fetch_all(&self.pool)
        .await?;
        let placed: Vec<(String,)> =
            sqlx::query_as("SELECT mod_id FROM mod_files WHERE game_id = ?")
                .bind(game_id)
                .fetch_all(&self.pool)
                .await?;
        let mut mods = Vec::new();
        for row in rows {
            let id: String = row.try_get("id")?;
            let enabled: bool = row.try_get("enabled")?;
            let files: i64 = row.try_get("files")?;
            let active = i64::try_from(placed.iter().filter(|(m,)| *m == id).count()).unwrap_or(0);
            mods.push(ModInfo {
                name: row.try_get("name")?,
                source: row.try_get("source")?,
                target: row.try_get("target")?,
                enabled,
                files,
                size_bytes: row.try_get("size_bytes")?,
                added_at: row.try_get("added_at")?,
                // Switched-on files another mod (later in the order) replaces.
                overridden: if enabled { (files - active).max(0) } else { 0 },
                id,
            });
        }
        Ok(mods)
    }

    /// Unpacks a mod into Cove's folder and switches it on (last in the order).
    pub async fn add(
        &self,
        item: &LibraryItem,
        source: &Path,
        name: &str,
        target: &str,
    ) -> AppResult<Vec<ModInfo>> {
        let game = game_folder(item)?;
        let target = checked_target(target)?;
        let mut id_bytes = [0_u8; 8];
        getrandom::fill(&mut id_bytes).map_err(io_error)?;
        let mod_id = id_bytes.iter().fold(String::new(), |mut id, b| {
            use std::fmt::Write as _;
            let _ = write!(id, "{b:02x}");
            id
        });
        let staging = self.staging(&item.game.id, &mod_id);
        let source_path = source.to_path_buf();
        let file_name = source
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .ok_or_else(|| {
                AppError::InvalidInput(tr!(
                    "Diese Datei gibt es nicht.",
                    "This file doesn't exist."
                ))
            })?;
        let unpack_into = staging.clone();
        let game_target = game.join(&target);
        let name_for_unpack = file_name.clone();
        let (count, size) = tokio::task::spawn_blocking(move || -> AppResult<(usize, u64)> {
            let result = (|| -> AppResult<()> {
                if source_path.is_dir() {
                    copy_dir(&source_path, &unpack_into)?;
                } else if !source_path.is_file() {
                    return Err(AppError::InvalidInput(tr!(
                        "Diese Datei gibt es nicht.",
                        "This file doesn't exist."
                    )));
                } else if archive_kind(&name_for_unpack) == ArchiveKind::Plain {
                    // A single mod file: copied, the user's file stays where it is.
                    fs::create_dir_all(&unpack_into)?;
                    fs::copy(&source_path, unpack_into.join(&name_for_unpack))?;
                } else {
                    unpack(&source_path, &name_for_unpack, &unpack_into)?;
                    strip_wrapper(&unpack_into, &game_target)?;
                }
                Ok(())
            })();
            if let Err(error) = result {
                let _ = fs::remove_dir_all(&unpack_into);
                return Err(error);
            }
            let list = files(&unpack_into);
            let size = list
                .iter()
                .filter_map(|f| fs::metadata(unpack_into.join(f)).ok())
                .map(|m| m.len())
                .sum();
            if list.is_empty() {
                let _ = fs::remove_dir_all(&unpack_into);
                return Err(AppError::InvalidInput(tr!(
                    "In dieser Mod sind keine Dateien.",
                    "This mod has no files."
                )));
            }
            Ok((list.len(), size))
        })
        .await
        .map_err(io_error)??;

        let name = name.trim();
        let name = if name.is_empty() {
            file_name.split('.').next().unwrap_or(&file_name).to_owned()
        } else {
            name.chars().take(120).collect()
        };
        sqlx::query(
            "INSERT INTO mods (id, game_id, name, source, target, enabled, position, files, size_bytes, added_at) \
             VALUES (?, ?, ?, ?, ?, 1, (SELECT COALESCE(MAX(position), -1) + 1 FROM mods WHERE game_id = ?), ?, ?, ?)",
        )
        .bind(&mod_id)
        .bind(&item.game.id)
        .bind(&name)
        .bind(&file_name)
        .bind(&target)
        .bind(&item.game.id)
        .bind(i64::try_from(count).unwrap_or(i64::MAX))
        .bind(i64::try_from(size).unwrap_or(i64::MAX))
        .bind(now_iso())
        .execute(&self.pool)
        .await?;
        if let Err(error) = self.apply(item).await {
            // Not deployable: the mod is not kept.
            self.forget(&item.game.id, &mod_id).await?;
            let _ = self.apply(item).await;
            return Err(error);
        }
        self.list(&item.game.id).await
    }

    pub async fn set_enabled(
        &self,
        item: &LibraryItem,
        mod_id: &str,
        enabled: bool,
    ) -> AppResult<Vec<ModInfo>> {
        self.require(&item.game.id, mod_id).await?;
        sqlx::query("UPDATE mods SET enabled = ? WHERE id = ? AND game_id = ?")
            .bind(enabled)
            .bind(mod_id)
            .bind(&item.game.id)
            .execute(&self.pool)
            .await?;
        self.apply(item).await?;
        self.list(&item.game.id).await
    }

    /// Moves a mod to `position` (0 = first; later mods win).
    pub async fn move_to(
        &self,
        item: &LibraryItem,
        mod_id: &str,
        position: usize,
    ) -> AppResult<Vec<ModInfo>> {
        self.require(&item.game.id, mod_id).await?;
        let mut order: Vec<String> = self
            .list(&item.game.id)
            .await?
            .into_iter()
            .map(|m| m.id)
            .collect();
        order.retain(|id| id != mod_id);
        order.insert(position.min(order.len()), mod_id.to_owned());
        let mut tx = self.pool.begin().await?;
        for (index, id) in order.iter().enumerate() {
            sqlx::query("UPDATE mods SET position = ? WHERE id = ?")
                .bind(i64::try_from(index).unwrap_or(i64::MAX))
                .bind(id)
                .execute(&mut *tx)
                .await?;
        }
        tx.commit().await?;
        self.apply(item).await?;
        self.list(&item.game.id).await
    }

    pub async fn remove(&self, item: &LibraryItem, mod_id: &str) -> AppResult<Vec<ModInfo>> {
        self.require(&item.game.id, mod_id).await?;
        self.forget(&item.game.id, mod_id).await?;
        self.apply(item).await?;
        let staging = self.staging(&item.game.id, mod_id);
        tokio::task::spawn_blocking(move || fs::remove_dir_all(staging))
            .await
            .map_err(io_error)??;
        self.list(&item.game.id).await
    }

    async fn require(&self, game_id: &str, mod_id: &str) -> AppResult<()> {
        let found: Option<i64> =
            sqlx::query_scalar("SELECT 1 FROM mods WHERE id = ? AND game_id = ?")
                .bind(mod_id)
                .bind(game_id)
                .fetch_optional(&self.pool)
                .await?;
        found.map(|_| ()).ok_or_else(|| {
            AppError::InvalidInput(tr!(
                "Diese Mod gibt es nicht mehr.",
                "This mod doesn't exist any more."
            ))
        })
    }

    async fn forget(&self, game_id: &str, mod_id: &str) -> AppResult<()> {
        sqlx::query("DELETE FROM mods WHERE id = ? AND game_id = ?")
            .bind(mod_id)
            .bind(game_id)
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    /// Takes out what Cove put into the game folder, then copies the switched-on mods in.
    async fn apply(&self, item: &LibraryItem) -> AppResult<()> {
        let game = game_folder(item)?;
        let game_id = item.game.id.clone();
        let placed: Vec<Placed> =
            sqlx::query("SELECT path, mod_id, size, modified FROM mod_files WHERE game_id = ?")
                .bind(&game_id)
                .fetch_all(&self.pool)
                .await?
                .into_iter()
                .map(|row| -> AppResult<Placed> {
                    Ok(Placed {
                        path: row.try_get("path")?,
                        mod_id: row.try_get("mod_id")?,
                        size: u64::try_from(row.try_get::<i64, _>("size")?).unwrap_or(0),
                        modified: row.try_get("modified")?,
                    })
                })
                .collect::<AppResult<_>>()?;
        let dirs: Vec<String> = sqlx::query_scalar("SELECT path FROM mod_dirs WHERE game_id = ?")
            .bind(&game_id)
            .fetch_all(&self.pool)
            .await?;
        let mods: Vec<(String, PathBuf, String)> = sqlx::query_as::<_, (String, String)>(
            "SELECT id, target FROM mods WHERE game_id = ? AND enabled = 1 ORDER BY position",
        )
        .bind(&game_id)
        .fetch_all(&self.pool)
        .await?
        .into_iter()
        .map(|(id, target)| {
            let staging = self.staging(&game_id, &id);
            (id, staging, target)
        })
        .collect();

        let (placed, made) = tokio::task::spawn_blocking(move || {
            purge(&game, &placed, &dirs);
            deploy(&game, &mods)
        })
        .await
        .map_err(io_error)??;

        let mut tx = self.pool.begin().await?;
        sqlx::query("DELETE FROM mod_files WHERE game_id = ?")
            .bind(&item.game.id)
            .execute(&mut *tx)
            .await?;
        sqlx::query("DELETE FROM mod_dirs WHERE game_id = ?")
            .bind(&item.game.id)
            .execute(&mut *tx)
            .await?;
        for file in &placed {
            sqlx::query("INSERT INTO mod_files (game_id, path, mod_id, size, modified) VALUES (?, ?, ?, ?, ?)")
                .bind(&item.game.id)
                .bind(&file.path)
                .bind(&file.mod_id)
                .bind(i64::try_from(file.size).unwrap_or(i64::MAX))
                .bind(file.modified)
                .execute(&mut *tx)
                .await?;
        }
        for dir in &made {
            sqlx::query("INSERT INTO mod_dirs (game_id, path) VALUES (?, ?)")
                .bind(&item.game.id)
                .bind(dir)
                .execute(&mut *tx)
                .await?;
        }
        tx.commit().await?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::ManualGameInput;
    use std::io::Write;

    struct Setup {
        _tmp: tempfile::TempDir,
        game: PathBuf,
        downloads: PathBuf,
        item: LibraryItem,
        mods: Mods,
    }

    async fn setup() -> Setup {
        let tmp = tempfile::tempdir().unwrap();
        let game = tmp.path().join("Games/Ember");
        fs::create_dir_all(game.join("Data")).unwrap();
        fs::write(game.join("ember.sh"), "#!/bin/sh\n").unwrap();
        fs::write(game.join("Data/textures.pak"), "original").unwrap();
        let downloads = tmp.path().join("Downloads");
        fs::create_dir_all(&downloads).unwrap();
        let pool = crate::db::open_in_memory().await.unwrap();
        let item = crate::db::library::add_manual(
            &pool,
            &ManualGameInput {
                title: "Ember".into(),
                executable_path: game.join("ember.sh").to_string_lossy().into_owned(),
                size_bytes: None,
            },
        )
        .await
        .unwrap();
        let mods = Mods::new(pool, tmp.path().join("data/mods"));
        Setup {
            game,
            downloads,
            item,
            mods,
            _tmp: tmp,
        }
    }

    fn zip(path: &Path, entries: &[(&str, &str)]) {
        let mut writer = zip::ZipWriter::new(fs::File::create(path).unwrap());
        for (name, text) in entries {
            writer
                .start_file(*name, zip::write::SimpleFileOptions::default())
                .unwrap();
            writer.write_all(text.as_bytes()).unwrap();
        }
        writer.finish().unwrap();
    }

    fn read(path: &Path) -> String {
        fs::read_to_string(path).unwrap()
    }

    #[tokio::test]
    async fn mods_go_in_in_order_and_everything_comes_back_out() {
        let s = setup().await;
        // "HD Pack-1.2/Data/…": the wrapper folder is left out.
        let hd = s.downloads.join("hd.zip");
        zip(
            &hd,
            &[
                ("HD Pack-1.2/Data/textures.pak", "hd"),
                ("HD Pack-1.2/Data/new/extra.pak", "extra"),
            ],
        );
        let list = s.mods.add(&s.item, &hd, "HD Pack", "").await.unwrap();
        assert_eq!(list[0].files, 2);
        assert_eq!(read(&s.game.join("Data/textures.pak")), "hd");
        assert_eq!(
            read(&s.game.join(ORIGINALS).join("Data/textures.pak")),
            "original"
        );
        assert!(hd.is_file(), "the user's archive stays");

        // A single file into a target folder; a second mod for the same file wins.
        let fix = s.downloads.join("textures.pak");
        fs::write(&fix, "fix").unwrap();
        let list = s.mods.add(&s.item, &fix, "", "Data").await.unwrap();
        assert_eq!(list[1].name, "textures");
        assert_eq!(read(&s.game.join("Data/textures.pak")), "fix");
        assert_eq!(list[0].overridden, 1);

        // Order changed: the HD pack wins again.
        let fix_id = list[1].id.clone();
        let hd_id = list[0].id.clone();
        s.mods.move_to(&s.item, &fix_id, 0).await.unwrap();
        assert_eq!(read(&s.game.join("Data/textures.pak")), "hd");

        // Switched off: the fix is still there; then the HD pack goes too.
        s.mods.set_enabled(&s.item, &hd_id, false).await.unwrap();
        assert_eq!(read(&s.game.join("Data/textures.pak")), "fix");
        assert!(
            !s.game.join("Data/new").exists(),
            "Cove's folders go with their files"
        );
        let list = s.mods.remove(&s.item, &fix_id).await.unwrap();
        assert_eq!(list.len(), 1);
        assert_eq!(read(&s.game.join("Data/textures.pak")), "original");
        assert!(!s.game.join(ORIGINALS).exists());
    }

    #[tokio::test]
    async fn files_the_game_changed_stay_and_bad_targets_are_refused() {
        let s = setup().await;
        let save = s.downloads.join("config.ini");
        fs::write(&save, "mod").unwrap();
        let list = s.mods.add(&s.item, &save, "Config", "").await.unwrap();
        // The game rewrites the file while the mod is on.
        std::thread::sleep(std::time::Duration::from_millis(20));
        fs::write(s.game.join("config.ini"), "written by the game").unwrap();
        s.mods
            .set_enabled(&s.item, &list[0].id, false)
            .await
            .unwrap();
        assert_eq!(read(&s.game.join("config.ini")), "written by the game");

        for bad in ["../outside", "/etc", ".cove-originals"] {
            assert!(matches!(
                s.mods.add(&s.item, &save, "x", bad).await,
                Err(AppError::InvalidInput(_))
            ));
        }
        let empty = s.downloads.join("empty.zip");
        zip(&empty, &[]);
        assert!(s.mods.add(&s.item, &empty, "x", "").await.is_err());
        assert_eq!(s.mods.list(&s.item.game.id).await.unwrap().len(), 1);
    }

    #[test]
    fn a_wrapper_folder_the_game_has_is_kept() {
        let tmp = tempfile::tempdir().unwrap();
        let staging = tmp.path().join("staging");
        fs::create_dir_all(staging.join("Data")).unwrap();
        fs::write(staging.join("Data/a.pak"), "a").unwrap();
        let game = tmp.path().join("game");
        fs::create_dir_all(game.join("Data")).unwrap();
        strip_wrapper(&staging, &game).unwrap();
        assert!(staging.join("Data/a.pak").is_file());
        strip_wrapper(&staging, &tmp.path().join("other")).unwrap();
        assert!(staging.join("a.pak").is_file());
    }
}
