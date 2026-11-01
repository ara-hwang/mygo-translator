//! Supervision of the local `llama-server` process.
//!
//! The application ships no native inference code: it starts a standard llama.cpp server with
//! the Hy-MT2 GGUF, waits until `/health` reports ready, and then talks to it over loopback.
//! Benefits: no C++ toolchain is needed to build HyMT, any llama.cpp build (CPU, CUDA, Vulkan,
//! ROCm) works, and a crashed model can never take the UI down with it.
//!
//! All process handling lives on one supervisor thread; the UI polls events and never blocks.

use std::collections::VecDeque;
use std::fs;
use std::fs::OpenOptions;
use std::io::{self, BufRead, BufReader, Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError, Sender};
use std::sync::{Arc, Mutex};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

use crate::client::{self, Endpoint};
use crate::http::{cancel_token, CancelToken, HttpError};

/// Executable name of llama.cpp's server, per platform.
pub const SERVER_BINARY: &str = if cfg!(windows) {
    "llama-server.exe"
} else {
    "llama-server"
};

/// Maximum time a model may take to load before the launch is considered failed.
pub const READY_TIMEOUT: Duration = Duration::from_secs(300);
/// How many trailing log lines are quoted in failure messages.
const TAIL_LINES: usize = 8;

/// Everything needed to launch the server.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LaunchConfig {
    /// Path to `llama-server(.exe)`.
    pub server_path: PathBuf,
    /// Path to the `.gguf` model.
    pub model_path: PathBuf,
    /// Loopback port to bind.
    pub port: u16,
    /// Context window.
    pub ctx_size: u32,
    /// Worker threads.
    pub threads: u32,
    /// GPU layers (`0` = CPU only, `-1` = let llama.cpp decide).
    pub gpu_layers: i32,
    /// Model alias reported by the OpenAI-compatible API.
    pub alias: String,
}

impl LaunchConfig {
    /// Builds the llama.cpp command line.
    ///
    /// Deliberately conservative: only flags that have existed for many llama.cpp releases are
    /// used, so an older binary does not abort on an unknown argument. `--jinja` is required
    /// because Hy-MT2's chat template ships inside the GGUF.
    pub fn args(&self) -> Vec<String> {
        vec![
            "-m".to_string(),
            self.model_path.to_string_lossy().into_owned(),
            "--host".to_string(),
            "127.0.0.1".to_string(),
            "--port".to_string(),
            self.port.to_string(),
            "-c".to_string(),
            self.ctx_size.to_string(),
            "-t".to_string(),
            self.threads.to_string(),
            "-ngl".to_string(),
            self.gpu_layers.to_string(),
            "-np".to_string(),
            "1".to_string(),
            "--jinja".to_string(),
            "--alias".to_string(),
            self.alias.clone(),
        ]
    }

    /// Endpoint the server will listen on.
    pub fn endpoint(&self) -> Endpoint {
        Endpoint::localhost(self.port)
    }
}

/// Published state of the managed server.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum EngineState {
    /// Nothing is running.
    Stopped,
    /// A process was started and the model is loading.
    Starting,
    /// `/health` reports ready; translations can be sent.
    Ready,
    /// Launch or runtime failure. The string is user-facing.
    Failed(String),
}

impl EngineState {
    /// Korean label for the status bar.
    pub fn label(&self) -> String {
        match self {
            EngineState::Stopped => "중지됨".to_string(),
            EngineState::Starting => "모델 불러오는 중".to_string(),
            EngineState::Ready => "준비됨".to_string(),
            EngineState::Failed(_) => "오류".to_string(),
        }
    }

    /// True when translations can be sent.
    pub fn is_ready(&self) -> bool {
        matches!(self, EngineState::Ready)
    }

    /// True when a launch is in flight.
    pub fn is_busy(&self) -> bool {
        matches!(self, EngineState::Starting)
    }

    /// Failure detail, if any.
    pub fn detail(&self) -> Option<&str> {
        match self {
            EngineState::Failed(message) => Some(message),
            _ => None,
        }
    }
}

/// Events published by the supervisor thread.
#[derive(Clone, Debug, PartialEq)]
pub enum EngineEvent {
    /// A state transition.
    State(EngineState),
    /// One line of llama-server output.
    Log(String),
}

enum Command_ {
    Start(Box<LaunchConfig>),
    Stop,
    Shutdown,
}

