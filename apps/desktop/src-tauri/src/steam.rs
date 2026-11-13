//! Steam in the background. Steam games always start through Steam
//! (`steam://rungameid/<appid>`, no DRM bypass). When Steam is not running,
//! Cove first starts it with `-silent` (tray only, no window), waits until it
//! is ready and only then hands over the URL, so no Steam window shows up.

use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use sysinfo::{ProcessRefreshKind, ProcessesToUpdate, System, UpdateKind};

use crate::error::{AppError, AppResult};

/// Give up waiting after this long and hand over the URL anyway.
const READY_TIMEOUT: Duration = Duration::from_secs(90);
const POLL: Duration = Duration::from_millis(500);
/// Without a signed-in user (unknown on some systems), the web helper must have
/// been running this long before Steam reliably accepts URLs.
const SETTLE: Duration = Duration::from_secs(5);

fn is_client(name: &str) -> bool {
    let name = name.to_ascii_lowercase();
    name == "steam" || name == "steam.exe"
}

fn is_helper(name: &str) -> bool {
    name.to_ascii_lowercase().starts_with("steamwebhelper")
}

#[derive(Debug, Clone, Copy, Default)]
struct Seen {
    client: bool,
    helper: bool,
}

fn look(system: &mut System) -> Seen {
    if crate::flatpak::active() {
        // Steam runs on the host, outside the sandbox's process list.
        let mut seen = Seen::default();
        for name in crate::flatpak::host_process_names() {
            seen.client |= is_client(&name);
            seen.helper |= is_helper(&name);
        }
        return seen;
    }
    system.refresh_processes_specifics(
        ProcessesToUpdate::All,
        true,
        ProcessRefreshKind::nothing().with_exe(UpdateKind::Never),
    );
    let mut seen = Seen::default();
    for process in system.processes().values() {
        let name = process.name().to_string_lossy();
        seen.client |= is_client(&name);
        seen.helper |= is_helper(&name);
    }
    seen
}

/// Finds `name` in a `PATH`-style list of folders.
fn find_in_path(path_var: Option<&OsString>, name: &str) -> Option<PathBuf> {
    std::env::split_paths(path_var?)
        .map(|dir| dir.join(name))
        .find(|p| p.is_file())
}

/// Program and arguments that start Steam silently on this machine.
fn start_command(
    home: Option<&Path>,
    path_var: Option<&OsString>,
) -> Option<(PathBuf, Vec<String>)> {
    let silent = || vec!["-silent".to_owned()];
    if cfg!(windows) {
        return windows_steam_exe().map(|exe| (exe, silent()));
    }
    if cfg!(target_os = "macos") {
        let app = Path::new("/Applications/Steam.app");
        return app.is_dir().then(|| {
            let args = ["-g", "-a", "Steam", "--args", "-silent"];
            (PathBuf::from("open"), args.map(str::to_owned).to_vec())
        });
    }
    if crate::flatpak::active() {
        return flatpak_start_command(home);
    }
    if let Some(steam) = find_in_path(path_var, "steam") {
        return Some((steam, silent()));
    }
    let flatpak_installed = home
        .is_some_and(|h| h.join(".var/app/com.valvesoftware.Steam").is_dir())
        || Path::new("/var/lib/flatpak/app/com.valvesoftware.Steam").is_dir();
    let flatpak = find_in_path(path_var, "flatpak").filter(|_| flatpak_installed)?;
    let args = ["run", "com.valvesoftware.Steam", "-silent"];
    Some((flatpak, args.map(str::to_owned).to_vec()))
}

/// Inside Flatpak: Steam of the host (installed by the system or as Flatpak).
fn flatpak_start_command(home: Option<&Path>) -> Option<(PathBuf, Vec<String>)> {
    let host_dirs = crate::flatpak::host_path_dirs(home);
    let host_steam = host_dirs
        .iter()
        .map(|d| d.join("steam"))
        .find(|p| p.is_file())
        .map(|p| crate::flatpak::to_host(&p.to_string_lossy()));
    let spawn = PathBuf::from("flatpak-spawn");
    if let Some(steam) = host_steam {
        return Some((spawn, vec!["--host".into(), steam, "-silent".into()]));
    }
    crate::flatpak::host_app_installed("com.valvesoftware.Steam").then(|| {
        let args = [
            "--host",
            "flatpak",
            "run",
            "com.valvesoftware.Steam",
            "-silent",
        ];
        (spawn, args.map(str::to_owned).to_vec())
    })
}

#[cfg(windows)]
fn windows_steam_exe() -> Option<PathBuf> {
    use winreg::enums::HKEY_CURRENT_USER;
    let from_registry = winreg::RegKey::predef(HKEY_CURRENT_USER)
        .open_subkey("Software\\Valve\\Steam")
        .and_then(|key| key.get_value::<String, _>("SteamExe"))
        .ok()
        .map(PathBuf::from);
    let fallback = std::env::var_os("ProgramFiles(x86)")
        .map(|p| PathBuf::from(p).join("Steam").join("steam.exe"));
    from_registry
        .into_iter()
        .chain(fallback)
        .find(|p| p.is_file())
}

#[cfg(not(windows))]
fn windows_steam_exe() -> Option<PathBuf> {
    None
}

/// Account id of the signed-in Steam user (0 or None = nobody yet).
#[cfg(windows)]
fn active_user(_home: Option<&Path>) -> Option<u32> {
    use winreg::enums::HKEY_CURRENT_USER;
    winreg::RegKey::predef(HKEY_CURRENT_USER)
        .open_subkey("Software\\Valve\\Steam\\ActiveProcess")
        .and_then(|key| key.get_value::<u32, _>("ActiveUser"))
        .ok()
}

