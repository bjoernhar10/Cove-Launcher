//! The download manager's Epic part: installing and updating, starting with
//! an exchange code, and syncing cloud saves around a session.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use base64::Engine as _;
use tokio::sync::watch;

use super::epic::{self, saves, EpicMeta};
use super::transfer::{Control, Outcome, Progress};
use super::{extract, gog, DownloadManager};
use crate::db;
use crate::error::{AppError, AppResult};
use crate::launcher::HostEnv;
use crate::models::{CompatibilityTool, LibraryItem, StoreSource};

impl DownloadManager {
    /// Downloads an Epic build and the owned DLCs with files into the game folder.
    pub(super) async fn run_epic(
        &self,
        record: &db::downloads::Record,
        control: watch::Receiver<Control>,
    ) -> AppResult<Outcome> {
        let (_, _, app) = self.store_game(&record.game_id).await?;
        let info = self
            .stores()?
            .epic_install_info(&app, gog::host_platform())
            .await?;
        let (manifest, bases) = epic::fetch_manifest(&self.client, &info.manifest_urls).await?;
        if let Some(reason) = epic::launcher_only(&info, &manifest) {
            return Err(AppError::Download(reason));
        }
        let excluded = db::store_installs::excluded_dlcs(&self.pool, &record.game_id).await?;
        let mut dlcs = Vec::with_capacity(info.dlcs.len());
        for dlc in info.dlcs.iter().filter(|d| !excluded.contains(&d.app_name)) {
            let (manifest, bases) = epic::fetch_manifest(&self.client, &dlc.manifest_urls).await?;
            dlcs.push((dlc, manifest, bases));
        }
        let dir = Self::game_dir(record)?;
        let work = Self::work_dir(record)
            .ok_or_else(|| {
                AppError::Download(tr!("Kein Zielordner festgelegt.", "No target folder set."))
            })?
            .join(&record.id);
        tokio::fs::create_dir_all(&work).await?;
        tokio::fs::create_dir_all(&dir).await?;
        let mut old_files = db::store_installs::get(&self.pool, &record.game_id)
            .await?
            .map(|p| p.files)
            .unwrap_or_default();
        for dlc in db::store_installs::dlcs(&self.pool, &record.game_id).await? {
            old_files.extend(dlc.files);
        }
        let progress = Arc::new(Progress::default());
        // The game first, then its DLCs (a DLC may replace a file of the game).
        let jobs: Vec<epic::Job> = std::iter::once((&manifest, &bases))
            .chain(dlcs.iter().map(|(_, m, b)| (m, b)))
            .map(|(manifest, base_urls)| epic::Job {
                client: &self.client,
                limiter: &self.limiter,
                progress: &progress,
                control: control.clone(),
                manifest,
                base_urls,
                dir: &dir,
                work: &work,
            })
            .collect();
        let mut todo = Vec::with_capacity(jobs.len());
        for job in &jobs {
            todo.push(job.files_to_write().await?);
        }
        let total = jobs
            .iter()
            .zip(&todo)
            .map(|(job, files)| job.total(files))
            .sum::<u64>();
        let total = i64::try_from(total).unwrap_or(i64::MAX);
        db::downloads::set_remote(
            &self.pool,
            &record.id,
            total,
            Some(&info.build_version),
            &record.title,
        )
        .await?;
        let outcome = {
            let reporter = self.report_progress(&record.id, total, progress.clone());
            let all = async {
                for (job, files) in jobs.iter().zip(&todo) {
                    let outcome = job.run(files).await?;
                    if outcome != Outcome::Completed {
                        return Ok(outcome);
                    }
                }
                Ok(Outcome::Completed)
            };
            let outcome: AppResult<Outcome> = tokio::select! {
                outcome = all => outcome,
                () = reporter => unreachable!("the reporter runs until dropped"),
            };
            self.save_progress(&record.id, &progress).await?;
            outcome?
        };
        match outcome {
            Outcome::Completed => {}
            Outcome::Cancelled => {
                self.remove_unfinished_game(record).await;
                return Ok(outcome);
            }
            Outcome::Paused => return Ok(outcome),
        }
        self.finish_epic(record, &info, &manifest, &dlcs, &old_files, &dir, &work)
            .await?;
        Ok(Outcome::Completed)
    }

    /// After a completed download: drops files no build has anymore, tidies
    /// up and records the game and its DLCs.
    #[allow(clippy::too_many_arguments)]
    async fn finish_epic(
        &self,
        record: &db::downloads::Record,
        info: &epic::InstallInfo,
        manifest: &epic::format::Manifest,
        dlcs: &[(&epic::DlcInfo, epic::format::Manifest, Vec<String>)],
        old_files: &[String],
        dir: &Path,
        work: &Path,
    ) -> AppResult<()> {
        let installed: Vec<db::store_installs::InstalledDlc> = dlcs
            .iter()
            .map(|(dlc, manifest, _)| db::store_installs::InstalledDlc {
                app_name: dlc.app_name.clone(),
                title: dlc.title.clone(),
                build_id: dlc.build_version.clone(),
                files: epic::installed_files(manifest),
            })
            .collect();
        let game_files = epic::installed_files(manifest);
        epic::remove_old(
            dir,
            old_files,
            game_files
                .iter()
                .chain(installed.iter().flat_map(|d| &d.files)),
        )
        .await;
        let _ = tokio::fs::remove_dir_all(work).await;
        if let Some(parent) = work.parent() {
            let _ = tokio::fs::remove_dir(parent).await; // only if empty
        }
        self.complete_epic(record, info, manifest, &installed, dir)
            .await
    }