/// Owns the supervisor thread and the llama-server child process.
pub struct Supervisor {
    commands: Sender<Command_>,
    events: Receiver<EngineEvent>,
    join: Option<JoinHandle<()>>,
}

impl Supervisor {
    /// Creates the supervisor. It starts in [`EngineState::Stopped`].
    pub fn spawn(log_path: PathBuf) -> io::Result<Self> {
        let (command_tx, command_rx) = mpsc::channel();
        let (event_tx, event_rx) = mpsc::channel();
        let tail: Arc<Mutex<VecDeque<String>>> = Arc::new(Mutex::new(VecDeque::new()));
        let join = thread::Builder::new()
            .name("hymt-engine".to_string())
            .spawn(move || run(command_rx, event_tx, log_path, tail))?;
        Ok(Self {
            commands: command_tx,
            events: event_rx,
            join: Some(join),
        })
    }

    /// Asks the supervisor to (re)start the model server.
    pub fn start(&self, config: LaunchConfig) {
        let _ = self.commands.send(Command_::Start(Box::new(config)));
    }

    /// Asks the supervisor to stop the model server.
    pub fn stop(&self) {
        let _ = self.commands.send(Command_::Stop);
    }

    /// Non-blocking event read.
    pub fn try_recv(&self) -> Option<EngineEvent> {
        self.events.try_recv().ok()
    }

    /// Blocking event read with a timeout, used by tests and the UI pump.
    pub fn recv_timeout(&self, timeout: Duration) -> Option<EngineEvent> {
        self.events.recv_timeout(timeout).ok()
    }
}

impl Drop for Supervisor {
    fn drop(&mut self) {
        let _ = self.commands.send(Command_::Shutdown);
        if let Some(join) = self.join.take() {
            let _ = join.join();
        }
    }
}

fn run(
    commands: Receiver<Command_>,
    events: Sender<EngineEvent>,
    log_path: PathBuf,
    tail: Arc<Mutex<VecDeque<String>>>,
) {
    let mut child: Option<Child> = None;
    let mut pending: Option<(LaunchConfig, Instant)> = None;
    let cancel = cancel_token();
    let _ = events.send(EngineEvent::State(EngineState::Stopped));

    loop {
        match commands.recv_timeout(Duration::from_millis(150)) {
            Ok(Command_::Start(config)) => {
                stop_child(&mut child, &mut pending);
                tail.lock().map(|mut tail| tail.clear()).ok();
                append_log(
                    &log_path,
                    &format!(
                        "=== starting {} ({} ctx, {} threads, ngl {}) ===",
                        config.model_path.display(),
                        config.ctx_size,
                        config.threads,
                        config.gpu_layers
                    ),
                );
                match spawn_child(&config, &log_path, &events, &tail) {
                    Ok(new_child) => {
                        child = Some(new_child);
                        pending = Some(((*config).clone(), Instant::now()));
                        let _ = events.send(EngineEvent::State(EngineState::Starting));
                    }
                    Err(err) => {
                        let _ = events.send(EngineEvent::State(EngineState::Failed(format!(
                            "llama-server를 실행할 수 없습니다: {err}"
                        ))));
                    }
                }
            }
            Ok(Command_::Stop) => {
                stop_child(&mut child, &mut pending);
                let _ = events.send(EngineEvent::State(EngineState::Stopped));
            }
            Ok(Command_::Shutdown) => {
                stop_child(&mut child, &mut pending);
                return;
            }
            Err(RecvTimeoutError::Timeout) => {}
            Err(RecvTimeoutError::Disconnected) => {
                stop_child(&mut child, &mut pending);
                return;
            }
        }

        // Did the child die on its own?
        let mut exit_message: Option<String> = None;
        if let Some(process) = child.as_mut() {
            match process.try_wait() {
                Ok(Some(status)) => {
                    child = None;
                    if pending.take().is_some() {
                        exit_message = Some(format!(
                            "모델 서버가 종료되었습니다 ({status}). {}",
                            tail_summary(&tail)
                        ));
                    }
                }
                Ok(None) => {}
                Err(err) => {
                    exit_message = Some(format!("모델 서버 상태를 확인할 수 없습니다: {err}"));
                    child = None;
                    pending = None;
                }
            }
        }
        if let Some(message) = exit_message {
            append_log(&log_path, &format!("[fatal] {message}"));
            let _ = events.send(EngineEvent::State(EngineState::Failed(message)));
            continue;
        }

        // Still loading? Probe /health.
        let Some((config, started)) = pending.clone() else {
            continue;
        };
        let endpoint = config.endpoint();
        match client::probe_health(&endpoint, cancel.clone(), Duration::from_secs(2)) {
            Ok(health) if health.ready => {
                pending = None;
                append_log(&log_path, "=== ready ===");
                let _ = events.send(EngineEvent::State(EngineState::Ready));
            }
            Ok(health) => {
                if started.elapsed() > READY_TIMEOUT {
                    stop_child(&mut child, &mut pending);
                    let message = format!(
                        "모델을 {}초 안에 불러오지 못했습니다. ({})",
                        READY_TIMEOUT.as_secs(),
                        if health.detail.is_empty() {
                            "응답 없음".to_string()
                        } else {
                            health.detail
                        }
                    );
                    append_log(&log_path, &format!("[fatal] {message}"));
                    let _ = events.send(EngineEvent::State(EngineState::Failed(message)));
                }
            }
            Err(HttpError::Cancelled) => {}
            Err(err) => {
                if started.elapsed() > READY_TIMEOUT {
                    stop_child(&mut child, &mut pending);
                    let message = format!(
                        "모델을 {}초 안에 불러오지 못했습니다: {err}",
                        READY_TIMEOUT.as_secs()
                    );
                    append_log(&log_path, &format!("[fatal] {message}"));
                    let _ = events.send(EngineEvent::State(EngineState::Failed(message)));
                }
            }
        }
    }
}

