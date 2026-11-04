//! Sidecar manager: runs a JSON-RPC 2.0 service (newline-delimited JSON over
//! stdin/stdout) as a child process.
//!
//! - Started lazily on the first call (or eagerly via [`Sidecar::warm_up`]) and
//!   health-checked with `system.ping` before it is used.
//! - Every request has a timeout; a timeout, EOF or broken pipe kills the
//!   process and the next call starts a fresh one.
//! - Crash loops are cut off: at most `MAX_STARTS_PER_MINUTE` starts per minute.

use std::collections::VecDeque;
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::time::{Duration, Instant};

use serde_json::{json, Value};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader, Lines};
use tokio::process::{Child, ChildStdin, ChildStdout, Command};
use tokio::sync::Mutex;

use crate::error::{AppError, AppResult};

const MAX_STARTS_PER_MINUTE: usize = 3;
/// First line written by `build.rs` into the placeholder binary.
pub const PLACEHOLDER_MARKER: &str = "cove-sidecar-placeholder";

#[derive(Debug, Clone)]
pub struct SidecarConfig {
    pub name: String,
    pub program: PathBuf,
    pub args: Vec<String>,
    pub cwd: Option<PathBuf>,
    /// Startup incl. health check (a `PyInstaller` onefile binary unpacks itself first).
    pub start_timeout: Duration,
    pub request_timeout: Duration,
}

impl SidecarConfig {
    pub fn new(name: &str, program: PathBuf, args: Vec<String>) -> Self {
        Self {
            name: name.to_owned(),
            program,
            args,
            cwd: None,
            start_timeout: Duration::from_secs(30),
            request_timeout: Duration::from_secs(20),
        }
    }
}

/// True for the stand-in that `build.rs` creates when the real binary was not built.
pub fn is_placeholder(path: &Path) -> bool {
    match std::fs::metadata(path) {
        Ok(meta) if meta.len() < 4096 => {
            std::fs::read_to_string(path).is_ok_and(|text| text.contains(PLACEHOLDER_MARKER))
        }
        Ok(_) => false,
        Err(_) => true,
    }
}

/// Finds the recommender: the bundled sidecar next to the app binary, or —
/// in debug builds without a built sidecar — `uv run` in the repository.
pub fn resolve_recommender() -> Option<SidecarConfig> {
    if let Ok(custom) = std::env::var("COVE_RECOMMENDER_CMD") {
        let mut parts = shlex::split(&custom)?.into_iter();
        let program = PathBuf::from(parts.next()?);
        return Some(SidecarConfig::new("recommender", program, parts.collect()));
    }
    let exe_dir = std::env::current_exe().ok()?.parent()?.to_path_buf();
    let bundled = exe_dir.join(if cfg!(windows) {
        "cove-recommender.exe"
    } else {
        "cove-recommender"
    });
    if !is_placeholder(&bundled) {
        return Some(SidecarConfig::new("recommender", bundled, Vec::new()));
    }
    if cfg!(debug_assertions) {
        let project = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../../services/recommender");
        if project.join("pyproject.toml").is_file() && which("uv").is_some() {
            let mut config = SidecarConfig::new(
                "recommender",
                PathBuf::from("uv"),
                vec!["run".into(), "--quiet".into(), "cove-recommender".into()],
            );
            config.cwd = Some(project);
            return Some(config);
        }
    }
    None
}

/// Finds the store connectors: the bundled sidecar next to the app binary, or —
/// in debug builds — the `dotnet build` output / `dotnet run` in the repository.
pub fn resolve_connectors() -> Option<SidecarConfig> {
    let named = |program: PathBuf, args: Vec<String>| {
        let mut config = SidecarConfig::new("connectors", program, args);
        // Library imports page through store APIs; allow them more time.
        config.request_timeout = Duration::from_secs(120);
        config
    };
    if let Ok(custom) = std::env::var("COVE_CONNECTORS_CMD") {
        let mut parts = shlex::split(&custom)?.into_iter();
        let program = PathBuf::from(parts.next()?);
        return Some(named(program, parts.collect()));
    }
    let exe_name = if cfg!(windows) {
        "cove-connectors.exe"
    } else {
        "cove-connectors"
    };
    let bundled = std::env::current_exe().ok()?.parent()?.join(exe_name);
    if !is_placeholder(&bundled) {
        return Some(named(bundled, Vec::new()));
    }
    if cfg!(debug_assertions) {
        let project = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../../services/connectors/src/Cove.Connectors");
        let built = project.join("bin/Debug/net9.0").join(if cfg!(windows) {
            "cove-connectors.exe"
        } else {
            "cove-connectors"
        });
        if built.is_file() {
            return Some(named(built, Vec::new()));
        }
        if project.join("Cove.Connectors.csproj").is_file() && which("dotnet").is_some() {
            let mut config = named(
                PathBuf::from("dotnet"),
                vec![
                    "run".into(),
                    "--project".into(),
                    project.display().to_string(),
                ],
            );
            config.start_timeout = Duration::from_secs(120);
            return Some(config);
        }
    }
    None
}

