//! Starting games and tracking their playtime.
//!
//! - [`plan`] decides *how* a game starts (store URL scheme, native process,
//!   Wine/Proton, `.desktop` entry). Pure and unit-tested.
//! - [`Tracker`] follows a process tree (root + all descendants, even after the
//!   launcher exits) or every process running from an install folder (for
//!   store-client launches). Pure and unit-tested.
//! - [`Launcher`] spawns, monitors, records `play_sessions` (with heartbeat)
//!   and emits `game-state-changed` events.

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, PoisonError};
use std::time::{Duration, Instant};

use sqlx::SqlitePool;
use sysinfo::{
    Pid, ProcessRefreshKind, ProcessStatus, ProcessesToUpdate, Signal, System, UpdateKind,
};

use crate::db;
use crate::error::{AppError, AppResult};
use crate::models::{
    now_iso, CompatibilityTool, GameId, GameState, GameStateEvent, LaunchMethod, LaunchOptions,
    LaunchResult, LaunchVia, LibraryItem, Platform, RunningGame, StoreSource,
};
use crate::scanner::parse_desktop_entry;

// ───────────────────────── launch plan ─────────────────────────

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LaunchPlan {
    /// Hand over to a store client via its URL scheme.
    Url(String),
    Process {
        program: PathBuf,
        args: Vec<String>,
        env: Vec<(String, String)>,
        cwd: Option<PathBuf>,
    },
}

/// Facts about the machine needed to build a plan.
#[derive(Debug, Clone)]
pub struct HostEnv {
    pub platform: Platform,
    pub home: Option<PathBuf>,
    /// Per-game Proton prefixes live below this folder.
    pub compat_data_root: PathBuf,
    /// Proton builds on this machine (Cove's GE-Proton and Steam's).
    pub protons: Vec<crate::proton::ProtonBuild>,
    /// Build for games without a choice of their own (`library.defaultProton`).
    pub default_proton: String,
    /// Proton starts through umu-launcher (filled per launch); `None` = directly.
    pub umu: Option<crate::umu::UmuLaunch>,
    /// `mangohud`, `gamemoderun`, `gamescope` (looked up per launch).
    pub tools: ToolPaths,
    /// ROM folders: a file in one starts through its emulator (filled per launch).
    pub rom_folders: Vec<crate::models::RomFolder>,
    /// Where emulators are looked for.
    pub emulators: crate::emulators::Env,
}

impl HostEnv {
    pub fn detect(compat_data_root: PathBuf) -> Self {
        let home = std::env::var_os("HOME")
            .or_else(|| std::env::var_os("USERPROFILE"))
            .map(PathBuf::from);
        Self {
            platform: Platform::current(),
            home: home.clone(),
            compat_data_root,
            // Filled per launch by `ProtonManager::host`.
            protons: Vec::new(),
            default_proton: crate::proton::LATEST.to_owned(),
            umu: None,
            tools: ToolPaths::default(),
            rom_folders: Vec::new(),
            emulators: crate::emulators::Env::detect(home),
        }
    }

    pub fn steam_root(&self) -> Option<PathBuf> {
        let home = self.home.as_ref()?;
        steam_roots(home).into_iter().find(|p| p.is_dir())
    }
}

fn steam_roots(home: &Path) -> Vec<PathBuf> {
    vec![
        home.join(".steam/steam"),
        home.join(".local/share/Steam"),
        home.join(".var/app/com.valvesoftware.Steam/data/Steam"),
    ]
}

/// All Proton folders in the Steam library and `compatibilitytools.d`.
pub fn find_proton_installs(home: &Path) -> Vec<PathBuf> {
    let mut found = Vec::new();
    for root in steam_roots(home) {
        for dir in [
            root.join("steamapps/common"),
            root.join("compatibilitytools.d"),
        ] {
            let Ok(entries) = std::fs::read_dir(&dir) else {
                continue;
            };
            for entry in entries.flatten() {
                let path = entry.path();
                if path.join("proton").is_file() && !found.contains(&path) {
                    found.push(path);
                }
            }
        }
    }
    found
}

/// Version numbers in a name ("GE-Proton10-25" → [10, 25]).
pub fn version_key(name: &str) -> Vec<u32> {
    name.split(|c: char| !c.is_ascii_digit())
        .filter_map(|p| p.parse().ok())
        .collect()
}

/// Everything about a library entry needed to start it.
#[derive(Debug, Clone)]
pub struct LaunchTarget {
    pub game_id: GameId,
    pub source: StoreSource,
    pub store_game_id: Option<String>,
    pub executable_path: Option<String>,
    pub install_path: Option<String>,
    pub options: LaunchOptions,
    /// Installed by Cove (Epic: started directly with a login instead of the Epic Launcher).
    pub installed_by_cove: bool,
    /// Store login arguments (Epic exchange code), before the user's arguments.
    pub extra_args: Vec<String>,
    /// Store variables (Amazon Games SDK), before the user's variables.
    pub extra_env: Vec<(String, String)>,
}

impl From<&LibraryItem> for LaunchTarget {
    fn from(item: &LibraryItem) -> Self {
        Self {
            game_id: item.game.id.clone(),
            source: item.entry.source,
            store_game_id: item.entry.store_game_id.clone(),
            executable_path: item.entry.executable_path.clone(),
            install_path: item.entry.install_path.clone(),
            options: item.entry.launch_options.clone(),
            installed_by_cove: item.entry.installed_by_cove,
            extra_args: Vec::new(),
            extra_env: Vec::new(),
        }
    }
}

fn store_url(source: StoreSource, id: &str) -> Option<String> {
    match source {
        StoreSource::Steam => Some(format!("steam://rungameid/{id}")),
        StoreSource::Epic => Some(format!(
            "com.epicgames.launcher://apps/{id}?action=launch&silent=true"
        )),
        StoreSource::Gog => Some(format!("goggalaxy://openGameView/{id}")),
        // The Amazon Games app exists only on Windows.
        StoreSource::Amazon if cfg!(windows) => Some(format!("amazon-games://play/{id}")),
        StoreSource::Amazon | StoreSource::Manual => None,
    }
}

/// How [`plan`] will start a game (shown per game). Mirrors its decisions
/// without touching the file system; None = nothing to start yet.
pub fn launch_via(
    source: StoreSource,
    store_game_id: Option<&str>,
    executable_path: Option<&str>,
    installed_by_cove: bool,
) -> Option<LaunchVia> {
    if source == StoreSource::Steam && store_game_id.is_some() {
        return Some(LaunchVia::Steam);
    }
    if source == StoreSource::Epic && store_game_id.is_some() && !installed_by_cove {
        return Some(LaunchVia::EpicLauncher);
    }
    if source == StoreSource::Amazon
        && store_game_id.is_some()
        && !installed_by_cove
        && cfg!(windows)
    {
        return Some(LaunchVia::AmazonGames);
    }
    if executable_path.is_some_and(|p| !p.trim().is_empty()) {
        return Some(LaunchVia::Direct);
    }
    store_game_id?;
    match source {
        StoreSource::Epic => Some(LaunchVia::EpicLauncher),
        StoreSource::Gog => Some(LaunchVia::GogGalaxy),
        StoreSource::Amazon if cfg!(windows) => Some(LaunchVia::AmazonGames),
        StoreSource::Amazon | StoreSource::Steam | StoreSource::Manual => None,
    }
}