fn stop_child(child: &mut Option<Child>, pending: &mut Option<(LaunchConfig, Instant)>) {
    *pending = None;
    if let Some(mut process) = child.take() {
        let _ = process.kill();
        let _ = process.wait();
    }
}

fn spawn_child(
    config: &LaunchConfig,
    log_path: &Path,
    events: &Sender<EngineEvent>,
    tail: &Arc<Mutex<VecDeque<String>>>,
) -> io::Result<Child> {
    let mut command = Command::new(&config.server_path);
    command
        .args(config.args())
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());

    #[cfg(windows)]
    {
        // Keep the console window hidden; the app is a GUI.
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        command.creation_flags(CREATE_NO_WINDOW);
    }

    let mut child = command.spawn()?;
    if let Some(stdout) = child.stdout.take() {
        pump(stdout, log_path.to_path_buf(), events.clone(), tail.clone());
    }
    if let Some(stderr) = child.stderr.take() {
        pump(stderr, log_path.to_path_buf(), events.clone(), tail.clone());
    }
    Ok(child)
}

fn pump<R>(
    stream: R,
    log_path: PathBuf,
    events: Sender<EngineEvent>,
    tail: Arc<Mutex<VecDeque<String>>>,
) where
    R: Read + Send + 'static,
{
    thread::spawn(move || {
        let mut log = OpenOptions::new()
            .create(true)
            .append(true)
            .open(&log_path)
            .ok();
        let reader = BufReader::new(stream);
        for line in reader.lines().map_while(Result::ok) {
            let line = line.trim_end().to_string();
            if line.is_empty() {
                continue;
            }
            if let Some(file) = log.as_mut() {
                let _ = writeln!(file, "{line}");
                let _ = file.flush();
            }
            if let Ok(mut tail) = tail.lock() {
                tail.push_back(line.clone());
                while tail.len() > TAIL_LINES {
                    tail.pop_front();
                }
            }
            if events.send(EngineEvent::Log(line)).is_err() {
                break;
            }
        }
    });
}

fn tail_summary(tail: &Arc<Mutex<VecDeque<String>>>) -> String {
    let Ok(tail) = tail.lock() else {
        return String::new();
    };
    if tail.is_empty() {
        return String::new();
    }
    let joined = tail
        .iter()
        .rev()
        .take(3)
        .cloned()
        .collect::<Vec<_>>()
        .into_iter()
        .rev()
        .collect::<Vec<_>>()
        .join(" / ");
    joined.chars().take(400).collect()
}

fn append_log(log_path: &Path, line: &str) {
    if let Ok(mut file) = OpenOptions::new().create(true).append(true).open(log_path) {
        let _ = writeln!(file, "{line}");
    }
}