fn which(program: &str) -> Option<PathBuf> {
    std::env::var_os("PATH").and_then(|paths| {
        std::env::split_paths(&paths)
            .map(|dir| dir.join(program))
            .find(|p| p.is_file())
    })
}

/// A JSON-RPC error answered by the service (the process stays usable).
#[derive(Debug, Clone, PartialEq)]
pub struct RpcError {
    pub code: i64,
    pub message: String,
    pub data: Option<Value>,
}

/// How a sidecar's failures become [`AppError`]s.
#[derive(Debug, Clone, Copy)]
pub struct SidecarErrors {
    pub unavailable: fn(String) -> AppError,
    pub rpc: fn(RpcError) -> AppError,
}

impl SidecarErrors {
    pub const RECOMMENDER: Self = Self {
        unavailable: AppError::RecommenderUnavailable,
        rpc: recommender_rpc_error,
    };
}

fn recommender_rpc_error(error: RpcError) -> AppError {
    AppError::Recommender(error.message)
}

struct Running {
    child: Child,
    stdin: ChildStdin,
    stdout: Lines<BufReader<ChildStdout>>,
    next_id: u64,
}

pub struct Sidecar {
    config: Option<SidecarConfig>,
    errors: SidecarErrors,
    running: Mutex<Option<Running>>,
    starts: std::sync::Mutex<VecDeque<Instant>>,
}

impl Sidecar {
    pub fn new(config: Option<SidecarConfig>) -> Self {
        Self::with_errors(config, SidecarErrors::RECOMMENDER)
    }

    pub fn with_errors(config: Option<SidecarConfig>, errors: SidecarErrors) -> Self {
        Self {
            config,
            errors,
            running: Mutex::new(None),
            starts: std::sync::Mutex::new(VecDeque::new()),
        }
    }

    pub fn is_configured(&self) -> bool {
        self.config.is_some()
    }

    fn unavailable(&self, message: impl Into<String>) -> AppError {
        (self.errors.unavailable)(message.into())
    }

    fn note_start(&self) -> AppResult<()> {
        let mut starts = self
            .starts
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let now = Instant::now();
        while starts
            .front()
            .is_some_and(|t| now.duration_since(*t) > Duration::from_secs(60))
        {
            starts.pop_front();
        }
        if starts.len() >= MAX_STARTS_PER_MINUTE {
            return Err(self.unavailable("stürzt wiederholt ab, nächster Versuch in einer Minute"));
        }
        starts.push_back(now);
        Ok(())
    }

    async fn spawn(&self, config: &SidecarConfig) -> AppResult<Running> {
        self.note_start()?;
        let mut command = Command::new(&config.program);
        command
            .args(&config.args)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .kill_on_drop(true);
        if let Some(cwd) = &config.cwd {
            command.current_dir(cwd);
        }
        let mut child = command.spawn().map_err(|e| {
            self.unavailable(format!(
                "{} konnte nicht gestartet werden: {e}",
                config.program.display()
            ))
        })?;
        let stdin = child
            .stdin
            .take()
            .ok_or_else(|| self.unavailable("kein stdin"))?;
        let stdout = child
            .stdout
            .take()
            .ok_or_else(|| self.unavailable("kein stdout"))?;
        if let Some(stderr) = child.stderr.take() {
            let name = config.name.clone();
            tokio::spawn(async move {
                let mut lines = BufReader::new(stderr).lines();
                while let Ok(Some(line)) = lines.next_line().await {
                    eprintln!("[{name}] {line}");
                }
            });
        }
        let mut running = Running {
            child,
            stdin,
            stdout: BufReader::new(stdout).lines(),
            next_id: 1,
        };
        // Health check: the service must answer a ping before it is used.
        let ping = Self::exchange(
            &mut running,
            "system.ping",
            json!({ "message": "health" }),
            config.start_timeout,
        )
        .await;
        match ping {
            Ok(_) => Ok(running),
            Err(e) => {
                let _ = running.child.kill().await;
                Err(self.unavailable(format!("startet nicht ({e})")))
            }
        }
    }