fn split_args(raw: &str) -> AppResult<Vec<String>> {
    shlex::split(raw).ok_or_else(|| {
        AppError::InvalidInput(tr!(
            "Startparameter: Anführungszeichen sind nicht geschlossen.",
            "Launch arguments: a quotation mark is not closed."
        ))
    })
}

/// Steam games, and Epic games from the Epic Launcher, start in their store client.
fn store_client_plan(target: &LaunchTarget) -> Option<LaunchPlan> {
    // Steam: DRM, overlay, cloud saves. Epic Launcher and Amazon Games app
    // installs need their login (the Amazon app only exists on Windows).
    let via_client = target.source == StoreSource::Steam
        || (target.source == StoreSource::Epic && !target.installed_by_cove)
        || (target.source == StoreSource::Amazon && !target.installed_by_cove && cfg!(windows));
    let id = target.store_game_id.as_deref().filter(|_| via_client)?;
    store_url(target.source, id).map(LaunchPlan::Url)
}

/// GOG games define their start in goggame-<id>.info: arguments and working folder.
fn gog_task(target: &LaunchTarget, exe: &Path) -> Option<(Vec<String>, Option<PathBuf>)> {
    let (StoreSource::Gog, Some(dir), Some(product)) = (
        target.source,
        target.install_path.as_deref(),
        target.store_game_id.as_deref(),
    ) else {
        return None;
    };
    crate::downloads::gog::primary_task(Path::new(dir), product)
        .filter(|t| t.path == exe)
        .map(|t| (t.arguments, t.working_dir))
}

/// Amazon games define their start in fuel.json: arguments and working folder.
fn amazon_task(target: &LaunchTarget, exe: &Path) -> Option<(Vec<String>, Option<PathBuf>)> {
    let (StoreSource::Amazon, Some(dir)) = (target.source, target.install_path.as_deref()) else {
        return None;
    };
    crate::downloads::amazon::fuel_start(Path::new(dir))
        .filter(|s| s.command == exe)
        .map(|s| (s.args, s.working_dir))
}

/// Decides how to start a game.
/// Linux game tools on this machine (looked up for every launch).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ToolPaths {
    pub mangohud: Option<PathBuf>,
    pub gamemode: Option<PathBuf>,
    pub gamescope: Option<PathBuf>,
}

impl ToolPaths {
    pub fn detect() -> Self {
        if Platform::current() != Platform::Linux {
            return Self::default();
        }
        Self {
            mangohud: crate::sidecar::which("mangohud"),
            gamemode: crate::sidecar::which("gamemoderun"),
            gamescope: crate::sidecar::which("gamescope"),
        }
    }

    pub fn installed(&self) -> crate::models::GameTools {
        crate::models::GameTools {
            mangohud: self.mangohud.is_some(),
            gamemode: self.gamemode.is_some(),
            gamescope: self.gamescope.is_some(),
        }
    }
}

fn tool_missing(name: &str) -> AppError {
    AppError::Launch(tr!(
        "{name} ist nicht installiert. Installiere es über die Paketverwaltung oder schalte es in den Einstellungen des Spiels aus.",
        "{name} is not installed. Install it with your package manager or switch it off in the game's settings."
    ))
}

/// The game's Linux options: Proton variables (DXVK, VKD3D, FSR) and the
/// wrappers around the command, outermost first:
/// `gamemoderun gamescope … -- mangohud <program> <args>`.
fn apply_tweaks(
    target: &LaunchTarget,
    host: &HostEnv,
    exe: &Path,
    program: PathBuf,
    args: Vec<String>,
    env: &mut Vec<(String, String)>,
) -> AppResult<(PathBuf, Vec<String>)> {
    let tweaks = &target.options.tweaks;
    if host.platform != Platform::Linux {
        return Ok((program, args));
    }
    let proton = target.options.compatibility_tool == CompatibilityTool::Proton
        && exe
            .extension()
            .is_some_and(|e| e.eq_ignore_ascii_case("exe"));
    if proton {
        if !tweaks.dxvk {
            env.push(("PROTON_USE_WINED3D".into(), "1".into()));
        }
        if !tweaks.vkd3d {
            env.push(("PROTON_NO_D3D12".into(), "1".into()));
        }
        if tweaks.fsr && !tweaks.gamescope {
            // GE-Proton scales a fullscreen game rendered below the screen's resolution.
            env.push(("WINE_FULLSCREEN_FSR".into(), "1".into()));
        }
    }
    let mut command: Vec<String> = Vec::new();
    let text = |p: &Path| p.to_string_lossy().into_owned();
    if tweaks.gamemode {
        let path = host
            .tools
            .gamemode
            .as_ref()
            .ok_or_else(|| tool_missing("GameMode"))?;
        command.push(text(path));
    }
    if tweaks.gamescope {
        let path = host
            .tools
            .gamescope
            .as_ref()
            .ok_or_else(|| tool_missing("gamescope"))?;
        command.push(text(path));
        if let (Some(width), Some(height)) = (tweaks.gamescope_width, tweaks.gamescope_height) {
            command.extend([
                "-w".into(),
                width.to_string(),
                "-h".into(),
                height.to_string(),
            ]);
        }
        if tweaks.gamescope_fullscreen {
            command.push("-f".into());
        }
        if tweaks.fsr {
            command.extend(["-F".into(), "fsr".into()]);
        }
        command.push("--".into());
    }
    if tweaks.mangohud {
        let path = host
            .tools
            .mangohud
            .as_ref()
            .ok_or_else(|| tool_missing("MangoHud"))?;
        command.push(text(path));
    }
    if command.is_empty() {
        return Ok((program, args));
    }
    command.push(text(&program));
    command.extend(args);
    let first = PathBuf::from(command.remove(0));
    Ok((first, command))
}

/// How a Windows game starts with Proton: through umu when it is there, else
/// Proton directly. Adds the environment both need.
fn proton_command(
    target: &LaunchTarget,
    host: &HostEnv,
    executable: &str,
    cwd: Option<&Path>,
    env: &mut Vec<(String, String)>,
) -> AppResult<(PathBuf, Vec<String>)> {
    let wanted = crate::proton::wanted(&target.options, &host.default_proton)
        .unwrap_or_else(|| host.default_proton.clone());
    let proton = crate::proton::resolve(&host.protons, &wanted).ok_or_else(|| {
        AppError::CompatToolNotFound(crate::proton::display_name(&wanted).to_owned())
    })?;
    let prefix = host.compat_data_root.join(&target.game_id);
    let text = |path: &Path| path.to_string_lossy().into_owned();
    if let Some(umu) = &host.umu {
        // umu runs Proton in the Steam Linux Runtime. Its prefix is the same
        // `pfx` folder Proton uses when it runs directly.
        env.push(("WINEPREFIX".into(), text(&prefix.join("pfx"))));
        env.push(("PROTONPATH".into(), text(&proton.path)));
        env.push(("GAMEID".into(), umu.game_id.clone()));
        if let Some(store) = umu.store {
            env.push(("STORE".into(), store.to_owned()));
        }
        return Ok((umu.program.clone(), vec![executable.to_owned()]));
    }
    env.push(("STEAM_COMPAT_DATA_PATH".into(), text(&prefix)));
    // Proton runs without Steam too; Steam's folder only adds its client libraries.
    let steam = host.steam_root().map(|p| text(&p));
    env.push((
        "STEAM_COMPAT_CLIENT_INSTALL_PATH".into(),
        steam.unwrap_or_default(),
    ));
    if let Some(dir) = cwd {
        env.push(("STEAM_COMPAT_INSTALL_PATH".into(), text(dir)));
    }
    // Like Steam: Proton waits for the game, so the session ends with it.
    Ok((
        proton.path.join("proton"),
        vec!["waitforexitandrun".into(), executable.to_owned()],
    ))
}