/// Looks for `llama-server` next to the executable, in `runtime/`, on `PATH`, or at a
/// user-configured location.
pub fn discover_server(exe_dir: &Path, configured: Option<&Path>) -> Option<PathBuf> {
    if let Some(configured) = configured {
        if configured.is_dir() {
            let candidate = configured.join(SERVER_BINARY);
            if candidate.is_file() {
                return Some(candidate);
            }
        } else if configured.is_file() {
            return Some(configured.to_path_buf());
        }
    }

    let mut candidates = vec![
        exe_dir.join(SERVER_BINARY),
        exe_dir.join("runtime").join(SERVER_BINARY),
    ];
    if let Some(parent) = exe_dir.parent() {
        candidates.push(parent.join("runtime").join(SERVER_BINARY));
    }
    for candidate in candidates {
        if candidate.is_file() {
            return Some(candidate);
        }
    }

    if let Some(path_var) = std::env::var_os("PATH") {
        for dir in std::env::split_paths(&path_var) {
            let candidate = dir.join(SERVER_BINARY);
            if candidate.is_file() {
                return Some(candidate);
            }
        }
    }
    None
}

/// Lists `.gguf` files in a folder, sorted by name.
pub fn scan_gguf(dir: &Path) -> Vec<PathBuf> {
    let mut models = Vec::new();
    let Ok(entries) = fs::read_dir(dir) else {
        return models;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_file() && crate::models::is_model_file(&path) {
            models.push(path);
        }
    }
    models.sort_by(|a, b| a.file_name().cmp(&b.file_name()));
    models
}

/// Picks the most sensible default model from a folder listing.
///
/// The 1.8B Q4_K_M file is the app's recommendation, so it wins; otherwise smaller families
/// and lighter quantizations are preferred over 7B/30B files that may not fit in RAM.
pub fn pick_model(models: &[PathBuf]) -> Option<PathBuf> {
    models
        .iter()
        .max_by_key(|path| model_score(&path.file_name().unwrap_or_default().to_string_lossy()))
        .cloned()
}

fn model_score(name: &str) -> i32 {
    let name = name.to_ascii_lowercase();
    let mut score = 0;
    if name.contains("hy-mt2") {
        score += 20;
    }
    if name.contains("1.8b") {
        score += 40;
    }
    if name.contains("q4_k_m") {
        score += 30;
    } else if name.contains("q4_0") {
        score += 20;
    } else if name.contains("q6_k") {
        score += 15;
    } else if name.contains("q8_0") {
        score += 10;
    }
    if name.contains("30b") {
        score -= 40;
    }
    if name.contains("7b") {
        score -= 10;
    }
    if name.contains("1.25bit") || name.contains("2bit") {
        score -= 5;
    }
    score
}

