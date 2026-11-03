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
    LaunchResult, LibraryItem, Platform, RunningGame, StoreSource,
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
    /// Proton installations found on this machine (folders containing a `proton` script).
    pub proton_installs: Vec<PathBuf>,
}

impl HostEnv {
    pub fn detect(compat_data_root: PathBuf) -> Self {
        let home = std::env::var_os("HOME")
            .or_else(|| std::env::var_os("USERPROFILE"))
            .map(PathBuf::from);
        let proton_installs = home
            .as_deref()
            .map(find_proton_installs)
            .unwrap_or_default();
        Self {
            platform: Platform::current(),
            home,
            compat_data_root,
            proton_installs,
        }
    }

    fn steam_root(&self) -> Option<PathBuf> {
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

fn version_key(name: &str) -> Vec<u32> {
    name.split(|c: char| !c.is_ascii_digit())
        .filter_map(|p| p.parse().ok())
        .collect()
}

/// Picks "Proton - Experimental" or the newest numbered official Proton (falls back to any build, e.g. GE).
pub fn pick_proton(installs: &[PathBuf], experimental: bool) -> Option<PathBuf> {
    let name = |p: &PathBuf| {
        p.file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default()
    };
    if experimental {
        return installs
            .iter()
            .find(|p| name(p).to_lowercase().contains("experimental"))
            .cloned();
    }
    let mut stable: Vec<&PathBuf> = installs
        .iter()
        .filter(|p| {
            let n = name(p).to_lowercase();
            !n.contains("experimental") && !n.contains("hotfix") && !version_key(&n).is_empty()
        })
        .collect();
    stable.sort_by_key(|p| (name(p).starts_with("Proton "), version_key(&name(p))));
    stable.last().map(|p| (*p).clone())
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
        StoreSource::Manual => None,
    }
}

fn split_args(raw: &str) -> AppResult<Vec<String>> {
    shlex::split(raw).ok_or_else(|| {
        AppError::InvalidInput("Startparameter: Anführungszeichen sind nicht geschlossen.".into())
    })
}

/// Decides how to start a game.
pub fn plan(target: &LaunchTarget, host: &HostEnv) -> AppResult<LaunchPlan> {
    // Steam games always go through Steam (DRM, overlay, cloud saves).
    if target.source == StoreSource::Steam {
        if let Some(id) = &target.store_game_id {
            return Ok(LaunchPlan::Url(
                store_url(StoreSource::Steam, id).unwrap_or_default(),
            ));
        }
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
            "Für dieses Spiel ist noch keine ausführbare Datei festgelegt. Trage sie unter Einstellungen ein.".into(),
        ));
    };

    let exe = PathBuf::from(executable);
    let user_args = split_args(&target.options.arguments)?;
    let mut env: Vec<(String, String)> = Vec::new();
    let mut cwd = exe.parent().map(Path::to_path_buf);
    let has_ext = |ext: &str| exe.extension().is_some_and(|e| e.eq_ignore_ascii_case(ext));

    let (program, mut args) = if has_ext("desktop") {
        let content = std::fs::read_to_string(&exe)?;
        let entry = parse_desktop_entry(&content).ok_or_else(|| {
            AppError::Launch("Die .desktop-Datei enthält keinen gültigen Start-Eintrag.".into())
        })?;
        let mut parts = split_args(&entry.exec)?.into_iter();
        let program = parts
            .next()
            .ok_or_else(|| AppError::Launch("Exec-Zeile ist leer.".into()))?;
        if let Some(dir) = entry.working_dir {
            cwd = Some(PathBuf::from(dir));
        }
        (PathBuf::from(program), parts.collect::<Vec<_>>())
    } else if has_ext("exe") && host.platform != Platform::Windows {
        match target.options.compatibility_tool {
            CompatibilityTool::Native => {
                return Err(AppError::Launch(
                    "Das ist ein Windows-Spiel. Wähle unter Einstellungen → Kompatibilität Proton oder Wine.".into(),
                ))
            }
            CompatibilityTool::Wine => (PathBuf::from("wine"), vec![executable.to_owned()]),
            tool @ (CompatibilityTool::Proton | CompatibilityTool::ProtonExperimental) => {
                let experimental = tool == CompatibilityTool::ProtonExperimental;
                let proton = pick_proton(&host.proton_installs, experimental).ok_or_else(|| {
                    AppError::CompatToolNotFound(if experimental { "Proton Experimental" } else { "Proton" }.into())
                })?;
                let prefix = host.compat_data_root.join(&target.game_id);
                env.push(("STEAM_COMPAT_DATA_PATH".into(), prefix.to_string_lossy().into_owned()));
                if let Some(steam) = host.steam_root() {
                    env.push(("STEAM_COMPAT_CLIENT_INSTALL_PATH".into(), steam.to_string_lossy().into_owned()));
                }
                (proton.join("proton"), vec!["run".into(), executable.to_owned()])
            }
        }
    } else {
        (exe.clone(), Vec::new())
    };

    args.extend(user_args);
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
        let game_id = item.game.id.clone();
        if self.lock().contains_key(&game_id) {
            return Err(AppError::AlreadyRunning(game_id));
        }
        let target = LaunchTarget::from(item);
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
                    return Err(AppError::Launch(format!(
                        "Datei nicht gefunden: {}",
                        program.display()
                    )));
                }
                if let Some((_, prefix)) = env.iter().find(|(k, _)| k == "STEAM_COMPAT_DATA_PATH") {
                    std::fs::create_dir_all(prefix)?;
                }
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
                    AppError::Launch(format!(
                        "{} konnte nicht gestartet werden: {e}",
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
            let procs = snapshot(&mut system);
            tracker.update(&procs);

            if stop.load(Ordering::SeqCst) {
                let first = *kill_requested_at.get_or_insert_with(Instant::now);
                // Polite SIGTERM first, SIGKILL after 3 s.
                let force = first.elapsed() > Duration::from_secs(3);
                for pid in tracker.pids() {
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

    fn host(platform: Platform, proton: &[&str]) -> HostEnv {
        HostEnv {
            platform,
            home: None,
            compat_data_root: PathBuf::from("/data/compat"),
            proton_installs: proton.iter().map(PathBuf::from).collect(),
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
        let protons = [
            "/s/common/Proton 8.0",
            "/s/common/Proton 9.0",
            "/s/common/Proton - Experimental",
        ];
        let LaunchPlan::Process {
            program, args, env, ..
        } = plan(&t, &host(Platform::Linux, &protons)).unwrap()
        else {
            panic!()
        };
        assert_eq!(program, PathBuf::from("/s/common/Proton 9.0/proton"));
        assert_eq!(args, ["run", "/g/Game.exe"]);
        assert_eq!(
            env[0],
            (
                "STEAM_COMPAT_DATA_PATH".to_owned(),
                "/data/compat/g".to_owned()
            )
        );

        t.options.compatibility_tool = CompatibilityTool::ProtonExperimental;
        let LaunchPlan::Process { program, .. } =
            plan(&t, &host(Platform::Linux, &protons)).unwrap()
        else {
            panic!()
        };
        assert_eq!(
            program,
            PathBuf::from("/s/common/Proton - Experimental/proton")
        );
    }

    #[test]
    fn picks_newest_official_proton() {
        let installs: Vec<PathBuf> = [
            "Proton 7.0",
            "Proton 10.0",
            "Proton 9.0",
            "GE-Proton9-20",
            "Proton Hotfix",
        ]
        .iter()
        .map(PathBuf::from)
        .collect();
        assert_eq!(
            pick_proton(&installs, false),
            Some(PathBuf::from("Proton 10.0"))
        );
        assert_eq!(pick_proton(&installs, true), None);
        assert_eq!(
            pick_proton(&[PathBuf::from("GE-Proton9-20")], false),
            Some(PathBuf::from("GE-Proton9-20"))
        );
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