#[cfg(not(windows))]
fn active_user(home: Option<&Path>) -> Option<u32> {
    let text = std::fs::read_to_string(home?.join(".steam/registry.vdf")).ok()?;
    active_user_from_vdf(&text)
}

/// `"ActiveUser"  "123"` inside Steam's `registry.vdf`.
#[cfg_attr(windows, allow(dead_code))]
fn active_user_from_vdf(text: &str) -> Option<u32> {
    text.lines().find_map(|line| {
        let mut parts = line.split('"').filter(|p| !p.trim().is_empty());
        (parts.next()? == "ActiveUser")
            .then(|| parts.next()?.parse().ok())
            .flatten()
    })
}

/// Steam is ready for `steam://` URLs.
fn ready(seen: Seen, helper_since: Option<Instant>, active_user: Option<u32>) -> bool {
    if !seen.client || !seen.helper {
        return false;
    }
    active_user.is_some_and(|u| u > 0) || helper_since.is_some_and(|t| t.elapsed() >= SETTLE)
}

/// Starts Steam silently unless it runs already, and waits until it is ready
/// (at most 90 s; after that the URL is handed over anyway).
pub async fn ensure_running(home: Option<PathBuf>) -> AppResult<()> {
    let mut system = System::new();
    if look(&mut system).client {
        return Ok(());
    }
    let path_var = std::env::var_os("PATH");
    let (program, args) = start_command(home.as_deref(), path_var.as_ref()).ok_or_else(|| {
        AppError::Launch(tr!(
            "Steam wurde nicht gefunden. Ist Steam installiert?",
            "Steam wasn't found. Is Steam installed?"
        ))
    })?;
    let mut command = Command::new(&program);
    command
        .args(&args)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        command.process_group(0);
    }
    let mut child = command.spawn().map_err(|e| {
        eprintln!("[cove] Steam starten: {e}");
        AppError::Launch(tr!(
            "Steam konnte nicht gestartet werden.",
            "Steam could not be started."
        ))
    })?;
    std::thread::spawn(move || {
        let _ = child.wait();
    });

    let started = Instant::now();
    let mut helper_since = None;
    while started.elapsed() < READY_TIMEOUT {
        tokio::time::sleep(POLL).await;
        let seen = look(&mut system);
        if seen.helper {
            helper_since.get_or_insert_with(Instant::now);
        } else {
            helper_since = None;
        }
        if ready(seen, helper_since, active_user(home.as_deref())) {
            return Ok(());
        }
    }
    eprintln!("[cove] Steam meldet sich nicht als bereit; der Link wird trotzdem übergeben");
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn process_names() {
        assert!(is_client("steam"));
        assert!(is_client("Steam.exe"));
        assert!(!is_client("steamwebhelper"));
        assert!(is_helper("steamwebhelper.exe"));
        assert!(!is_helper("steam"));
    }

    #[test]
    fn reads_the_signed_in_user_from_registry_vdf() {
        let vdf = "\"Registry\"\n{\n\t\"HKCU\"\n\t{\n\t\t\"Software\"\n\t\t{\n\t\t\t\"Valve\"\n\t\t\t{\n\t\t\t\t\"Steam\"\n\t\t\t\t{\n\t\t\t\t\t\"ActiveProcess\"\n\t\t\t\t\t{\n\t\t\t\t\t\t\"pid\"\t\t\"4242\"\n\t\t\t\t\t\t\"ActiveUser\"\t\t\"98765\"\n\t\t\t\t\t}\n\t\t\t\t}\n\t\t\t}\n\t\t}\n\t}\n}\n";
        assert_eq!(active_user_from_vdf(vdf), Some(98_765));
        assert_eq!(active_user_from_vdf("\"ActiveUser\"\t\"0\""), Some(0));
        assert_eq!(active_user_from_vdf("\"AutoLoginUser\"\t\"ada\""), None);
    }

    #[test]
    fn ready_once_signed_in_or_settled() {
        let up = Seen {
            client: true,
            helper: true,
        };
        let just_now = Some(Instant::now());
        let long_ago = Instant::now().checked_sub(SETTLE * 2);
        assert!(!ready(Seen::default(), long_ago, Some(5)));
        assert!(!ready(
            Seen {
                client: true,
                helper: false
            },
            None,
            Some(5)
        ));
        assert!(ready(up, just_now, Some(5)));
        assert!(!ready(up, just_now, Some(0)));
        assert!(!ready(up, just_now, None));
        assert!(ready(up, long_ago, None));
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn finds_steam_in_path_or_as_flatpak() {
        let bin = tempfile::tempdir().unwrap();
        let path_var = OsString::from(bin.path());
        assert_eq!(start_command(None, Some(&path_var)), None);

        std::fs::write(bin.path().join("flatpak"), "").unwrap();
        let home = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(home.path().join(".var/app/com.valvesoftware.Steam")).unwrap();
        let (program, args) = start_command(Some(home.path()), Some(&path_var)).unwrap();
        assert_eq!(program, bin.path().join("flatpak"));
        assert_eq!(args, ["run", "com.valvesoftware.Steam", "-silent"]);

        std::fs::write(bin.path().join("steam"), "").unwrap();
        let (program, args) = start_command(Some(home.path()), Some(&path_var)).unwrap();
        assert_eq!(program, bin.path().join("steam"));
        assert_eq!(args, ["-silent"]);
    }
}