    async fn complete_epic(
        &self,
        record: &db::downloads::Record,
        info: &epic::InstallInfo,
        manifest: &epic::format::Manifest,
        dlcs: &[db::store_installs::InstalledDlc],
        dir: &Path,
    ) -> AppResult<()> {
        let executable = epic::launch_file(dir, manifest);
        let windows_game = executable
            .as_deref()
            .and_then(Path::extension)
            .is_some_and(|e| e.eq_ignore_ascii_case("exe"));
        let compat = if windows_game && !cfg!(windows) {
            CompatibilityTool::Proton
        } else {
            CompatibilityTool::Native
        };
        let size = {
            let path = dir.to_path_buf();
            tokio::task::spawn_blocking(move || extract::dir_size(&path))
                .await
                .unwrap_or_else(|_| manifest.install_size())
        };
        let meta = EpicMeta {
            app_name: info.app_name.clone(),
            namespace: info.namespace.clone(),
            launch_command: manifest.launch_command.clone(),
            cloud_save_folder: info.cloud_save_folder.clone(),
            ownership_token: info.ownership_token,
        };
        // Last: `complete_install` marks the download as completed.
        db::library::set_compat_if_native(&self.pool, &record.game_id, compat).await?;
        db::store_installs::save(
            &self.pool,
            &db::store_installs::StoreInstall {
                game_id: record.game_id.clone(),
                store: StoreSource::Epic,
                product_id: info.app_name.clone(),
                build_id: info.build_version.clone(),
                version: info.build_version.clone(),
                platform: gog::host_platform().to_owned(),
                files: epic::installed_files(manifest),
                meta: serde_json::to_string(&meta)?,
            },
        )
        .await?;
        db::store_installs::save_dlcs(&self.pool, &record.game_id, dlcs).await?;
        db::downloads::complete_install(
            &self.pool,
            &record.id,
            &record.game_id,
            &dir.to_string_lossy(),
            executable
                .as_deref()
                .map(|p| p.to_string_lossy())
                .as_deref(),
            i64::try_from(size).unwrap_or(i64::MAX),
            compat,
        )
        .await?;
        (self.hooks.installed)(record.game_id.clone());
        Ok(())
    }

    async fn epic_meta(&self, game_id: &str) -> AppResult<EpicMeta> {
        let install = db::store_installs::get(&self.pool, game_id)
            .await?
            .filter(|i| i.store == StoreSource::Epic)
            .ok_or_else(|| {
                AppError::Launch(tr!(
                    "Cove hat dieses Spiel nicht installiert.",
                    "Cove didn't install this game."
                ))
            })?;
        Ok(serde_json::from_str(&install.meta)?)
    }

    /// Arguments for starting an Epic game Cove installed: a fresh exchange code,
    /// the ownership token where needed, and newer cloud saves first.
    pub async fn epic_launch_args(
        &self,
        item: &LibraryItem,
        host: &HostEnv,
    ) -> AppResult<Vec<String>> {
        let meta = self.epic_meta(&item.game.id).await?;
        let auth = self
            .stores()?
            .epic_launch(&meta.app_name, meta.ownership_token)
            .await?;
        let settings = self.settings().await?;
        let ovt = match &auth.ownership_token {
            Some(token) => Some(self.write_ownership_token(&meta, token, item, host).await?),
            None => None,
        };
        if settings.library.epic_cloud_saves && meta.cloud_save_folder.is_some() {
            // Never keep the game from starting; the next session tries again.
            if let Err(error) = self
                .sync_saves_down(item, &meta, &auth.account_id, host)
                .await
            {
                eprintln!("[cove] Cloud-Spielstände von {}: {error}", item.game.id);
            }
        }
        let locale = if settings.general.language == "de" {
            "de-DE"
        } else {
            "en-US"
        };
        Ok(epic::launch_args(&meta, &auth, locale, ovt.as_deref()))
    }

    /// Ownership token file; under Proton/Wine as a Windows path (drive Z: is `/`).
    async fn write_ownership_token(
        &self,
        meta: &EpicMeta,
        token: &str,
        item: &LibraryItem,
        host: &HostEnv,
    ) -> AppResult<String> {
        let bytes = base64::engine::general_purpose::STANDARD
            .decode(token)
            .map_err(|_| {
                AppError::Launch(tr!(
                    "Epic hat ein ungültiges Token geliefert.",
                    "Epic sent an invalid token."
                ))
            })?;
        let dir = data_dir(host).join("epic-tokens");
        tokio::fs::create_dir_all(&dir).await?;
        let path = dir.join(format!("{}.ovt", crate::db::slugify(&meta.app_name)));
        tokio::fs::write(&path, bytes).await?;
        let windows_game = item
            .entry
            .executable_path
            .as_deref()
            .is_some_and(|e| e.to_lowercase().ends_with(".exe"));
        Ok(if windows_game && !cfg!(windows) {
            format!("Z:{}", path.to_string_lossy().replace('/', "\\"))
        } else {
            path.to_string_lossy().into_owned()
        })
    }