pub fn plan(target: &LaunchTarget, host: &HostEnv) -> AppResult<LaunchPlan> {
    if let Some(plan) = store_client_plan(target) {
        return Ok(plan);
    }

    let Some(executable) = target
        .executable_path
        .as_deref()
        .filter(|p| !p.trim().is_empty())
    else {
        if let Some(url) = target
            .store_game_id
            .as_deref()
            .and_then(|id| store_url(target.source, id))
        {
            return Ok(LaunchPlan::Url(url));
        }
        return Err(AppError::Launch(
            tr!("Für dieses Spiel ist noch keine ausführbare Datei festgelegt. Trage sie unter Einstellungen ein.", "No executable is set for this game yet. Enter it under Settings."),
        ));
    };

    let exe = PathBuf::from(executable);
    let user_args = split_args(&target.options.arguments)?;
    let mut env: Vec<(String, String)> = Vec::new();
    let mut cwd = exe.parent().map(Path::to_path_buf);
    let mut task_args = Vec::new();
    if let Some((args, working_dir)) = gog_task(target, &exe).or_else(|| amazon_task(target, &exe))
    {
        task_args = args;
        if working_dir.is_some() {
            cwd = working_dir;
        }
    }
    let has_ext = |ext: &str| exe.extension().is_some_and(|e| e.eq_ignore_ascii_case(ext));

    let (program, mut args) = if let Some(folder) =
        crate::emulators::folder_of(&host.rom_folders, &exe)
    {
        crate::emulators::command(folder, &exe, &host.emulators)?
    } else if has_ext("desktop") {
        let content = std::fs::read_to_string(&exe)?;
        let entry = parse_desktop_entry(&content).ok_or_else(|| {
            AppError::Launch(tr!(
                "Die .desktop-Datei enthält keinen gültigen Start-Eintrag.",
                "The .desktop file has no valid launch entry."
            ))
        })?;
        let mut parts = split_args(&entry.exec)?.into_iter();
        let program = parts.next().ok_or_else(|| {
            AppError::Launch(tr!("Exec-Zeile ist leer.", "The Exec line is empty."))
        })?;
        if let Some(dir) = entry.working_dir {
            cwd = Some(PathBuf::from(dir));
        }
        (PathBuf::from(program), parts.collect::<Vec<_>>())
    } else if has_ext("exe") && host.platform != Platform::Windows {
        match target.options.compatibility_tool {
            CompatibilityTool::Native => {
                return Err(AppError::Launch(
                    tr!("Das ist ein Windows-Spiel. Wähle in den Einstellungen des Spiels unter „Kompatibilität“ Proton oder Wine.", "This is a Windows game. Choose Proton or Wine under “Compatibility” in the game's settings."),
                ))
            }
            CompatibilityTool::Wine => (PathBuf::from("wine"), vec![executable.to_owned()]),
            CompatibilityTool::Proton => proton_command(target, host, executable, cwd.as_deref(), &mut env)?,
        }
    } else {
        (exe.clone(), Vec::new())
    };

    args.extend(task_args);
    args.extend(target.extra_args.iter().cloned());
    args.extend(user_args);
    let (program, args) = apply_tweaks(target, host, &exe, program, args, &mut env)?;
    env.extend(target.extra_env.iter().cloned());
    // User variables last so they can override ours.
    env.extend(
        target
            .options
            .env
            .iter()
            .filter(|e| !e.key.is_empty())
            .map(|e| (e.key.clone(), e.value.clone())),
    );
    Ok(LaunchPlan::Process {
        program,
        args,
        env,
        cwd,
    })
}

// ───────────────────────── process tracking ─────────────────────────

/// One row of a process snapshot.
#[derive(Debug, Clone)]
pub struct ProcInfo {
    pub pid: u32,
    pub parent: Option<u32>,
    pub start_time: u64,
    pub exe: Option<PathBuf>,
    /// Process group (Linux only) — games are spawned in their own group.
    pub pgid: Option<u32>,
    pub zombie: bool,
}

/// Follows the processes belonging to one game.
#[derive(Debug, Clone)]
pub struct Tracker {
    /// pid → start time (None until first seen; guards against pid reuse).
    known: HashMap<u32, Option<u64>>,
    /// Adopt every process whose executable lives below this folder.
    watch_dir: Option<PathBuf>,
    /// Adopt every process in this process group (catches children of a launcher
    /// that exited before the first poll).
    group: Option<u32>,
}

impl Tracker {
    pub fn for_pid(pid: u32) -> Self {
        Self {
            known: HashMap::from([(pid, None)]),
            watch_dir: None,
            group: Some(pid),
        }
    }

    pub fn for_dir(dir: PathBuf) -> Self {
        Self {
            known: HashMap::new(),
            watch_dir: Some(dir),
            group: None,
        }
    }

    pub fn update(&mut self, procs: &[ProcInfo]) {
        let alive: HashMap<u32, &ProcInfo> = procs
            .iter()
            .filter(|p| !p.zombie)
            .map(|p| (p.pid, p))
            .collect();
        self.known.retain(|pid, start| match alive.get(pid) {
            Some(p) => {
                let same = start.is_none_or(|s| s == p.start_time);
                *start = Some(p.start_time);
                same
            }
            None => false,
        });
        if let Some(group) = self.group {
            for p in alive.values() {
                if p.pgid == Some(group) {
                    self.known.entry(p.pid).or_insert(Some(p.start_time));
                }
            }
        }
        if let Some(dir) = &self.watch_dir {
            for p in alive.values() {
                if p.exe.as_deref().is_some_and(|exe| exe.starts_with(dir)) {
                    self.known.entry(p.pid).or_insert(Some(p.start_time));
                }
            }
        }
        // Adopt descendants until nothing changes (handles deep trees in any order).
        loop {
            let parents: HashSet<u32> = self.known.keys().copied().collect();
            let children: Vec<&ProcInfo> = alive
                .values()
                .filter(|p| {
                    !parents.contains(&p.pid) && p.parent.is_some_and(|pp| parents.contains(&pp))
                })
                .copied()
                .collect();
            if children.is_empty() {
                break;
            }
            for c in children {
                self.known.insert(c.pid, Some(c.start_time));
            }
        }
    }

    /// Whether it adopts processes by their folder (games a store client starts).
    pub fn watches_dir(&self) -> bool {
        self.watch_dir.is_some()
    }

    pub fn is_alive(&self) -> bool {
        !self.known.is_empty()
    }

    pub fn pids(&self) -> Vec<u32> {
        self.known.keys().copied().collect()
    }
}

