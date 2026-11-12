//! The download manager's part of GOG cloud saves: which games take part and
//! when the files move (see `gog_saves` for how).

use std::path::{Path, PathBuf};

use super::epic::saves::UserDirs;
use super::{gog_saves, DownloadManager};
use crate::error::AppResult;
use crate::launcher::HostEnv;
use crate::models::{CompatibilityTool, LibraryItem, StoreSource};

/// Cove's app data folder (parent of the Proton prefixes).
fn data_dir(host: &HostEnv) -> PathBuf {
    host.compat_data_root
        .parent()
        .map_or_else(|| host.compat_data_root.clone(), Path::to_path_buf)
}

/// The Windows folders of a GOG game Cove installed and its install folder:
/// this user on Windows, the prefix's user with Proton or Wine. None for
/// native Linux builds and games Cove did not install.
fn save_dirs(item: &LibraryItem, host: &HostEnv) -> Option<(UserDirs, PathBuf)> {
    if item.entry.source != StoreSource::Gog || !item.entry.installed_by_cove {
        return None;
    }
    let install = PathBuf::from(item.entry.install_path.as_deref()?);
    if cfg!(windows) {
        return Some((UserDirs::windows()?, install));
    }
    let windows_game = item
        .entry
        .executable_path
        .as_deref()
        .is_some_and(|e| e.to_lowercase().ends_with(".exe"));
    if !windows_game {
        return None;
    }
    let dirs = match item.entry.launch_options.compatibility_tool {
        CompatibilityTool::Proton => UserDirs::proton(&host.compat_data_root.join(&item.game.id)),
        CompatibilityTool::Wine => {
            let user = std::env::var("USER").unwrap_or_else(|_| "user".into());
            UserDirs::of_profile(host.home.as_ref()?.join(".wine/drive_c/users").join(user))
        }
        CompatibilityTool::Native => return None,
    };
    Some((dirs, install))
}

impl DownloadManager {
    /// The connector's answer for a game that takes part, else None.
    async fn gog_cloud(&self, item: &LibraryItem) -> AppResult<Option<gog_saves::CloudSaves>> {
        if !self.settings().await?.library.gog_cloud_saves {
            return Ok(None);
        }
        let Some(product) = item.entry.store_game_id.as_deref() else {
            return Ok(None);
        };
        let saves = self.stores()?.gog_cloud_saves(product).await?;
        Ok(saves.enabled.then_some(saves))
    }

    /// Before a GOG game Cove installed starts: newer cloud saves come down
    /// (local ones are backed up first). Never keeps the game from starting.
    pub async fn gog_saves_before_launch(&self, item: &LibraryItem, host: &HostEnv) {
        let Some((dirs, install)) = save_dirs(item, host) else {
            return;
        };
        let result = async {
            let Some(saves) = self.gog_cloud(item).await? else {
                return Ok(0);
            };
            let backups = data_dir(host).join("save-backups").join(&item.game.id);
            gog_saves::sync_down(&self.client, &saves, &dirs, &install, &backups).await
        }
        .await;
        match result {
            Ok(0) => {}
            Ok(count) => eprintln!(
                "[cove] {count} GOG-Cloud-Spielstand-Datei(en) für {} geladen",
                item.game.id
            ),
            Err(error) => eprintln!("[cove] GOG-Cloud-Spielstände von {}: {error}", item.game.id),
        }
    }

    /// After a session of a GOG game Cove installed: newer local saves go up.
    pub(super) async fn gog_after_session(
        &self,
        item: &LibraryItem,
        host: &HostEnv,
    ) -> AppResult<()> {
        let Some((dirs, install)) = save_dirs(item, host) else {
            return Ok(());
        };
        let Some(saves) = self.gog_cloud(item).await? else {
            return Ok(());
        };
        let count = gog_saves::sync_up(&self.client, &saves, &dirs, &install).await?;
        if count > 0 {
            eprintln!(
                "[cove] {count} GOG-Cloud-Spielstand-Datei(en) von {} hochgeladen",
                item.game.id
            );
        }
        Ok(())
    }
}