    /// One request/response round trip. Errors other than a JSON-RPC error
    /// leave the process in an unknown state; the caller discards it.
    async fn exchange(
        running: &mut Running,
        method: &str,
        params: Value,
        timeout: Duration,
    ) -> Result<Value, Exchange> {
        let id = running.next_id;
        running.next_id += 1;
        let mut line = serde_json::to_string(
            &json!({ "jsonrpc": "2.0", "id": id, "method": method, "params": params }),
        )
        .map_err(|e| Exchange::Broken(e.to_string()))?;
        line.push('\n');
        let work = async {
            running
                .stdin
                .write_all(line.as_bytes())
                .await
                .map_err(|e| Exchange::Broken(e.to_string()))?;
            running
                .stdin
                .flush()
                .await
                .map_err(|e| Exchange::Broken(e.to_string()))?;
            loop {
                let Some(text) = running
                    .stdout
                    .next_line()
                    .await
                    .map_err(|e| Exchange::Broken(e.to_string()))?
                else {
                    return Err(Exchange::Broken("Prozess beendet".into()));
                };
                let Ok(message) = serde_json::from_str::<Value>(&text) else {
                    continue;
                };
                if message.get("id").and_then(Value::as_u64) != Some(id) {
                    continue;
                }
                if let Some(error) = message.get("error") {
                    return Err(Exchange::Rpc(RpcError {
                        code: error.get("code").and_then(Value::as_i64).unwrap_or(0),
                        message: error
                            .get("message")
                            .and_then(Value::as_str)
                            .unwrap_or("unbekannter Fehler")
                            .to_owned(),
                        data: error.get("data").cloned(),
                    }));
                }
                return Ok(message.get("result").cloned().unwrap_or(Value::Null));
            }
        };
        tokio::time::timeout(timeout, work)
            .await
            .map_err(|_| Exchange::Timeout)?
    }

    /// Calls `method`; starts or restarts the process as needed.
    pub async fn call(&self, method: &str, params: Value) -> AppResult<Value> {
        let Some(config) = &self.config else {
            return Err(self.unavailable("nicht installiert"));
        };
        let mut guard = self.running.lock().await;
        // A process that exited since the last call is replaced.
        if let Some(running) = guard.as_mut() {
            if running.child.try_wait().ok().flatten().is_some() {
                *guard = None;
            }
        }
        if guard.is_none() {
            *guard = Some(self.spawn(config).await?);
        }
        let running = guard
            .as_mut()
            .ok_or_else(|| self.unavailable("nicht gestartet"))?;
        match Self::exchange(running, method, params, config.request_timeout).await {
            Ok(value) => Ok(value),
            Err(Exchange::Rpc(error)) => Err((self.errors.rpc)(error)),
            Err(other) => {
                if let Some(mut dead) = guard.take() {
                    let _ = dead.child.kill().await;
                }
                Err(self.unavailable(match other {
                    Exchange::Timeout => "antwortet nicht (Zeitüberschreitung)".to_owned(),
                    Exchange::Broken(e) | Exchange::Rpc(RpcError { message: e, .. }) => {
                        format!("abgestürzt ({e})")
                    }
                }))
            }
        }
    }

    /// Starts the process in the background so the first real request is fast.
    pub async fn warm_up(&self) -> AppResult<()> {
        self.call("system.ping", json!({ "message": "warm-up" }))
            .await
            .map(|_| ())
    }
}

enum Exchange {
    Timeout,
    Broken(String),
    Rpc(RpcError),
}