fn snapshot(system: &mut System) -> Vec<ProcInfo> {
    system.refresh_processes_specifics(
        ProcessesToUpdate::All,
        true,
        ProcessRefreshKind::nothing().with_exe(UpdateKind::OnlyIfNotSet),
    );
    system
        .processes()
        .values()
        .map(|p| ProcInfo {
            pid: p.pid().as_u32(),
            parent: p.parent().map(Pid::as_u32),
            start_time: p.start_time(),
            exe: p.exe().map(Path::to_path_buf),
            pgid: process_group_of(p.pid().as_u32()),
            zombie: matches!(p.status(), ProcessStatus::Zombie | ProcessStatus::Dead),
        })
        .collect()
}

/// Reads the process group id from `/proc/<pid>/stat` (field 5, after the command name).
#[cfg(target_os = "linux")]
fn process_group_of(pid: u32) -> Option<u32> {
    let stat = std::fs::read_to_string(format!("/proc/{pid}/stat")).ok()?;
    let after_comm = stat.rsplit_once(')')?.1;
    after_comm.split_whitespace().nth(2)?.parse().ok()
}

#[cfg(not(target_os = "linux"))]
fn process_group_of(_pid: u32) -> Option<u32> {
    None
}

// ───────────────────────── launcher ─────────────────────────

pub type EmitFn = Arc<dyn Fn(GameStateEvent) + Send + Sync>;
pub type OpenUrlFn = Arc<dyn Fn(&str) -> AppResult<()> + Send + Sync>;

#[derive(Debug, Clone, Copy)]
pub struct Timing {
    pub fast_poll: Duration,
    pub slow_poll: Duration,
    /// Poll fast during this period after launch (catches short-lived launchers).
    pub fast_period: Duration,
    pub heartbeat: Duration,
    /// Store-client launches: give up if no game process appears within this time.
    pub adopt_timeout: Duration,
}

impl Default for Timing {
    fn default() -> Self {
        Self {
            fast_poll: Duration::from_millis(250),
            slow_poll: Duration::from_secs(2),
            fast_period: Duration::from_secs(15),
            heartbeat: Duration::from_secs(30),
            adopt_timeout: Duration::from_secs(120),
        }
    }
}

#[derive(Debug)]
struct Running {
    started_at: Option<String>,
    stop: Arc<AtomicBool>,
}

#[derive(Clone)]
pub struct Launcher {
    running: Arc<Mutex<HashMap<GameId, Running>>>,
    pool: SqlitePool,
    emit: EmitFn,
    open_url: OpenUrlFn,
    timing: Timing,
}

