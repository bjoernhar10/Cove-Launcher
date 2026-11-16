//! Windows game options outside the game itself: the "performance mode"
//! (high process priority, the "High performance" power plan while a game
//! runs) and `RivaTuner` Statistics Server for its overlay. Cove never injects
//! anything into a game; RTSS does its overlay on its own.

use std::path::{Path, PathBuf};
use std::sync::Mutex;

/// Windows extras of a launch (from the game's options).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Boost {
    /// High priority + the "High performance" power plan.
    pub performance: bool,
    /// `RivaTuner` Statistics Server to start before the game.
    pub rtss: Option<PathBuf>,
}

/// `HIGH_PRIORITY_CLASS` for `CreateProcess`.
#[cfg_attr(not(windows), allow(dead_code))]
pub const HIGH_PRIORITY_CLASS: u32 = 0x0000_0080;

/// powercfg's alias for the "High performance" plan.
const HIGH_PERFORMANCE: &str = "SCHEME_MIN";

/// Where RTSS installs itself (it comes with MSI Afterburner too).
pub fn find_rtss() -> Option<PathBuf> {
    ["ProgramFiles(x86)", "ProgramFiles"]
        .iter()
        .filter_map(std::env::var_os)
        .map(|base| {
            PathBuf::from(base)
                .join("RivaTuner Statistics Server")
                .join("RTSS.exe")
        })
        .find(|p| p.is_file())
}

/// Starts RTSS unless it already runs. Never keeps the game from starting.
pub fn start_rtss(path: &Path) {
    let mut system = sysinfo::System::new();
    system.refresh_processes(sysinfo::ProcessesToUpdate::All, true);
    let running = system
        .processes()
        .values()
        .any(|p| p.name().eq_ignore_ascii_case("RTSS.exe"));
    if running {
        return;
    }
    if let Err(error) = std::process::Command::new(path).spawn() {
        eprintln!("[cove] RTSS konnte nicht gestartet werden: {error}");
    }
}

/// Power plans switched by running games: how many hold it and the plan to go back to.
static POWER: Mutex<(usize, Option<String>)> = Mutex::new((0, None));

/// Holds the "High performance" plan until dropped; the last one restores the
/// plan that was active before.
#[derive(Debug)]
pub struct PowerGuard(());

impl PowerGuard {
    pub fn acquire() -> Self {
        let mut power = POWER
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if power.0 == 0 {
            power.1 = powercfg(&["/getactivescheme"]).and_then(|out| scheme_guid(&out));
            // Machines with Modern Standby only have "Balanced": then nothing changes.
            if powercfg(&["/setactive", HIGH_PERFORMANCE]).is_none() {
                power.1 = None;
            }
        }
        power.0 += 1;
        Self(())
    }
}

impl Drop for PowerGuard {
    fn drop(&mut self) {
        let mut power = POWER
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        power.0 = power.0.saturating_sub(1);
        if power.0 == 0 {
            if let Some(previous) = power.1.take() {
                let _ = powercfg(&["/setactive", &previous]);
            }
        }
    }
}

/// Runs powercfg without a console window; its output when it succeeded.
fn powercfg(args: &[&str]) -> Option<String> {
    let mut command = std::process::Command::new("powercfg");
    command.args(args);
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        command.creation_flags(CREATE_NO_WINDOW);
    }
    let output = command.output().ok()?;
    output
        .status
        .success()
        .then(|| String::from_utf8_lossy(&output.stdout).into_owned())
}

/// The GUID in `powercfg /getactivescheme` ("Power Scheme GUID: 381b4222-… (Balanced)"),
/// whatever the system language.
fn scheme_guid(output: &str) -> Option<String> {
    output
        .split(|c: char| c.is_whitespace() || c == '(' || c == ')')
        .find(|word| {
            word.len() == 36
                && word.chars().filter(|c| *c == '-').count() == 4
                && word.chars().all(|c| c.is_ascii_hexdigit() || c == '-')
        })
        .map(str::to_owned)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_the_active_power_plan_in_any_language() {
        assert_eq!(
            scheme_guid("Power Scheme GUID: 381b4222-f694-41f0-9685-ff5bb260df2e  (Balanced)\r\n")
                .as_deref(),
            Some("381b4222-f694-41f0-9685-ff5bb260df2e")
        );
        assert_eq!(
            scheme_guid(
                "GUID des Energieschemas: 8c5e7fda-e8bf-4a96-9a85-a6e23a8c635c  (Höchstleistung)"
            )
            .as_deref(),
            Some("8c5e7fda-e8bf-4a96-9a85-a6e23a8c635c")
        );
        assert_eq!(scheme_guid("nothing here"), None);
    }
}