impl std::fmt::Display for Exchange {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Timeout => write!(f, "Zeitüberschreitung"),
            Self::Broken(e) | Self::Rpc(RpcError { message: e, .. }) => write!(f, "{e}"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A tiny JSON-RPC service in Python with failure modes for the tests.
    const FAKE: &str = r#"
import json, os, sys, time
mode = sys.argv[1] if len(sys.argv) > 1 else "ok"
if mode == "dead":
    sys.exit(3)
for line in sys.stdin:
    msg = json.loads(line)
    method, mid = msg["method"], msg["id"]
    if method == "system.ping" and mode == "noping":
        continue
    if method == "crash":
        os._exit(1)
    if method == "hang":
        time.sleep(30)
    if method == "fail":
        out = {"jsonrpc": "2.0", "id": mid, "error": {"code": -32602, "message": "bad params"}}
    else:
        out = {"jsonrpc": "2.0", "id": mid, "result": {"method": method, "pid": os.getpid()}}
    print("noise that is not json", flush=True)
    print(json.dumps(out), flush=True)
"#;

    fn sidecar(dir: &tempfile::TempDir, mode: &str) -> Sidecar {
        let script = dir.path().join("fake.py");
        std::fs::write(&script, FAKE).unwrap();
        let mut config = SidecarConfig::new(
            "fake",
            PathBuf::from("python3"),
            vec![script.to_string_lossy().into_owned(), mode.into()],
        );
        config.start_timeout = Duration::from_secs(5);
        config.request_timeout = Duration::from_millis(800);
        Sidecar::new(Some(config))
    }

    fn pid(value: &Value) -> u64 {
        value["pid"].as_u64().unwrap()
    }

    #[tokio::test]
    async fn calls_and_reuses_the_process() {
        let dir = tempfile::tempdir().unwrap();
        let sidecar = sidecar(&dir, "ok");
        let first = sidecar.call("recommend_for_user", json!({})).await.unwrap();
        assert_eq!(first["method"], "recommend_for_user");
        let second = sidecar.call("discover_new", json!({})).await.unwrap();
        assert_eq!(pid(&first), pid(&second), "same process");
    }

    #[tokio::test]
    async fn restarts_after_a_crash() {
        let dir = tempfile::tempdir().unwrap();
        let sidecar = sidecar(&dir, "ok");
        let before = pid(&sidecar.call("a", json!({})).await.unwrap());
        assert!(matches!(
            sidecar.call("crash", json!({})).await,
            Err(AppError::RecommenderUnavailable(_))
        ));
        let after = pid(&sidecar.call("a", json!({})).await.unwrap());
        assert_ne!(before, after);
    }

    #[tokio::test]
    async fn times_out_hanging_requests_and_recovers() {
        let dir = tempfile::tempdir().unwrap();
        let sidecar = sidecar(&dir, "ok");
        let before = pid(&sidecar.call("a", json!({})).await.unwrap());
        let started = Instant::now();
        let err = sidecar.call("hang", json!({})).await.unwrap_err();
        assert!(started.elapsed() < Duration::from_secs(3));
        assert!(err.to_string().contains("Zeitüberschreitung"), "{err}");
        assert_ne!(pid(&sidecar.call("a", json!({})).await.unwrap()), before);
    }

    #[tokio::test]
    async fn rpc_errors_keep_the_process() {
        let dir = tempfile::tempdir().unwrap();
        let sidecar = sidecar(&dir, "ok");
        let before = pid(&sidecar.call("a", json!({})).await.unwrap());
        assert!(
            matches!(sidecar.call("fail", json!({})).await, Err(AppError::Recommender(m)) if m == "bad params")
        );
        assert_eq!(pid(&sidecar.call("a", json!({})).await.unwrap()), before);
    }

    #[tokio::test]
    async fn failed_health_check_and_crash_loops_are_reported() {
        let dir = tempfile::tempdir().unwrap();
        let dead = sidecar(&dir, "dead");
        for _ in 0..MAX_STARTS_PER_MINUTE {
            assert!(matches!(
                dead.call("a", json!({})).await,
                Err(AppError::RecommenderUnavailable(_))
            ));
        }
        let err = dead.call("a", json!({})).await.unwrap_err();
        assert!(err.to_string().contains("wiederholt"), "{err}");

        let mut config = SidecarConfig::new("x", PathBuf::from("/definitely/missing"), Vec::new());
        config.start_timeout = Duration::from_millis(200);
        let missing = Sidecar::new(Some(config));
        assert!(matches!(
            missing.call("a", json!({})).await,
            Err(AppError::RecommenderUnavailable(_))
        ));
        let none = Sidecar::new(None);
        assert!(!none.is_configured());
        assert!(matches!(
            none.call("a", json!({})).await,
            Err(AppError::RecommenderUnavailable(_))
        ));
    }

    #[test]
    fn detects_the_build_placeholder() {
        let dir = tempfile::tempdir().unwrap();
        let placeholder = dir.path().join("p");
        std::fs::write(
            &placeholder,
            format!("#!/bin/sh\n# {PLACEHOLDER_MARKER}\nexit 3\n"),
        )
        .unwrap();
        assert!(is_placeholder(&placeholder));
        let real = dir.path().join("r");
        std::fs::write(&real, vec![0u8; 10_000]).unwrap();
        assert!(!is_placeholder(&real));
        assert!(is_placeholder(&dir.path().join("missing")));
    }
}