impl Launcher {
    pub fn new(pool: SqlitePool, emit: EmitFn, open_url: OpenUrlFn, timing: Timing) -> Self {
        Self {
            running: Arc::default(),
            pool,
            emit,
            open_url,
            timing,
        }
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, HashMap<GameId, Running>> {
        self.running.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// Games with an active session.
    pub fn running(&self) -> Vec<RunningGame> {
        self.lock()
            .iter()
            .filter_map(|(id, r)| {
                Some(RunningGame {
                    game_id: id.clone(),
                    started_at: r.started_at.clone()?,
                })
            })
            .collect()
    }

    pub fn launch(&self, item: &LibraryItem, host: &HostEnv) -> AppResult<LaunchResult> {
        self.launch_with(item, host, Vec::new(), Vec::new())
    }

    /// Starts with extra store arguments (Epic login) and variables (Amazon SDK).
    pub fn launch_with(
        &self,
        item: &LibraryItem,
        host: &HostEnv,
        extra_args: Vec<String>,
        extra_env: Vec<(String, String)>,
    ) -> AppResult<LaunchResult> {
        let game_id = item.game.id.clone();
        if self.lock().contains_key(&game_id) {
            return Err(AppError::AlreadyRunning(game_id));
        }
        let target = LaunchTarget {
            extra_args,
            extra_env,
            ..LaunchTarget::from(item)
        };
        let plan = plan(&target, host)?;

        match plan {
            LaunchPlan::Url(url) => {
                (self.open_url)(&url)?;
                let Some(dir) = target.install_path.filter(|p| Path::new(p).is_dir()) else {
                    return Ok(LaunchResult {
                        game_id,
                        method: LaunchMethod::Url,
                        tracked: false,
                    });
                };
                self.start_monitor(game_id.clone(), Tracker::for_dir(PathBuf::from(dir)), false);
                Ok(LaunchResult {
                    game_id,
                    method: LaunchMethod::Url,
                    tracked: true,
                })
            }
            LaunchPlan::Process {
                program,
                args,
                env,
                cwd,
            } => {
                if program.is_absolute() && !program.exists() {
                    return Err(AppError::Launch(tr!(
                        "Datei nicht gefunden: {}",
                        "File not found: {}",
                        program.display()
                    )));
                }
                if let Some((_, prefix)) = env
                    .iter()
                    .find(|(k, _)| k == "STEAM_COMPAT_DATA_PATH" || k == "WINEPREFIX")
                {
                    std::fs::create_dir_all(prefix)?;
                }
                // In Flatpak, host programs (emulators, …) run on the host.
                let (program, args, env) = if crate::flatpak::active() {
                    let routed = crate::flatpak::route(&program, &args, &env, cwd.as_deref());
                    (routed.program, routed.args, routed.env)
                } else {
                    (program, args, env)
                };
                let mut command = Command::new(&program);
                command
                    .args(&args)
                    .envs(env)
                    .stdin(Stdio::null())
                    .stdout(Stdio::null())
                    .stderr(Stdio::null());
                if let Some(dir) = cwd.filter(|d| d.is_dir()) {
                    command.current_dir(dir);
                }
                #[cfg(unix)]
                {
                    use std::os::unix::process::CommandExt;
                    command.process_group(0);
                }
                let mut child = command.spawn().map_err(|e| {
                    AppError::Launch(tr!(
                        "{} konnte nicht gestartet werden: {e}",
                        "{} could not be started: {e}",
                        program.display()
                    ))
                })?;
                let pid = child.id();
                // Reap the direct child so it does not linger as a zombie.
                std::thread::spawn(move || {
                    let _ = child.wait();
                });
                self.start_monitor(game_id.clone(), Tracker::for_pid(pid), true);
                Ok(LaunchResult {
                    game_id,
                    method: LaunchMethod::Process,
                    tracked: true,
                })
            }
        }
    }

    /// Requests termination of a running game; the monitor records the session.
    pub fn stop(&self, game_id: &str) -> AppResult<()> {
        let guard = self.lock();
        let running = guard
            .get(game_id)
            .ok_or_else(|| AppError::NotRunning(game_id.to_owned()))?;
        running.stop.store(true, Ordering::SeqCst);
        Ok(())
    }

    fn start_monitor(&self, game_id: GameId, tracker: Tracker, started: bool) {
        let stop = Arc::new(AtomicBool::new(false));
        let started_at = started.then(now_iso);
        self.lock().insert(
            game_id.clone(),
            Running {
                started_at: started_at.clone(),
                stop: stop.clone(),
            },
        );
        let this = self.clone();
        tauri::async_runtime::spawn(async move {
            if let Err(error) = this.monitor(&game_id, tracker, started_at, &stop).await {
                eprintln!("[cove] Überwachung von {game_id} fehlgeschlagen: {error}");
            }
            this.lock().remove(&game_id);
        });
    }

    async fn begin_session(&self, game_id: &str, started_at: &str) -> AppResult<i64> {
        let session = db::sessions::start(&self.pool, game_id, started_at).await?;
        if let Some(r) = self.lock().get_mut(game_id) {
            r.started_at = Some(started_at.to_owned());
        }
        (self.emit)(GameStateEvent {
            game_id: game_id.to_owned(),
            state: GameState::Running,
            started_at: started_at.to_owned(),
            session_seconds: None,
        });
        Ok(session)
    }

    async fn monitor(
        &self,
        game_id: &str,
        mut tracker: Tracker,
        started_at: Option<String>,
        stop: &AtomicBool,
    ) -> AppResult<()> {
        let launched = Instant::now();
        let mut system = System::new();
        let mut session: Option<(i64, String, Instant)> = None;
        if let Some(at) = started_at {
            session = Some((self.begin_session(game_id, &at).await?, at, Instant::now()));
        }
        let mut last_heartbeat = Instant::now();
        let mut kill_requested_at: Option<Instant> = None;

        loop {
            let poll = if launched.elapsed() < self.timing.fast_period {
                self.timing.fast_poll
            } else {
                self.timing.slow_poll
            };
            tokio::time::sleep(poll).await;
            // Games a store client started run outside a Flatpak sandbox.
            let on_host = crate::flatpak::active() && tracker.watches_dir();
            let procs = if on_host {
                tokio::task::spawn_blocking(crate::flatpak::host_processes)
                    .await
                    .unwrap_or_default()
            } else {
                snapshot(&mut system)
            };
            tracker.update(&procs);

            if stop.load(Ordering::SeqCst) {
                let first = *kill_requested_at.get_or_insert_with(Instant::now);
                // Polite SIGTERM first, SIGKILL after 3 s.
                let force = first.elapsed() > Duration::from_secs(3);
                if on_host {
                    crate::flatpak::kill_on_host(
                        &tracker.pids(),
                        if force { "KILL" } else { "TERM" },
                    );
                }
                for pid in tracker.pids().into_iter().filter(|_| !on_host) {
                    if let Some(p) = system.process(Pid::from_u32(pid)) {
                        if force || p.kill_with(Signal::Term).is_none() {
                            p.kill();
                        }
                    }
                }
            }

            match (&session, tracker.is_alive()) {
                (None, true) => {
                    let at = now_iso();
                    session = Some((self.begin_session(game_id, &at).await?, at, Instant::now()));
                }
                (None, false) => {
                    if launched.elapsed() > self.timing.adopt_timeout || stop.load(Ordering::SeqCst)
                    {
                        return Ok(());
                    }
                }
                (Some((id, _, since)), true) => {
                    if last_heartbeat.elapsed() >= self.timing.heartbeat {
                        let secs = i64::try_from(since.elapsed().as_secs()).unwrap_or(i64::MAX);
                        db::sessions::update(&self.pool, *id, &now_iso(), secs).await?;
                        last_heartbeat = Instant::now();
                    }
                }
                (Some((id, at, since)), false) => {
                    let secs = i64::try_from(since.elapsed().as_secs()).unwrap_or(i64::MAX);
                    db::sessions::update(&self.pool, *id, &now_iso(), secs).await?;
                    (self.emit)(GameStateEvent {
                        game_id: game_id.to_owned(),
                        state: GameState::Stopped,
                        started_at: at.clone(),
                        session_seconds: Some(secs),
                    });
                    return Ok(());
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::EnvVar;

    /// `proton`: build ids; each lives in `/p/<id without "steam:">`.
    fn host(platform: Platform, proton: &[&str]) -> HostEnv {
        HostEnv {
            platform,
            home: None,
            compat_data_root: PathBuf::from("/data/compat"),
            protons: proton
                .iter()
                .map(|id| crate::proton::ProtonBuild {
                    id: (*id).to_owned(),
                    path: PathBuf::from("/p").join(crate::proton::display_name(id)),
                })
                .collect(),
            default_proton: crate::proton::LATEST.to_owned(),
            umu: None,
            tools: ToolPaths::default(),
            rom_folders: Vec::new(),
            emulators: crate::emulators::Env {
                platform,
                home: None,
                path: Vec::new(),
                program_files: Vec::new(),
            },
        }
    }

    fn target(source: StoreSource, exe: Option<&str>, store_id: Option<&str>) -> LaunchTarget {
        LaunchTarget {
            game_id: "g".into(),
            source,
            store_game_id: store_id.map(Into::into),
            executable_path: exe.map(Into::into),
            install_path: None,
            options: LaunchOptions::default(),
            installed_by_cove: false,
            extra_args: Vec::new(),
            extra_env: Vec::new(),
        }
    }

    #[test]
    fn amazon_games_start_from_fuel_json_with_the_sdk_variables() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join("Bin")).unwrap();
        std::fs::write(dir.path().join("Bin/game.sh"), "#!/bin/sh").unwrap();
        std::fs::write(
            dir.path().join("fuel.json"),
            r#"{"Main":{"Command":"Bin\\game.sh","Args":["-fuel"],"WorkingSubdirOverride":"Bin"}}"#,
        )
        .unwrap();
        let exe = dir.path().join("Bin/game.sh");
        let mut t = target(
            StoreSource::Amazon,
            Some(&exe.to_string_lossy()),
            Some("amzn1.adg.product.1"),
        );
        t.install_path = Some(dir.path().to_string_lossy().into_owned());
        t.installed_by_cove = true;
        t.extra_env = vec![("AMAZON_GAMES_FUEL_PRODUCT_SKU".into(), "SKU1".into())];
        let LaunchPlan::Process { args, env, cwd, .. } =
            plan(&t, &host(Platform::Linux, &[])).unwrap()
        else {
            panic!("expected a process");
        };
        assert_eq!(args, ["-fuel"]);
        assert_eq!(cwd, Some(dir.path().join("Bin")));
        assert!(env.contains(&("AMAZON_GAMES_FUEL_PRODUCT_SKU".into(), "SKU1".into())));
        assert_eq!(
            launch_via(StoreSource::Amazon, Some("p"), Some("/g/x.exe"), true),
            Some(LaunchVia::Direct)
        );
        let none = if cfg!(windows) {
            Some(LaunchVia::AmazonGames)
        } else {
            None
        };
        assert_eq!(
            launch_via(StoreSource::Amazon, Some("p"), None, false),
            none
        );
        // Installed by the Amazon Games app (Windows) or Heroic (Linux): the app
        // starts it on Windows, Cove itself (fuel.json) elsewhere.
        let app_or_direct = if cfg!(windows) {
            LaunchVia::AmazonGames
        } else {
            LaunchVia::Direct
        };
        assert_eq!(
            launch_via(StoreSource::Amazon, Some("p"), Some("/g/x.exe"), false),
            Some(app_or_direct)
        );
        t.installed_by_cove = false;
        let plan = plan(&t, &host(Platform::Linux, &[])).unwrap();
        if cfg!(windows) {
            assert_eq!(
                plan,
                LaunchPlan::Url("amazon-games://play/amzn1.adg.product.1".into())
            );
        } else {
            assert!(matches!(plan, LaunchPlan::Process { ref args, .. } if args == &["-fuel"]));
        }
    }

    #[test]
    fn store_url_schemes() {
        let linux = host(Platform::Linux, &[]);
        assert_eq!(
            plan(
                &target(StoreSource::Steam, Some("/x/game"), Some("620")),
                &linux
            )
            .unwrap(),
            LaunchPlan::Url("steam://rungameid/620".into())
        );
        assert_eq!(
            plan(&target(StoreSource::Epic, None, Some("Fortnite")), &linux).unwrap(),
            LaunchPlan::Url(
                "com.epicgames.launcher://apps/Fortnite?action=launch&silent=true".into()
            )
        );
        assert_eq!(
            plan(&target(StoreSource::Gog, None, Some("1207658924")), &linux).unwrap(),
            LaunchPlan::Url("goggalaxy://openGameView/1207658924".into())
        );
        // GOG game with a local executable starts directly
        assert!(matches!(
            plan(
                &target(StoreSource::Gog, Some("/g/start.sh"), Some("1")),
                &linux
            )
            .unwrap(),
            LaunchPlan::Process { .. }
        ));
        assert!(matches!(
            plan(&target(StoreSource::Manual, None, None), &linux),
            Err(AppError::Launch(_))
        ));
    }

    #[test]
    fn gog_games_start_with_their_play_task() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(
            dir.path().join("goggame-100.info"),
            r#"{ "playTasks": [ { "isPrimary": true, "type": "FileTask", "path": "bin\\game.exe",
                 "arguments": "-lang de", "workingDir": "bin" } ] }"#,
        )
        .unwrap();
        let exe = dir.path().join("bin").join("game.exe");
        let mut t = target(StoreSource::Gog, Some(&exe.to_string_lossy()), Some("100"));
        t.install_path = Some(dir.path().to_string_lossy().into_owned());
        t.options.arguments = "-windowed".into();
        t.options.compatibility_tool = CompatibilityTool::Wine;
        let LaunchPlan::Process { args, cwd, .. } = plan(&t, &host(Platform::Linux, &[])).unwrap()
        else {
            panic!("expected process")
        };
        assert_eq!(
            args,
            [
                exe.to_string_lossy().into_owned(),
                "-lang".into(),
                "de".into(),
                "-windowed".into()
            ]
        );
        assert_eq!(cwd, Some(dir.path().join("bin")));
    }

