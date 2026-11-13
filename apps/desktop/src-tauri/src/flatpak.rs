//! Cove inside a Flatpak sandbox (Flathub build, `packaging/flatpak`).
//!
//! Games Cove starts itself run inside the sandbox (with the 32-bit and Vulkan
//! layer extensions). Programs of the host (Steam, emulators installed by the
//! system or as Flatpak) run on the host through `flatpak-spawn --host`; the
//! sandbox sees the host's `/usr` under `/run/host` (`--filesystem=host-os:ro`).
//! Host processes are invisible in the sandbox's process list, so games that
//! a store client starts are followed through `/proc` of the host.

use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::OnceLock;

use crate::launcher::ProcInfo;

/// The host's root as the sandbox sees it.
pub const HOST_ROOT: &str = "/run/host";
const SPAWN: &str = "flatpak-spawn";

/// Whether Cove runs inside Flatpak.
pub fn active() -> bool {
    static ACTIVE: OnceLock<bool> = OnceLock::new();
    *ACTIVE.get_or_init(|| Path::new("/.flatpak-info").is_file())
}

/// Folders of the host's `PATH`, as the sandbox sees them.
pub fn host_path_dirs(home: Option<&Path>) -> Vec<PathBuf> {
    let mut dirs: Vec<PathBuf> = ["usr/local/bin", "usr/bin", "usr/games", "bin"]
        .iter()
        .map(|d| Path::new(HOST_ROOT).join(d))
        .collect();
    if let Some(home) = home {
        dirs.insert(0, home.join(".local/bin"));
        dirs.insert(1, home.join("bin"));
    }
    dirs
}

/// `/run/host/usr/bin/x` → `/usr/bin/x`; other paths stay.
pub fn to_host(path: &str) -> String {
    match path.strip_prefix(HOST_ROOT) {
        Some(rest) if rest.starts_with('/') => rest.to_owned(),
        _ => path.to_owned(),
    }
}

fn is_host_path(path: &str) -> bool {
    path.strip_prefix(HOST_ROOT)
        .is_some_and(|rest| rest.starts_with('/'))
}

/// A command routed for the sandbox: either as it is, or on the host.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Routed {
    pub program: PathBuf,
    pub args: Vec<String>,
    pub env: Vec<(String, String)>,
    /// True when the command runs on the host (game options don't apply there).
    pub on_host: bool,
}

/// Runs a command on the host when it is a host program. The first host path
/// (the program or, behind sandbox wrappers like `gamemoderun`, an argument)
/// starts the host command; wrappers before it are dropped, since they exist
/// only in the sandbox.
pub fn route(
    program: &Path,
    args: &[String],
    env: &[(String, String)],
    cwd: Option<&Path>,
) -> Routed {
    let program_text = program.to_string_lossy().into_owned();
    let tokens: Vec<&str> = std::iter::once(program_text.as_str())
        .chain(args.iter().map(String::as_str))
        .collect();
    let Some(start) = tokens.iter().position(|t| is_host_path(t)) else {
        return Routed {
            program: program.to_path_buf(),
            args: args.to_vec(),
            env: env.to_vec(),
            on_host: false,
        };
    };
    let mut spawn_args = vec!["--host".to_owned(), "--watch-bus".to_owned()];
    for (key, value) in env {
        spawn_args.push(format!("--env={key}={}", to_host(value)));
    }
    if let Some(dir) = cwd {
        spawn_args.push(format!("--directory={}", to_host(&dir.to_string_lossy())));
    }
    spawn_args.extend(tokens.iter().skip(start).map(|t| to_host(t)));
    Routed {
        program: PathBuf::from(SPAWN),
        args: spawn_args,
        env: Vec::new(),
        on_host: true,
    }
}

/// A program on the host (`flatpak-spawn --host <program> …`).
pub fn host_command(program: &str, args: &[&str]) -> Command {
    let mut command = Command::new(SPAWN);
    command.arg("--host").arg(program).args(args);
    command
}

/// Whether a Flatpak app is installed on the host.
pub fn host_app_installed(app: &str) -> bool {
    host_command("flatpak", &["info", app])
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .is_ok_and(|s| s.success())
}