/// Polls `/health` until the server reports ready.
pub fn wait_until_ready(
    endpoint: &Endpoint,
    cancel: CancelToken,
    timeout: Duration,
    poll: Duration,
) -> Result<(), String> {
    let started = Instant::now();
    let mut last = String::from("모델 서버가 응답하지 않습니다.");
    loop {
        if cancel.load(std::sync::atomic::Ordering::Relaxed) {
            return Err("취소되었습니다.".into());
        }
        match client::probe_health(endpoint, cancel.clone(), Duration::from_secs(2)) {
            Ok(health) if health.ready => return Ok(()),
            Ok(health) => {
                if !health.detail.is_empty() {
                    last = health.detail;
                }
            }
            Err(HttpError::Cancelled) => return Err("취소되었습니다.".into()),
            Err(err) => last = err.to_string(),
        }
        if started.elapsed() >= timeout {
            return Err(format!(
                "{} ({}초 동안 준비되지 않았습니다)",
                last,
                timeout.as_secs()
            ));
        }
        thread::sleep(poll);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testutil::mock::{MockServer, MockSpec};
    use crate::testutil::TempDir;

    fn config(model: &str) -> LaunchConfig {
        LaunchConfig {
            server_path: PathBuf::from("/opt/llama/llama-server"),
            model_path: PathBuf::from(model),
            port: 8788,
            ctx_size: 4096,
            threads: 6,
            gpu_layers: 0,
            alias: "hymt".into(),
        }
    }

    #[test]
    fn builds_a_conservative_llama_cpp_command_line() {
        let args = config("/models/hy.gguf").args();
        assert_eq!(
            args,
            vec![
                "-m",
                "/models/hy.gguf",
                "--host",
                "127.0.0.1",
                "--port",
                "8788",
                "-c",
                "4096",
                "-t",
                "6",
                "-ngl",
                "0",
                "-np",
                "1",
                "--jinja",
                "--alias",
                "hymt"
            ]
            .into_iter()
            .map(String::from)
            .collect::<Vec<_>>()
        );
    }

    #[test]
    fn finds_a_server_next_to_the_executable_or_in_runtime() {
        let dir = TempDir::new("engine-discover");
        assert!(discover_server(dir.path(), None).is_none());

        fs::create_dir_all(dir.path().join("runtime")).unwrap();
        let binary = dir.path().join("runtime").join(SERVER_BINARY);
        fs::write(&binary, b"stub").unwrap();
        assert_eq!(discover_server(dir.path(), None), Some(binary.clone()));

        fs::write(dir.path().join(SERVER_BINARY), b"stub").unwrap();
        assert_eq!(
            discover_server(dir.path(), None),
            Some(dir.path().join(SERVER_BINARY))
        );

        let configured = dir.path().join("elsewhere.exe");
        fs::write(&configured, b"stub").unwrap();
        assert_eq!(
            discover_server(dir.path(), Some(&configured)),
            Some(configured)
        );
    }

    #[test]
    fn scans_models_and_ignores_partial_downloads() {
        let dir = TempDir::new("engine-scan");
        fs::write(dir.path().join("b.gguf"), b"x").unwrap();
        fs::write(dir.path().join("a.gguf"), b"x").unwrap();
        fs::write(dir.path().join("c.gguf.part"), b"x").unwrap();
        fs::write(dir.path().join("readme.txt"), b"x").unwrap();
        let found = scan_gguf(dir.path());
        assert_eq!(found.len(), 2);
        assert!(found[0].ends_with("a.gguf"));
        assert!(found[1].ends_with("b.gguf"));
    }

    #[test]
    fn picks_the_recommended_quantization() {
        let models = vec![
            PathBuf::from("Hy-MT2-7B-Q4_K_M.gguf"),
            PathBuf::from("Hy-MT2-1.8B-Q8_0.gguf"),
            PathBuf::from("Hy-MT2-1.8B-Q4_K_M.gguf"),
        ];
        assert_eq!(
            pick_model(&models),
            Some(PathBuf::from("Hy-MT2-1.8B-Q4_K_M.gguf"))
        );
        assert_eq!(pick_model(&[]), None);
    }

    #[test]
    fn waits_for_health_to_flip_to_ready() {
        let server = MockServer::start(MockSpec::Sequence(vec![
            MockSpec::status(503, "loading"),
            MockSpec::status(503, "loading"),
            MockSpec::json(r#"{"status":"ok"}"#),
        ]));
        let endpoint = Endpoint::new("127.0.0.1", server.addr().port());
        wait_until_ready(
            &endpoint,
            cancel_token(),
            Duration::from_secs(5),
            Duration::from_millis(10),
        )
        .expect("server becomes ready");
    }

    #[test]
    fn wait_times_out_with_a_useful_message() {
        let server = MockServer::start(MockSpec::status(503, "still loading"));
        let endpoint = Endpoint::new("127.0.0.1", server.addr().port());
        let error = wait_until_ready(
            &endpoint,
            cancel_token(),
            Duration::from_millis(120),
            Duration::from_millis(10),
        )
        .unwrap_err();
        assert!(error.contains("준비되지 않았습니다"), "{error}");
    }

    #[test]
    fn supervisor_reports_a_missing_binary() {
        let dir = TempDir::new("engine-supervisor");
        let log = dir.path().join("engine.log");
        let supervisor = Supervisor::spawn(log).unwrap();
        supervisor.start(LaunchConfig {
            server_path: dir.path().join("missing-server.exe"),
            model_path: dir.path().join("model.gguf"),
            port: 8788,
            ctx_size: 2048,
            threads: 2,
            gpu_layers: 0,
            alias: "hymt".into(),
        });
        let mut failure = None;
        for _ in 0..50 {
            match supervisor.recv_timeout(Duration::from_millis(100)) {
                Some(EngineEvent::State(EngineState::Failed(message))) => {
                    failure = Some(message);
                    break;
                }
                Some(_) => {}
                None => break,
            }
        }
        let failure = failure.expect("failure event");
        assert!(failure.contains("llama-server"), "{failure}");
    }
}