    /// The game's save folder on this computer (inside the Proton/Wine prefix on Linux).
    fn save_dir(
        item: &LibraryItem,
        meta: &EpicMeta,
        account_id: &str,
        host: &HostEnv,
    ) -> Option<PathBuf> {
        let template = meta.cloud_save_folder.as_deref()?;
        let install = PathBuf::from(item.entry.install_path.as_deref()?);
        let dirs = if cfg!(windows) {
            saves::UserDirs::windows()?
        } else {
            match item.entry.launch_options.compatibility_tool {
                CompatibilityTool::Proton => {
                    saves::UserDirs::proton(&host.compat_data_root.join(&item.game.id))
                }
                CompatibilityTool::Wine => {
                    let user = std::env::var("USER").unwrap_or_else(|_| "user".into());
                    saves::UserDirs::of_profile(
                        host.home.as_ref()?.join(".wine/drive_c/users").join(user),
                    )
                }
                CompatibilityTool::Native => return None,
            }
        };
        saves::save_folder(template, &dirs, &install, account_id)
    }

    async fn sync_saves_down(
        &self,
        item: &LibraryItem,
        meta: &EpicMeta,
        account_id: &str,
        host: &HostEnv,
    ) -> AppResult<()> {
        let Some(dir) = Self::save_dir(item, meta, account_id, host) else {
            return Ok(());
        };
        let cloud = self.stores()?.epic_saves(&meta.app_name).await?;
        let cloud_time = saves::newest_manifest(&cloud).and_then(|f| f.last_modified);
        let local = dir.clone();
        let local_time = tokio::task::spawn_blocking(move || saves::newest_local(&local))
            .await
            .unwrap_or(None);
        if !saves::cloud_is_newer(cloud_time, local_time) {
            return Ok(());
        }
        let (from, backups) = (
            dir.clone(),
            data_dir(host).join("save-backups").join(&item.game.id),
        );
        tokio::task::spawn_blocking(move || saves::back_up(&from, &backups))
            .await
            .map_err(|_| AppError::Download("backup".into()))??;
        let prefix = format!("{account_id}/{}/", meta.app_name);
        let count = saves::download(&self.client, &cloud, &prefix, &dir).await?;
        eprintln!(
            "[cove] {count} Cloud-Spielstand-Datei(en) für {} geladen",
            item.game.id
        );
        Ok(())
    }

    /// After a session: newer local saves of an Epic game Cove installed go to the cloud.
    pub async fn after_session(&self, game_id: &str, host: &HostEnv) -> AppResult<()> {
        let item = db::library::require(&self.pool, game_id).await?;
        if item.entry.source != StoreSource::Epic || !item.entry.installed_by_cove {
            return Ok(());
        }
        if !self.settings().await?.library.epic_cloud_saves {
            return Ok(());
        }
        let meta = self.epic_meta(game_id).await?;
        let account = self.stores()?.session(StoreSource::Epic).await?.account_id;
        let Some(dir) = Self::save_dir(&item, &meta, &account, host) else {
            return Ok(());
        };
        let cloud = self.stores()?.epic_saves(&meta.app_name).await?;
        let cloud_time = saves::newest_manifest(&cloud).and_then(|f| f.last_modified);
        let local = dir.clone();
        let local_time = tokio::task::spawn_blocking(move || saves::newest_local(&local))
            .await
            .unwrap_or(None);
        if !saves::local_is_newer(cloud_time, local_time) {
            return Ok(());
        }
        let prefix = format!("{account}/{}/", meta.app_name);
        let template = meta.cloud_save_folder.clone().unwrap_or_default();
        let app = meta.app_name.clone();
        let files = tokio::task::spawn_blocking(move || {
            saves::pack(&dir, &prefix, &app, &template, chrono::Utc::now())
        })
        .await
        .map_err(|_| AppError::Download("pack".into()))??;
        let paths: Vec<String> = files.iter().map(|(p, _)| p.clone()).collect();
        let links = self
            .stores()?
            .epic_write_links(&meta.app_name, &paths)
            .await?;
        saves::upload(&self.client, files, &links).await?;
        eprintln!("[cove] Cloud-Spielstände von {game_id} hochgeladen");
        Ok(())
    }
}

/// Cove's app data folder (parent of the Proton prefixes).
fn data_dir(host: &HostEnv) -> PathBuf {
    host.compat_data_root
        .parent()
        .map_or_else(|| host.compat_data_root.clone(), Path::to_path_buf)
}