/// The host's processes: `/proc/<pid>/stat` of all, plus the executables of
/// the user's own. Blocking (runs one host command).
pub fn host_processes() -> Vec<ProcInfo> {
    let script = "cat /proc/[0-9]*/stat 2>/dev/null; echo @@; \
                  find /proc -mindepth 2 -maxdepth 2 -name exe -printf '%h %l\\n' 2>/dev/null";
    let Ok(output) = host_command("sh", &["-c", script])
        .stdin(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .output()
    else {
        return Vec::new();
    };
    parse_host_processes(&String::from_utf8_lossy(&output.stdout))
}

/// Names of the host's processes (`comm`, at most 15 characters).
pub fn host_process_names() -> Vec<String> {
    let script = "cat /proc/[0-9]*/comm 2>/dev/null";
    host_command("sh", &["-c", script])
        .stdin(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .output()
        .map(|o| {
            String::from_utf8_lossy(&o.stdout)
                .lines()
                .map(str::to_owned)
                .collect()
        })
        .unwrap_or_default()
}

/// Sends a signal to host processes (`TERM` or `KILL`).
pub fn kill_on_host(pids: &[u32], signal: &str) {
    if pids.is_empty() {
        return;
    }
    let mut args = vec![format!("-{signal}")];
    args.extend(pids.iter().map(u32::to_string));
    let refs: Vec<&str> = args.iter().map(String::as_str).collect();
    let _ = host_command("kill", &refs)
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status();
}

fn parse_host_processes(text: &str) -> Vec<ProcInfo> {
    let (stat_lines, exes) = text.split_once("@@").unwrap_or((text, ""));
    let exes: std::collections::HashMap<u32, PathBuf> = exes
        .lines()
        .filter_map(|line| {
            let (dir, target) = line.trim().split_once(' ')?;
            let pid = dir.strip_prefix("/proc/")?.parse().ok()?;
            Some((pid, PathBuf::from(target)))
        })
        .collect();
    stat_lines
        .lines()
        .filter_map(|line| {
            let (head, rest) = line.rsplit_once(')')?;
            let pid: u32 = head.split_whitespace().next()?.parse().ok()?;
            // After the name: state ppid pgrp session tty tpgid flags minflt
            // cminflt majflt cmajflt utime stime cutime cstime priority nice
            // threads itrealvalue starttime …
            let fields: Vec<&str> = rest.split_whitespace().collect();
            let run_state = *fields.first()?;
            let parent = fields.get(1)?.parse().ok().filter(|p| *p > 0);
            let group = fields.get(2)?.parse().ok();
            let start_time = fields.get(19)?.parse().ok()?;
            Some(ProcInfo {
                pid,
                parent,
                start_time,
                exe: exes.get(&pid).cloned(),
                pgid: group,
                zombie: run_state == "Z" || run_state == "X",
            })
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn host_programs_run_on_the_host_without_sandbox_wrappers() {
        let env = vec![("SDL_VIDEODRIVER".to_owned(), "wayland".to_owned())];
        let routed = route(
            Path::new("/usr/bin/gamemoderun"),
            &[
                "/run/host/usr/bin/retroarch".into(),
                "-L".into(),
                "/run/host/usr/lib64/libretro/snes9x_libretro.so".into(),
                "/home/u/ROMs/a.sfc".into(),
            ],
            &env,
            Some(Path::new("/home/u/ROMs")),
        );
        assert!(routed.on_host);
        assert_eq!(routed.program, PathBuf::from("flatpak-spawn"));
        assert_eq!(
            routed.args,
            [
                "--host",
                "--watch-bus",
                "--env=SDL_VIDEODRIVER=wayland",
                "--directory=/home/u/ROMs",
                "/usr/bin/retroarch",
                "-L",
                "/usr/lib64/libretro/snes9x_libretro.so",
                "/home/u/ROMs/a.sfc",
            ]
        );
    }

    #[test]
    fn sandbox_programs_stay_in_the_sandbox() {
        let routed = route(
            Path::new("/home/u/Games/x/start.sh"),
            &["--fullscreen".into()],
            &[],
            None,
        );
        assert!(!routed.on_host);
        assert_eq!(routed.program, PathBuf::from("/home/u/Games/x/start.sh"));
        assert_eq!(to_host("/run/hostile"), "/run/hostile");
    }

    #[test]
    fn host_processes_are_read_from_proc() {
        let text = "\
1 (systemd) S 0 1 1 0 -1 4194560 0 0 0 0 0 0 0 0 20 0 1 0 12 0 0\n\
4242 (Game (x64)) S 4100 4100 4100 0 -1 0 0 0 0 0 0 0 0 0 20 0 1 0 98765 0 0\n\
4300 (defunct) Z 4242 4100 4100 0 -1 0 0 0 0 0 0 0 0 0 20 0 1 0 99000 0 0\n\
@@\n\
/proc/4242 /home/u/.steam/steamapps/common/Game/game.x86_64\n";
        let procs = parse_host_processes(text);
        assert_eq!(procs.len(), 3);
        let game = procs.iter().find(|p| p.pid == 4242).unwrap();
        assert_eq!(game.parent, Some(4100));
        assert_eq!(game.pgid, Some(4100));
        assert_eq!(game.start_time, 98765);
        assert_eq!(
            game.exe.as_deref(),
            Some(Path::new(
                "/home/u/.steam/steamapps/common/Game/game.x86_64"
            ))
        );
        assert!(procs.iter().any(|p| p.pid == 4300 && p.zombie));
        assert_eq!(procs.iter().find(|p| p.pid == 1).unwrap().parent, None);
    }
}