    #[test]
    fn launch_via_follows_the_plan() {
        let linux = host(Platform::Linux, &[]);
        let cases = [
            (
                target(StoreSource::Steam, Some("/x/game"), Some("620")),
                Some(LaunchVia::Steam),
            ),
            (
                target(StoreSource::Epic, None, Some("Sugar")),
                Some(LaunchVia::EpicLauncher),
            ),
            (
                target(StoreSource::Gog, None, Some("1")),
                Some(LaunchVia::GogGalaxy),
            ),
            (
                target(StoreSource::Gog, Some("/g/start.sh"), Some("1")),
                Some(LaunchVia::Direct),
            ),
            (
                target(StoreSource::Manual, Some("/g/run"), None),
                Some(LaunchVia::Direct),
            ),
            (target(StoreSource::Manual, None, None), None),
        ];
        for (t, expected) in cases {
            let via = launch_via(
                t.source,
                t.store_game_id.as_deref(),
                t.executable_path.as_deref(),
                t.installed_by_cove,
            );
            assert_eq!(via, expected, "{t:?}");
            let planned = plan(&t, &linux).ok();
            assert_eq!(
                matches!(planned, Some(LaunchPlan::Process { .. })),
                via == Some(LaunchVia::Direct),
                "{t:?}"
            );
        }
    }

    #[test]
    fn native_process_with_args_and_env() {
        let mut t = target(StoreSource::Manual, Some("/games/tide/tide.x86_64"), None);
        t.options.arguments = "-fullscreen --name \"Ada Lovelace\"".into();
        t.options.env = vec![EnvVar {
            key: "DXVK_HUD".into(),
            value: "fps".into(),
        }];
        let LaunchPlan::Process {
            program,
            args,
            env,
            cwd,
        } = plan(&t, &host(Platform::Linux, &[])).unwrap()
        else {
            panic!("expected process")
        };
        assert_eq!(program, PathBuf::from("/games/tide/tide.x86_64"));
        assert_eq!(args, ["-fullscreen", "--name", "Ada Lovelace"]);
        assert_eq!(env, [("DXVK_HUD".to_owned(), "fps".to_owned())]);
        assert_eq!(cwd, Some(PathBuf::from("/games/tide")));

        t.options.arguments = "\"unclosed".into();
        assert!(matches!(
            plan(&t, &host(Platform::Linux, &[])),
            Err(AppError::InvalidInput(_))
        ));
    }

    #[test]
    fn windows_exe_on_linux_needs_compat_tool() {
        let mut t = target(StoreSource::Manual, Some("/g/Game.exe"), None);
        assert!(matches!(
            plan(&t, &host(Platform::Linux, &[])),
            Err(AppError::Launch(_))
        ));
        // …but runs directly on Windows
        assert!(matches!(
            plan(&t, &host(Platform::Windows, &[])).unwrap(),
            LaunchPlan::Process { .. }
        ));

        t.options.compatibility_tool = CompatibilityTool::Wine;
        let LaunchPlan::Process { program, args, .. } =
            plan(&t, &host(Platform::Linux, &[])).unwrap()
        else {
            panic!()
        };
        assert_eq!(program, PathBuf::from("wine"));
        assert_eq!(args, ["/g/Game.exe"]);

        t.options.compatibility_tool = CompatibilityTool::Proton;
        assert!(matches!(
            plan(&t, &host(Platform::Linux, &[])),
            Err(AppError::CompatToolNotFound(_))
        ));
        let protons = ["GE-Proton9-27", "GE-Proton10-25", "steam:Proton 9.0"];
        // Without a choice of its own the game uses the default: the newest GE-Proton.
        let LaunchPlan::Process {
            program, args, env, ..
        } = plan(&t, &host(Platform::Linux, &protons)).unwrap()
        else {
            panic!()
        };
        assert_eq!(program, PathBuf::from("/p/GE-Proton10-25/proton"));
        assert_eq!(args, ["waitforexitandrun", "/g/Game.exe"]);
        let var = |key: &str| env.iter().find(|(k, _)| k == key).map(|(_, v)| v.clone());
        assert_eq!(
            var("STEAM_COMPAT_DATA_PATH"),
            // Built with the host's separator (this test also runs on Windows).
            Some(
                PathBuf::from("/data/compat")
                    .join("g")
                    .to_string_lossy()
                    .into_owned()
            )
        );
        // No Steam needed.
        assert_eq!(var("STEAM_COMPAT_CLIENT_INSTALL_PATH").as_deref(), Some(""));
        assert_eq!(var("STEAM_COMPAT_INSTALL_PATH").as_deref(), Some("/g"));

        // A build of its own: a pinned GE-Proton or one from Steam.
        for (version, expected) in [
            ("GE-Proton9-27", "/p/GE-Proton9-27/proton"),
            ("steam:Proton 9.0", "/p/Proton 9.0/proton"),
        ] {
            t.options.proton_version = Some(version.into());
            let LaunchPlan::Process { program, .. } =
                plan(&t, &host(Platform::Linux, &protons)).unwrap()
            else {
                panic!()
            };
            assert_eq!(program, PathBuf::from(expected));
        }
        t.options.proton_version = Some("steam:Proton - Experimental".into());
        assert!(matches!(
            plan(&t, &host(Platform::Linux, &protons)),
            Err(AppError::CompatToolNotFound(name)) if name == "Proton - Experimental"
        ));
    }

