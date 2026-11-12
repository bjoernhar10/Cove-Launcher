//! Save backups of every game. Where a game keeps its saves comes from the
//! Ludusavi manifest (`manifest.rs`), filled in for this computer
//! (`paths.rs`): this user, the game's Proton prefix, or Steam's prefix of a
//! Steam game. After every session (and on request) the files are copied to
//! `<backup folder>/<game>/<time>/files/…` with a `backup.json` listing where
//! each file came from; a backup with nothing changed since the last one is
//! skipped, and only the newest `keep` stay. Restoring copies the files back
//! (the current state is backed up first) and never deletes anything.

pub mod manifest;
pub mod paths;

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use sqlx::SqlitePool;

use self::manifest::ManifestStore;
use self::paths::{Context, Os, WindowsDirs, MAX_BYTES, MAX_FILES};
use crate::db;
use crate::error::{AppError, AppResult};
use crate::launcher::HostEnv;
use crate::models::{
    AppSettings, BackupInfo, CompatibilityTool, GameBackups, LibraryItem, Platform, StoreSource,
};

const RECORD: &str = "backup.json";
const FILES: &str = "files";

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct Record {
    game_id: String,
    title: String,
    created_at: String,
    /// Hash of every file's path, size and time: equal = nothing changed.
    fingerprint: String,
    files: Vec<RecordFile>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct RecordFile {
    /// Where the file lives on this computer.
    original: String,
    /// Below the backup folder, with `/`.
    stored: String,
    size: u64,
}

/// `/home/u/x` → `home/u/x`, `C:\Users\x` → `C/Users/x`: a relative path to store a file under.
fn stored_name(original: &Path) -> String {
    original
        .components()
        .filter_map(|c| match c {
            std::path::Component::Normal(part) => Some(part.to_string_lossy().into_owned()),
            std::path::Component::Prefix(prefix) => Some(
                prefix
                    .as_os_str()
                    .to_string_lossy()
                    .chars()
                    .filter(char::is_ascii_alphanumeric)
                    .collect(),
            ),
            _ => None,
        })
        .collect::<Vec<_>>()
        .join("/")
}

fn fingerprint(files: &[(PathBuf, u64, i64)]) -> String {
    let mut hash = Sha256::new();
    for (path, size, time) in files {
        hash.update(path.to_string_lossy().as_bytes());
        hash.update(size.to_le_bytes());
        hash.update(time.to_le_bytes());
    }
    hash.finalize().iter().fold(String::new(), |mut hex, b| {
        use std::fmt::Write as _;
        let _ = write!(hex, "{b:02x}");
        hex
    })
}

/// Path, size and modification time (seconds) of each file.
fn describe(files: &BTreeSet<PathBuf>) -> AppResult<Vec<(PathBuf, u64, i64)>> {
    let mut out = Vec::with_capacity(files.len());
    let mut total = 0u64;
    for path in files {
        let Ok(meta) = std::fs::metadata(path) else {
            continue;
        };
        total += meta.len();
        let time = meta
            .modified()
            .ok()
            .map_or(0, |t| DateTime::<Utc>::from(t).timestamp());
        out.push((path.clone(), meta.len(), time));
    }
    if out.len() > MAX_FILES || total > MAX_BYTES {
        return Err(AppError::InvalidInput(tr!(
            "Die Spielstände dieses Spiels sind zu groß für ein Backup (über 4 GB oder 20.000 Dateien).",
            "This game's saves are too big for a backup (over 4 GB or 20,000 files)."
        )));
    }
    Ok(out)
}

fn read_record(dir: &Path) -> Option<Record> {
    serde_json::from_slice(&std::fs::read(dir.join(RECORD)).ok()?).ok()
}

/// Backups of a game, newest first.
fn list(game_dir: &Path) -> Vec<(PathBuf, Record)> {
    let Ok(entries) = std::fs::read_dir(game_dir) else {
        return Vec::new();
    };
    let mut out: Vec<(PathBuf, Record)> = entries
        .flatten()
        .filter_map(|e| read_record(&e.path()).map(|r| (e.path(), r)))
        .collect();
    out.sort_by(|a, b| b.1.created_at.cmp(&a.1.created_at));
    out
}

/// Copies the files into a new backup unless nothing changed since the last
/// one; keeps the newest `keep`. Blocking. Returns the new backup, if any.
fn write_backup(
    game_dir: &Path,
    item: &LibraryItem,
    files: &[(PathBuf, u64, i64)],
    keep: usize,
    protect: Option<&Path>,
) -> AppResult<Option<BackupInfo>> {
    if files.is_empty() {
        return Ok(None);
    }
    let print = fingerprint(files);
    if list(game_dir)
        .first()
        .is_some_and(|(_, r)| r.fingerprint == print)
    {
        return Ok(None);
    }
    let now = Utc::now();
    let id = now.format("%Y-%m-%d_%H-%M-%S%.3f").to_string();
    let target = game_dir.join(&id);
    let part = game_dir.join(format!(".{id}.part"));
    std::fs::create_dir_all(&part)?;
    let mut record_files = Vec::with_capacity(files.len());
    for (original, size, _) in files {
        let stored = format!("{FILES}/{}", stored_name(original));
        let to = part.join(&stored);
        if let Some(parent) = to.parent() {
            std::fs::create_dir_all(parent)?;
        }
        std::fs::copy(original, &to)?;
        record_files.push(RecordFile {
            original: original.to_string_lossy().into_owned(),
            stored,
            size: *size,
        });
    }
    let record = Record {
        game_id: item.game.id.clone(),
        title: item.game.title.clone(),
        created_at: now.to_rfc3339_opts(chrono::SecondsFormat::Millis, true),
        fingerprint: print,
        files: record_files,
    };
    std::fs::write(part.join(RECORD), serde_json::to_vec_pretty(&record)?)?;
    std::fs::rename(&part, &target)?;
    for (old, _) in list(game_dir).into_iter().skip(keep) {
        if Some(old.as_path()) != protect {
            let _ = std::fs::remove_dir_all(old);
        }
    }
    Ok(Some(info(&id, &record)))
}

fn info(id: &str, record: &Record) -> BackupInfo {
    BackupInfo {
        id: id.to_owned(),
        created_at: record.created_at.clone(),
        files: i64::try_from(record.files.len()).unwrap_or(i64::MAX),
        bytes: i64::try_from(record.files.iter().map(|f| f.size).sum::<u64>()).unwrap_or(i64::MAX),
    }
}

/// Puts a backup's files back where they came from. Blocking.
fn restore_files(backup: &Path) -> AppResult<usize> {
    let record = read_record(backup).ok_or_else(|| {
        AppError::InvalidInput(tr!(
            "Dieses Backup ist unvollständig.",
            "This backup is incomplete."
        ))
    })?;
    let mut count = 0;
    for file in &record.files {
        // Only files the backup really holds, from below its own folder.
        if file.stored.split('/').any(|p| p == ".." || p.is_empty()) {
            continue;
        }
        let from = backup.join(&file.stored);
        if !from.is_file() {
            continue;
        }
        let to = PathBuf::from(&file.original);
        if let Some(parent) = to.parent() {
            std::fs::create_dir_all(parent)?;
        }
        std::fs::copy(&from, &to)?;
        count += 1;
    }
    Ok(count)
}

pub struct BackupService {
    pool: SqlitePool,
    defaults: AppSettings,
    data_dir: PathBuf,
    manifest: ManifestStore,
}

impl BackupService {
    pub fn new(
        pool: SqlitePool,
        defaults: AppSettings,
        data_dir: PathBuf,
        manifest_url: String,
    ) -> Self {
        let client = reqwest::Client::builder()
            .user_agent(concat!(
                "Cove/",
                env!("CARGO_PKG_VERSION"),
                " (desktop game launcher)"
            ))
            .connect_timeout(std::time::Duration::from_secs(10))
            .build()
            .unwrap_or_default();
        Self {
            manifest: ManifestStore::new(data_dir.join("ludusavi"), manifest_url, client),
            pool,
            defaults,
            data_dir,
        }
    }

    async fn settings(&self) -> AppResult<AppSettings> {
        db::settings::load(&self.pool, &self.defaults).await
    }

    /// The backup folder (the setting, else `<app data>/backups`).
    pub fn root(&self, settings: &AppSettings) -> PathBuf {
        let dir = settings.backups.directory.trim();
        if dir.is_empty() {
            self.data_dir.join("backups")
        } else {
            PathBuf::from(dir)
        }
    }

    fn game_dir(root: &Path, item: &LibraryItem) -> PathBuf {
        let slug = db::slugify(&item.game.title);
        let safe: String = item
            .game
            .id
            .chars()
            .map(|c| {
                if c.is_ascii_alphanumeric() || c == '-' {
                    c
                } else {
                    '_'
                }
            })
            .collect();
        root.join(if slug.is_empty() {
            safe
        } else {
            format!("{slug}_{safe}")
        })
    }

    /// Where this game's saves may be on this computer.
    fn contexts(item: &LibraryItem, host: &HostEnv) -> Vec<Context> {
        let entry = &item.entry;
        let store = match entry.source {
            StoreSource::Steam => Some("steam"),
            StoreSource::Gog => Some("gog"),
            StoreSource::Epic => Some("epic"),
            StoreSource::Manual => None,
        };
        let base = entry.install_path.as_deref().map(PathBuf::from);
        let home = host.home.clone().unwrap_or_default();
        let user = std::env::var("USER")
            .or_else(|_| std::env::var("USERNAME"))
            .unwrap_or_else(|_| "user".into());
        let context = |os, home: PathBuf, os_user: String, windows| Context {
            os,
            store,
            store_game_id: entry.store_game_id.clone(),
            base: base.clone(),
            home,
            os_user,
            windows,
            xdg_data: None,
            xdg_config: None,
        };
        let mut out = Vec::new();
        match host.platform {
            Platform::Windows => {
                let drive_c = PathBuf::from(
                    std::env::var("SystemDrive").unwrap_or_else(|_| "C:".into()) + "\\",
                );
                out.push(context(
                    Os::Windows,
                    home.clone(),
                    user,
                    Some(WindowsDirs::of_profile(&home, &drive_c)),
                ));
            }
            Platform::Macos => out.push(context(Os::Mac, home, user, None)),
            Platform::Linux => {
                let windows_exe = entry
                    .executable_path
                    .as_deref()
                    .is_some_and(|e| e.to_lowercase().ends_with(".exe"));
                // Cove's Proton prefix (or ~/.wine for Wine).
                if windows_exe {
                    let drive_c = match entry.launch_options.compatibility_tool {
                        CompatibilityTool::Wine => Some(home.join(".wine/drive_c")),
                        CompatibilityTool::Proton => Some(
                            host.compat_data_root
                                .join(&item.game.id)
                                .join("pfx/drive_c"),
                        ),
                        CompatibilityTool::Native => None,
                    };
                    if let Some(drive_c) = drive_c {
                        let user =
                            if entry.launch_options.compatibility_tool == CompatibilityTool::Wine {
                                user.clone()
                            } else {
                                "steamuser".into()
                            };
                        let (profile, dirs) = WindowsDirs::in_prefix(&drive_c, &user);
                        out.push(context(Os::Windows, profile, user, Some(dirs)));
                    }
                }
                // Steam's prefix of a Steam game that runs with Proton.
                if let (StoreSource::Steam, Some(app), Some(steam)) = (
                    entry.source,
                    entry.store_game_id.as_deref(),
                    host.steam_root(),
                ) {
                    let drive_c = steam
                        .join("steamapps/compatdata")
                        .join(app)
                        .join("pfx/drive_c");
                    if drive_c.is_dir() {
                        let (profile, dirs) = WindowsDirs::in_prefix(&drive_c, "steamuser");
                        out.push(context(
                            Os::Windows,
                            profile,
                            "steamuser".into(),
                            Some(dirs),
                        ));
                    }
                }
                let xdg = |var: &str, fallback: &str| {
                    std::env::var_os(var)
                        .map(PathBuf::from)
                        .filter(|p| p.is_absolute())
                        .unwrap_or_else(|| home.join(fallback))
                };
                let mut native = context(Os::Linux, home.clone(), user.clone(), None);
                native.xdg_data = Some(xdg("XDG_DATA_HOME", ".local/share"));
                native.xdg_config = Some(xdg("XDG_CONFIG_HOME", ".config"));
                out.push(native);
            }
        }
        out
    }

    /// The game's entry in the manifest and its files right now.
    async fn current_files(
        &self,
        item: &LibraryItem,
        host: &HostEnv,
    ) -> AppResult<Option<Vec<(PathBuf, u64, i64)>>> {
        let index = self.manifest.index().await?;
        let id = |source| {
            (item.entry.source == source)
                .then(|| item.entry.store_game_id.as_deref()?.parse::<u64>().ok())
                .flatten()
        };
        let Some(game) = index
            .find(
                id(StoreSource::Steam),
                id(StoreSource::Gog),
                &item.game.title,
            )
            .cloned()
        else {
            return Ok(None);
        };
        drop(index);
        let contexts = Self::contexts(item, host);
        let files = tokio::task::spawn_blocking(move || {
            let found = paths::find_files(&game, &contexts);
            describe(&found)
        })
        .await
        .map_err(|e| AppError::Download(e.to_string()))??;
        Ok(Some(files))
    }

    /// What the game page shows: known to the manifest, files found now, backups.
    pub async fn overview(&self, game_id: &str, host: &HostEnv) -> AppResult<GameBackups> {
        let item = db::library::require(&self.pool, game_id).await?;
        let settings = self.settings().await?;
        let game_dir = Self::game_dir(&self.root(&settings), &item);
        let current = self.current_files(&item, host).await?;
        let backups = tokio::task::spawn_blocking(move || {
            list(&game_dir)
                .iter()
                .filter_map(|(path, record)| {
                    Some(info(&path.file_name()?.to_string_lossy(), record))
                })
                .collect()
        })
        .await
        .map_err(|e| AppError::Download(e.to_string()))?;
        let (files, bytes) = current.as_ref().map_or((0, 0), |f| {
            (f.len(), f.iter().map(|(_, size, _)| size).sum::<u64>())
        });
        Ok(GameBackups {
            known: current.is_some(),
            files: i64::try_from(files).unwrap_or(i64::MAX),
            bytes: i64::try_from(bytes).unwrap_or(i64::MAX),
            backups,
        })
    }

    /// Backs the game's saves up now. None = nothing to back up or nothing changed.
    pub async fn back_up(&self, game_id: &str, host: &HostEnv) -> AppResult<Option<BackupInfo>> {
        self.back_up_keeping(game_id, host, None).await
    }

    async fn back_up_keeping(
        &self,
        game_id: &str,
        host: &HostEnv,
        protect: Option<PathBuf>,
    ) -> AppResult<Option<BackupInfo>> {
        let item = db::library::require(&self.pool, game_id).await?;
        let settings = self.settings().await?;
        let Some(files) = self.current_files(&item, host).await? else {
            return Err(AppError::InvalidInput(tr!(
                "Für dieses Spiel kennt Cove keine Spielstand-Orte.",
                "Cove knows no save locations for this game."
            )));
        };
        let game_dir = Self::game_dir(&self.root(&settings), &item);
        let keep = usize::try_from(settings.backups.keep).unwrap_or(10).max(1);
        tokio::task::spawn_blocking(move || {
            write_backup(&game_dir, &item, &files, keep, protect.as_deref())
        })
        .await
        .map_err(|e| AppError::Download(e.to_string()))?
    }

    /// After a session: a backup when switched on (quietly; errors only logged).
    pub async fn after_session(&self, game_id: &str, host: &HostEnv) {
        match self.settings().await {
            Ok(settings) if settings.backups.enabled => {}
            _ => return,
        }
        match self.back_up(game_id, host).await {
            Ok(Some(backup)) => eprintln!(
                "[cove] Spielstand-Backup von {game_id}: {} Datei(en)",
                backup.files
            ),
            Ok(None) | Err(AppError::InvalidInput(_)) => {}
            Err(error) => eprintln!("[cove] Spielstand-Backup von {game_id}: {error}"),
        }
    }

    /// Puts a backup back (the current saves are backed up first).
    pub async fn restore(
        &self,
        game_id: &str,
        backup_id: &str,
        host: &HostEnv,
    ) -> AppResult<usize> {
        let item = db::library::require(&self.pool, game_id).await?;
        let settings = self.settings().await?;
        let plain = !backup_id.is_empty()
            && backup_id
                .chars()
                .all(|c| c.is_ascii_digit() || matches!(c, '-' | '_' | '.'));
        let backup = Self::game_dir(&self.root(&settings), &item).join(backup_id);
        if !plain || !backup.join(RECORD).is_file() {
            return Err(AppError::InvalidInput(tr!(
                "Dieses Backup gibt es nicht.",
                "This backup does not exist."
            )));
        }
        // The state before is kept as a backup of its own.
        if let Err(error) = self
            .back_up_keeping(game_id, host, Some(backup.clone()))
            .await
        {
            eprintln!("[cove] Backup vor dem Wiederherstellen: {error}");
        }
        tokio::task::spawn_blocking(move || restore_files(&backup))
            .await
            .map_err(|e| AppError::Download(e.to_string()))?
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::{LaunchOptions, LibraryUpdate, ManualGameInput};
    use wiremock::matchers::method;
    use wiremock::{Mock, MockServer, ResponseTemplate};

    const YAML: &str = r"
Saltmarsh:
  files:
    <winDocuments>/Saltmarsh:
      tags: [save]
";

    #[test]
    fn stored_names_are_relative() {
        assert_eq!(
            stored_name(Path::new("/home/u/.local/x.sav")),
            "home/u/.local/x.sav"
        );
    }

    #[tokio::test]
    async fn back_up_skip_unchanged_keep_the_newest_and_restore() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .respond_with(ResponseTemplate::new(200).set_body_string(YAML))
            .mount(&server)
            .await;
        let pool = db::open_in_memory().await.unwrap();
        let tmp = tempfile::tempdir().unwrap();
        let exe = tmp.path().join("games/Saltmarsh/game.exe");
        std::fs::create_dir_all(exe.parent().unwrap()).unwrap();
        std::fs::write(&exe, b"MZ").unwrap();
        let item = db::library::add_manual(
            &pool,
            &ManualGameInput {
                title: "Saltmarsh".into(),
                executable_path: exe.to_string_lossy().into_owned(),
                size_bytes: None,
            },
        )
        .await
        .unwrap();
        db::library::update(
            &pool,
            &item.game.id,
            &LibraryUpdate {
                launch_options: Some(LaunchOptions {
                    compatibility_tool: CompatibilityTool::Proton,
                    ..LaunchOptions::default()
                }),
                ..LibraryUpdate::default()
            },
        )
        .await
        .unwrap();
        let mut defaults = crate::state::default_settings(None);
        defaults.backups.keep = 2;
        let service = BackupService::new(
            pool,
            defaults,
            tmp.path().join("data"),
            format!("{}/m.yaml", server.uri()),
        );
        let mut host = HostEnv::detect(tmp.path().join("data/compat"));
        host.platform = Platform::Linux;
        host.home = Some(tmp.path().join("home"));

        let saves = tmp
            .path()
            .join("data/compat")
            .join(&item.game.id)
            .join("pfx/drive_c/users/steamuser/Documents/Saltmarsh");
        // Nothing there yet: known game, no files, no backup.
        let empty = service.overview(&item.game.id, &host).await.unwrap();
        assert!(empty.known);
        assert_eq!(empty.files, 0);
        assert_eq!(service.back_up(&item.game.id, &host).await.unwrap(), None);

        std::fs::create_dir_all(&saves).unwrap();
        std::fs::write(saves.join("slot1.sav"), b"day one").unwrap();
        let first = service
            .back_up(&item.game.id, &host)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(first.files, 1);
        // Unchanged: no second backup.
        assert_eq!(service.back_up(&item.game.id, &host).await.unwrap(), None);

        std::fs::write(saves.join("slot1.sav"), b"day two, longer").unwrap();
        service
            .back_up(&item.game.id, &host)
            .await
            .unwrap()
            .unwrap();
        std::fs::write(saves.join("slot2.sav"), b"new slot").unwrap();
        service
            .back_up(&item.game.id, &host)
            .await
            .unwrap()
            .unwrap();
        let overview = service.overview(&item.game.id, &host).await.unwrap();
        assert_eq!(overview.files, 2);
        assert_eq!(overview.backups.len(), 2, "only the newest two are kept");
        assert!(overview.backups.iter().all(|b| b.id != first.id));

        // Restore the older of the two: slot1 goes back to "day two".
        std::fs::write(saves.join("slot1.sav"), b"broken").unwrap();
        let older = overview.backups[1].id.clone();
        assert_eq!(
            service.restore(&item.game.id, &older, &host).await.unwrap(),
            1
        );
        assert_eq!(
            std::fs::read(saves.join("slot1.sav")).unwrap(),
            b"day two, longer"
        );
        assert!(saves.join("slot2.sav").is_file(), "nothing is deleted");
        assert!(service.restore(&item.game.id, "../x", &host).await.is_err());
    }
}
