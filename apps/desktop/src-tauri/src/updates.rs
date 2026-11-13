//! App updates through the Tauri updater plugin.
//!
//! Releases on GitHub carry a signed `latest.json` (see `.github/workflows/release.yml`);
//! the public key in `tauri.conf.json` checks the signature before anything is installed.
//! On Linux only the `AppImage` can replace itself; `.deb`/`.rpm` installs are updated
//! by the package manager, so Cove only points to the new version there.

use tauri::{AppHandle, Emitter, Manager, Runtime};
use tauri_plugin_updater::{Updater, UpdaterExt};

use crate::error::{AppError, AppResult};
use crate::models::{UpdateCheck, UpdateInfo, UpdateProgress};

pub const UPDATE_PROGRESS: &str = "update-progress";

/// Managed by `run()` once the updater plugin is registered (the plugin
/// panics when used without it, e.g. in the IPC tests).
pub struct UpdaterRegistered;

fn updater<R: Runtime>(app: &AppHandle<R>) -> AppResult<Updater> {
    if app.try_state::<UpdaterRegistered>().is_none() {
        return Err(AppError::Update(tr!(
            "Updates sind in diesem Build nicht eingerichtet.",
            "Updates aren't set up in this build."
        )));
    }
    app.updater().map_err(|e| update_error(&e))
}

/// Whether this installation can replace itself.
fn can_self_update() -> bool {
    if cfg!(target_os = "linux") {
        std::env::var_os("APPIMAGE").is_some() && !crate::flatpak::active()
    } else {
        true
    }
}

fn install_hint() -> Option<String> {
    (!can_self_update()).then(|| {
        if crate::flatpak::active() {
            return tr!("Cove wurde als Flatpak installiert. Updates kommen über deine Softwareverwaltung oder „flatpak update“.", "Cove was installed as a Flatpak. Updates come through your software centre or “flatpak update”.");
        }
        tr!("Cove wurde über ein Paket (.deb/.rpm) installiert. Aktualisiere es über deine Paketverwaltung oder lade die neue Version von GitHub.", "Cove was installed from a package (.deb/.rpm). Update it with your package manager or download the new version from GitHub.")
    })
}

/// Turns updater failures into plain text; details go to the log.
fn update_error(error: &impl std::fmt::Display) -> AppError {
    eprintln!("[updater] {error}");
    AppError::Update(tr!(
        "Der Update-Server ist gerade nicht erreichbar. Versuch es später noch einmal.",
        "The update server can't be reached right now. Please try again later."
    ))
}

pub async fn check<R: Runtime>(app: &AppHandle<R>) -> AppResult<UpdateCheck> {
    let updater = updater(app)?;
    let update = updater.check().await.map_err(|e| update_error(&e))?;
    Ok(UpdateCheck {
        current_version: app.package_info().version.to_string(),
        available: update.map(|u| UpdateInfo {
            version: u.version,
            notes: u.body,
            date: u.date.map(|d| d.to_string()),
        }),
        can_install: can_self_update(),
        install_hint: install_hint(),
    })
}

/// Downloads, verifies and installs the newest version, then restarts Cove.
pub async fn install<R: Runtime>(app: &AppHandle<R>) -> AppResult<()> {
    if !can_self_update() {
        return Err(AppError::Update(install_hint().unwrap_or_default()));
    }
    let updater = updater(app)?;
    let Some(update) = updater.check().await.map_err(|e| update_error(&e))? else {
        return Err(AppError::Update(tr!(
            "Cove ist bereits auf dem neuesten Stand.",
            "Cove is already up to date."
        )));
    };
    let progress_app = app.clone();
    let mut downloaded: u64 = 0;
    update
        .download_and_install(
            move |chunk, total| {
                downloaded += chunk as u64;
                let _ = progress_app.emit(
                    UPDATE_PROGRESS,
                    UpdateProgress {
                        downloaded_bytes: downloaded,
                        total_bytes: total,
                    },
                );
            },
            || {},
        )
        .await
        .map_err(|e| {
            eprintln!("[updater] {e}");
            AppError::Update(
                tr!("Das Update konnte nicht installiert werden. Deine bisherige Version bleibt erhalten.", "The update could not be installed. Your current version is kept."),
            )
        })?;
    app.restart();
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn update_errors_never_show_technical_details() {
        let error = update_error(&"error sending request for url (https://example.invalid)");
        assert_eq!(error.kind(), "update");
        assert!(!error.to_string().contains("http"), "{error}");
    }

    #[test]
    fn package_installs_get_a_hint_instead_of_self_update() {
        assert_eq!(install_hint().is_some(), !can_self_update());
    }
}