    #[test]
    fn game_options_wrap_the_command_and_set_proton_variables() {
        let mut t = target(StoreSource::Manual, Some("/g/Game.exe"), None);
        t.options.compatibility_tool = CompatibilityTool::Proton;
        t.options.tweaks = crate::models::GameTweaks {
            mangohud: true,
            gamemode: true,
            gamescope: true,
            gamescope_width: Some(1280),
            gamescope_height: Some(720),
            gamescope_fullscreen: true,
            fsr: true,
            dxvk: false,
            vkd3d: false,
        };
        let mut host = host(Platform::Linux, &["GE-Proton10-25"]);
        // Nothing installed: say which tool is missing instead of starting.
        assert!(matches!(
            plan(&t, &host),
            Err(AppError::Launch(message)) if message.contains("GameMode")
        ));
        host.tools = ToolPaths {
            mangohud: Some(PathBuf::from("/usr/bin/mangohud")),
            gamemode: Some(PathBuf::from("/usr/bin/gamemoderun")),
            gamescope: Some(PathBuf::from("/usr/bin/gamescope")),
        };
        let LaunchPlan::Process {
            program, args, env, ..
        } = plan(&t, &host).unwrap()
        else {
            panic!()
        };
        assert_eq!(program, PathBuf::from("/usr/bin/gamemoderun"));
        let proton = PathBuf::from("/p")
            .join("GE-Proton10-25")
            .join("proton")
            .to_string_lossy()
            .into_owned();
        assert_eq!(
            args,
            [
                "/usr/bin/gamescope",
                "-w",
                "1280",
                "-h",
                "720",
                "-f",
                "-F",
                "fsr",
                "--",
                "/usr/bin/mangohud",
                &proton,
                "waitforexitandrun",
                "/g/Game.exe"
            ]
        );
        let var = |key: &str| env.iter().find(|(k, _)| k == key).map(|(_, v)| v.as_str());
        assert_eq!(var("PROTON_USE_WINED3D"), Some("1"));
        assert_eq!(var("PROTON_NO_D3D12"), Some("1"));
        // gamescope scales: no Proton FSR on top.
        assert_eq!(var("WINE_FULLSCREEN_FSR"), None);

        // Without gamescope GE-Proton's FSR takes over; a native game gets no Proton variables.
        t.options.tweaks = crate::models::GameTweaks {
            fsr: true,
            ..crate::models::GameTweaks::default()
        };
        let LaunchPlan::Process { env, .. } = plan(&t, &host).unwrap() else {
            panic!()
        };
        assert!(env
            .iter()
            .any(|(k, v)| k == "WINE_FULLSCREEN_FSR" && v == "1"));
        let mut native = target(StoreSource::Manual, Some("/g/game.sh"), None);
        native.options.tweaks = crate::models::GameTweaks {
            mangohud: true,
            dxvk: false,
            ..crate::models::GameTweaks::default()
        };
        let LaunchPlan::Process {
            program, args, env, ..
        } = plan(&native, &host).unwrap()
        else {
            panic!()
        };
        assert_eq!(program, PathBuf::from("/usr/bin/mangohud"));
        assert_eq!(args, ["/g/game.sh"]);
        assert_eq!(env, Vec::<(String, String)>::new());
        // Not on Windows.
        assert!(matches!(
            plan(&native, &self::host(Platform::Windows, &[])).unwrap(),
            LaunchPlan::Process { program, .. } if program == Path::new("/g/game.sh")
        ));
    }

    #[test]
    fn roms_start_through_their_emulator_with_the_game_options() {
        let root = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(root.path().join("bin")).unwrap();
        std::fs::write(root.path().join("bin/dolphin-emu"), b"").unwrap();
        let roms = root.path().join("roms/gc");
        let rom = roms.join("Game.rvz");
        let mut t = target(StoreSource::Manual, Some(&rom.to_string_lossy()), None);
        t.options.tweaks.mangohud = true;
        let mut host = host(Platform::Linux, &[]);
        host.tools.mangohud = Some(PathBuf::from("/usr/bin/mangohud"));
        host.rom_folders = vec![crate::models::RomFolder {
            id: "gc".into(),
            system: "gamecube".into(),
            path: roms.to_string_lossy().into_owned(),
            emulator: "dolphin".into(),
            program: None,
            arguments: String::new(),
        }];
        host.emulators.path = vec![root.path().join("bin")];
        let LaunchPlan::Process { program, args, .. } = plan(&t, &host).unwrap() else {
            panic!()
        };
        assert_eq!(program, PathBuf::from("/usr/bin/mangohud"));
        let dolphin = root
            .path()
            .join("bin")
            .join("dolphin-emu")
            .to_string_lossy()
            .into_owned();
        assert_eq!(args, [dolphin.as_str(), "-b", "-e", &rom.to_string_lossy()]);
    }

    #[test]
    fn proton_runs_through_umu_when_it_is_there() {
        let mut t = target(StoreSource::Gog, Some("/g/Game.exe"), None);
        t.options.compatibility_tool = CompatibilityTool::Proton;
        t.options.arguments = "-windowed".into();
        let mut host = host(Platform::Linux, &["GE-Proton10-25"]);
        host.umu = Some(crate::umu::UmuLaunch::new(
            PathBuf::from("/u/umu-run"),
            StoreSource::Gog,
            Some("1091500"),
        ));
        let LaunchPlan::Process {
            program, args, env, ..
        } = plan(&t, &host).unwrap()
        else {
            panic!()
        };
        assert_eq!(program, PathBuf::from("/u/umu-run"));
        assert_eq!(args, ["/g/Game.exe", "-windowed"]);
        let var = |key: &str| env.iter().find(|(k, _)| k == key).map(|(_, v)| v.clone());
        // The same wine prefix as without umu, so saves and installs stay.
        let prefix = PathBuf::from("/data/compat").join("g").join("pfx");
        assert_eq!(
            var("WINEPREFIX"),
            Some(prefix.to_string_lossy().into_owned())
        );
        let proton = PathBuf::from("/p").join("GE-Proton10-25");
        assert_eq!(
            var("PROTONPATH"),
            Some(proton.to_string_lossy().into_owned())
        );
        assert_eq!(var("GAMEID").as_deref(), Some("umu-1091500"));
        assert_eq!(var("STORE").as_deref(), Some("gog"));
        assert_eq!(var("STEAM_COMPAT_DATA_PATH"), None);
    }

    #[test]
    fn desktop_entry_launch() {
        let tmp = tempfile::tempdir().unwrap();
        let file = tmp.path().join("orbit.desktop");
        std::fs::write(&file, "[Desktop Entry]\nType=Application\nName=Orbit\nExec=/opt/orbit/run --safe %U\nPath=/opt/orbit\n")
            .unwrap();
        let mut t = target(StoreSource::Manual, Some(file.to_str().unwrap()), None);
        t.options.arguments = "-x".into();
        let LaunchPlan::Process {
            program, args, cwd, ..
        } = plan(&t, &host(Platform::Linux, &[])).unwrap()
        else {
            panic!()
        };
        assert_eq!(program, PathBuf::from("/opt/orbit/run"));
        assert_eq!(args, ["--safe", "-x"]);
        assert_eq!(cwd, Some(PathBuf::from("/opt/orbit")));
    }

    fn proc(pid: u32, parent: Option<u32>, start: u64) -> ProcInfo {
        ProcInfo {
            pid,
            parent,
            start_time: start,
            exe: None,
            pgid: None,
            zombie: false,
        }
    }

    #[test]
    fn tracker_follows_descendants_after_launcher_exits() {
        let mut t = Tracker::for_pid(100);
        t.update(&[
            proc(1, None, 0),
            proc(100, Some(1), 10),
            proc(101, Some(100), 11),
            proc(102, Some(101), 12),
        ]);
        let mut pids = t.pids();
        pids.sort_unstable();
        assert_eq!(pids, [100, 101, 102]);

        // Launcher (100) exits; grandchild got reparented to init but is still tracked.
        t.update(&[proc(1, None, 0), proc(102, Some(1), 12)]);
        assert_eq!(t.pids(), [102]);
        assert!(t.is_alive());

        t.update(&[proc(1, None, 0)]);
        assert!(!t.is_alive());
    }

    #[test]
    fn tracker_adopts_its_process_group_after_root_died() {
        let mut t = Tracker::for_pid(100);
        // Root already gone at first poll; its child (reparented to 1) is still in group 100.
        t.update(&[
            proc(1, None, 0),
            ProcInfo {
                pgid: Some(100),
                ..proc(101, Some(1), 11)
            },
            proc(7, Some(1), 3),
        ]);
        assert_eq!(t.pids(), [101]);
    }

    #[test]
    fn tracker_detects_pid_reuse_and_zombies() {
        let mut t = Tracker::for_pid(100);
        t.update(&[proc(100, None, 10)]);
        t.update(&[proc(100, None, 99)]); // same pid, different process
        assert!(!t.is_alive());

        let mut t = Tracker::for_pid(5);
        t.update(&[ProcInfo {
            zombie: true,
            ..proc(5, None, 1)
        }]);
        assert!(!t.is_alive());
    }

    #[test]
    fn tracker_adopts_processes_from_install_dir() {
        let mut t = Tracker::for_dir(PathBuf::from("/games/tide"));
        t.update(&[proc(1, None, 0)]);
        assert!(!t.is_alive());
        let game = ProcInfo {
            exe: Some(PathBuf::from("/games/tide/bin/tide")),
            ..proc(50, Some(1), 5)
        };
        let other = ProcInfo {
            exe: Some(PathBuf::from("/usr/bin/steam")),
            ..proc(40, Some(1), 4)
        };
        t.update(&[proc(1, None, 0), other, game, proc(51, Some(50), 6)]);
        let mut pids = t.pids();
        pids.sort_unstable();
        assert_eq!(pids, [50, 51]);
    }

    #[cfg(unix)]
    mod process {
        use super::super::*;
        use crate::db::library::add_manual;
        use crate::models::ManualGameInput;
        use std::os::unix::fs::PermissionsExt;

        fn fast() -> Timing {
            Timing {
                fast_poll: Duration::from_millis(50),
                slow_poll: Duration::from_millis(50),
                fast_period: Duration::from_secs(1),
                heartbeat: Duration::from_millis(200),
                adopt_timeout: Duration::from_secs(2),
            }
        }

        async fn setup(
            script: &str,
        ) -> (
            tempfile::TempDir,
            Launcher,
            LibraryItem,
            Arc<Mutex<Vec<GameStateEvent>>>,
        ) {
            let tmp = tempfile::tempdir().unwrap();
            let path = tmp.path().join("game.sh");
            std::fs::write(&path, script).unwrap();
            std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).unwrap();
            let pool = crate::db::open_in_memory().await.unwrap();
            let input = ManualGameInput {
                title: "Script".into(),
                executable_path: path.to_string_lossy().into_owned(),
                size_bytes: None,
            };
            let item = add_manual(&pool, &input).await.unwrap();
            let events = Arc::new(Mutex::new(Vec::new()));
            let sink = events.clone();
            let launcher = Launcher::new(
                pool,
                Arc::new(move |e| sink.lock().unwrap().push(e)),
                Arc::new(|_| Ok(())),
                fast(),
            );
            (tmp, launcher, item, events)
        }

        async fn wait_until_stopped(launcher: &Launcher, id: &str) {
            for _ in 0..200 {
                if !launcher.lock().contains_key(id) {
                    return;
                }
                tokio::time::sleep(Duration::from_millis(50)).await;
            }
            panic!("game did not stop");
        }

        #[tokio::test(flavor = "multi_thread")]
        async fn tracks_children_of_an_exiting_launcher_and_records_session() {
            // Launcher script exits immediately, its child keeps "the game" running for ~1.2 s.
            let (_tmp, launcher, item, events) = setup("#!/bin/sh\nsleep 1.2 &\nexit 0\n").await;
            let host = HostEnv::detect(std::env::temp_dir());
            let result = launcher.launch(&item, &host).unwrap();
            assert_eq!(result.method, LaunchMethod::Process);
            assert_eq!(launcher.running().len(), 1);
            assert!(matches!(
                launcher.launch(&item, &host),
                Err(AppError::AlreadyRunning(_))
            ));

            wait_until_stopped(&launcher, &item.game.id).await;
            let events = events.lock().unwrap().clone();
            assert_eq!(events.len(), 2);
            assert_eq!(events[0].state, GameState::Running);
            assert_eq!(events[1].state, GameState::Stopped);
            assert!(
                events[1].session_seconds.unwrap() >= 1,
                "child kept the session alive"
            );

            let sessions: Vec<(i64, Option<String>)> =
                sqlx::query_as("SELECT duration_seconds, ended_at FROM play_sessions")
                    .fetch_all(&launcher.pool)
                    .await
                    .unwrap();
            assert_eq!(sessions.len(), 1);
            assert!(sessions[0].0 >= 1 && sessions[0].1.is_some());
        }

        #[tokio::test(flavor = "multi_thread")]
        async fn stop_terminates_the_process_tree() {
            let (_tmp, launcher, item, events) = setup("#!/bin/sh\nsleep 30 &\nwait\n").await;
            launcher
                .launch(&item, &HostEnv::detect(std::env::temp_dir()))
                .unwrap();
            tokio::time::sleep(Duration::from_millis(300)).await;
            let started = Instant::now();
            launcher.stop(&item.game.id).unwrap();
            wait_until_stopped(&launcher, &item.game.id).await;
            assert!(started.elapsed() < Duration::from_secs(5));
            assert_eq!(
                events.lock().unwrap().last().unwrap().state,
                GameState::Stopped
            );
            assert!(matches!(
                launcher.stop(&item.game.id),
                Err(AppError::NotRunning(_))
            ));
        }
    }
}
