//! The worker supervisor: spawns the resident shim, keeps it warm, bounds
//! every request, and falls into a backoff when anything goes wrong.
//!
//! The engine thread calls [`Supervisor::try_transcribe`], which answers at
//! once with the text, "not ready" (the worker is starting, backing off, or
//! busy), or a failure; it never waits on a cold start. Starting happens on a
//! background thread. Every failure kills the worker's whole process group
//! and starts a bounded, doubling backoff before the next start attempt.
//!
//! Audio reaches the worker only as owner-only WAV files inside a private
//! directory (mode 0700) that is swept at start and cleaned on every path.

use std::fs;
use std::io::{BufRead, BufReader, Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, ChildStdin, Command, Stdio};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::mpsc::{Receiver, RecvTimeoutError, channel};
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::{Duration, Instant};

use super::probe::Runtime;
use super::protocol::{self, MAX_REPLY_LINE, ProtocolError, Reply, Request};

/// The runtime makes its per-request scratch in its cache root under this
/// prefix, and a killed worker leaves it behind.
const RUNTIME_SCRATCH_PREFIX: &str = "parakeet-transcribe-";

/// Clips reach the worker as 16 kHz mono.
const SAMPLE_RATE: u32 = 16_000;

#[derive(Clone, Debug)]
pub(crate) struct Config {
    pub(crate) runtime: Runtime,
    /// A private parent directory; the supervisor works in `<run_dir>/<pid>`.
    pub(crate) run_dir: PathBuf,
    /// The runtime's own per-submit deadline, fixed for the worker's life
    /// (the runtime keys its shared session on it).
    pub(crate) cli_deadline_ms: u64,
    /// The supervisor's timeout for one request: this plus a fraction of the
    /// clip's length.
    pub(crate) request_base: Duration,
    pub(crate) request_per_clip_second: Duration,
    /// How long the cold start (interpreter, session, golden self-test) may take.
    pub(crate) start_timeout: Duration,
    pub(crate) backoff_initial: Duration,
    pub(crate) backoff_cap: Duration,
    /// How long shutdown waits for a polite exit before killing the group.
    pub(crate) shutdown_grace: Duration,
}

impl Config {
    pub(crate) fn new(runtime: Runtime, run_dir: PathBuf) -> Self {
        Self {
            runtime,
            run_dir,
            cli_deadline_ms: 20_000,
            request_base: Duration::from_secs(6),
            request_per_clip_second: Duration::from_millis(250),
            start_timeout: Duration::from_secs(90),
            backoff_initial: Duration::from_secs(5),
            backoff_cap: Duration::from_secs(300),
            shutdown_grace: Duration::from_secs(1),
        }
    }

    fn request_timeout(&self, clip: Duration) -> Duration {
        self.request_base + self.request_per_clip_second.mul_f64(clip.as_secs_f64())
    }

    /// Doubles per consecutive failure up to the cap; `failures` is at least 1.
    pub(crate) fn backoff(&self, failures: u32) -> Duration {
        let doublings = failures.saturating_sub(1).min(20);
        self.backoff_initial
            .saturating_mul(1u32 << doublings)
            .min(self.backoff_cap)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum State {
    /// Not started, or shut down.
    Stopped,
    Starting,
    Ready,
    /// Failed; no start attempt until the backoff elapses.
    Degraded,
}

#[derive(Debug, PartialEq)]
pub(crate) enum Outcome {
    Text {
        text: String,
        pipeline_ms: Option<f64>,
    },
    /// The worker is not ready; run this dictation elsewhere.
    NotReady,
    /// The attempt failed; the worker is now backing off.
    Failed(String),
}

struct Conn {
    child: Child,
    stdin: Option<ChildStdin>,
    replies: Receiver<String>,
    pgid: i32,
    next_id: u64,
}

enum CallError {
    Protocol(ProtocolError),
    Timeout,
    /// The worker exited or its pipe broke.
    Gone,
}

impl Conn {
    fn spawn(config: &Config, shim: &Path, work: &Path) -> std::io::Result<Self> {
        use std::os::unix::process::CommandExt;
        let mut child = Command::new(&config.runtime.interpreter)
            .arg(shim)
            .arg("--cli-script")
            .arg(&config.runtime.cli_script)
            .arg("--work-dir")
            .arg(work)
            .arg("--deadline-ms")
            .arg(config.cli_deadline_ms.to_string())
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .process_group(0)
            .spawn()?;
        let pgid = child.id() as i32;
        let (Some(stdin), Some(stdout)) = (child.stdin.take(), child.stdout.take()) else {
            let _ = child.kill();
            let _ = child.wait();
            return Err(std::io::Error::other("worker pipes missing"));
        };
        let (sender, replies) = channel();
        std::thread::spawn(move || read_lines(stdout, &sender));
        Ok(Self {
            child,
            stdin: Some(stdin),
            replies,
            pgid,
            next_id: 1,
        })
    }

    fn call(
        &mut self,
        timeout: Duration,
        build: impl FnOnce(u64) -> RequestOwned,
    ) -> Result<Reply, CallError> {
        let id = self.next_id;
        self.next_id += 1;
        let request = build(id);
        let line = request.as_request().encode();
        let stdin = self.stdin.as_mut().ok_or(CallError::Gone)?;
        stdin
            .write_all(line.as_bytes())
            .and_then(|()| stdin.flush())
            .map_err(|_| CallError::Gone)?;
        let deadline = Instant::now() + timeout;
        let remaining = deadline.saturating_duration_since(Instant::now());
        match self.replies.recv_timeout(remaining) {
            Ok(line) => protocol::decode(&line, &request.as_request()).map_err(CallError::Protocol),
            Err(RecvTimeoutError::Timeout) => Err(CallError::Timeout),
            Err(RecvTimeoutError::Disconnected) => Err(CallError::Gone),
        }
    }
}

impl Drop for Conn {
    fn drop(&mut self) {
        kill_group(self.pgid);
        let _ = self.child.wait();
    }
}

fn kill_group(pgid: i32) {
    // SAFETY: signalling a process group this supervisor created.
    unsafe { libc::kill(-pgid, libc::SIGKILL) };
}

fn read_lines(stdout: impl Read, sender: &std::sync::mpsc::Sender<String>) {
    let mut reader = BufReader::new(stdout);
    loop {
        let mut line = String::new();
        let read = reader
            .by_ref()
            .take(MAX_REPLY_LINE as u64 + 1)
            .read_line(&mut line);
        match read {
            Ok(0) | Err(_) => return,
            Ok(_) => {
                let over = line.len() > MAX_REPLY_LINE;
                if sender.send(line.trim_end().to_string()).is_err() || over {
                    return;
                }
            }
        }
    }
}

enum RequestOwned {
    Hello(u64),
    Shutdown(u64),
    Transcribe(u64, PathBuf, u64),
}

impl RequestOwned {
    fn as_request(&self) -> Request<'_> {
        match self {
            Self::Hello(id) => Request::Hello { id: *id },
            Self::Shutdown(id) => Request::Shutdown { id: *id },
            Self::Transcribe(id, wav, deadline_ms) => Request::Transcribe {
                id: *id,
                wav,
                deadline_ms: *deadline_ms,
            },
        }
    }
}

struct Inner {
    state: State,
    /// Idle connection; taken while a request is in flight.
    conn: Option<Conn>,
    /// The live worker's group, kept so a shutdown can cancel a request.
    pgid: Option<i32>,
    failures: u32,
    retry_at: Option<Instant>,
}

struct Shared {
    config: Config,
    work: PathBuf,
    shim: PathBuf,
    inner: Mutex<Inner>,
    stopped: AtomicBool,
    request_seq: AtomicU64,
}

impl Shared {
    fn lock(&self) -> MutexGuard<'_, Inner> {
        self.inner
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    fn fail(&self, inner: &mut Inner) {
        inner.failures = inner.failures.saturating_add(1);
        inner.retry_at = Some(Instant::now() + self.config.backoff(inner.failures));
        inner.state = State::Degraded;
        if let Some(pgid) = inner.pgid.take() {
            kill_group(pgid);
        }
        inner.conn = None;
    }
}

pub(crate) struct Supervisor {
    shared: Arc<Shared>,
}

impl Supervisor {
    /// Prepare the private directory, sweep what dead runs left behind, and
    /// begin starting the worker in the background.
    pub(crate) fn start(config: Config) -> std::io::Result<Self> {
        sweep_stale(&config.run_dir);
        let own = config.run_dir.join(std::process::id().to_string());
        let _ = fs::remove_dir_all(&own);
        make_private_dir(&own)?;
        let work = own.join("work");
        make_private_dir(&work)?;
        let shim = own.join("ane_worker.py");
        write_private(&shim, protocol::SHIM_SOURCE.as_bytes())?;
        let shared = Arc::new(Shared {
            config,
            work,
            shim,
            inner: Mutex::new(Inner {
                state: State::Stopped,
                conn: None,
                pgid: None,
                failures: 0,
                retry_at: None,
            }),
            stopped: AtomicBool::new(false),
            request_seq: AtomicU64::new(1),
        });
        begin_start(&shared);
        Ok(Self { shared })
    }

    pub(crate) fn state(&self) -> State {
        self.shared.lock().state
    }

    /// The worker's process id while one is alive.
    pub(crate) fn worker_pid(&self) -> Option<u32> {
        self.shared.lock().pgid.map(|pgid| pgid as u32)
    }

    pub(crate) fn try_transcribe(&self, samples: &[f32]) -> Outcome {
        let shared = &self.shared;
        let mut conn = {
            let mut inner = shared.lock();
            if shared.stopped.load(Ordering::SeqCst) {
                return Outcome::NotReady;
            }
            match inner.state {
                State::Ready => {}
                State::Degraded if inner.retry_at.is_none_or(|at| Instant::now() >= at) => {
                    drop(inner);
                    begin_start(shared);
                    return Outcome::NotReady;
                }
                _ => return Outcome::NotReady,
            }
            match inner.conn.take() {
                Some(conn) => conn,
                None => return Outcome::NotReady,
            }
        };
        let scratch_before = runtime_scratch(&shared.config);
        let clip = Duration::from_secs_f64(samples.len() as f64 / f64::from(SAMPLE_RATE));
        let timeout = shared.config.request_timeout(clip);
        let wav = shared.work.join(format!(
            "clip-{}.wav",
            shared.request_seq.fetch_add(1, Ordering::SeqCst)
        ));
        let result = match write_wav(&wav, samples) {
            Ok(()) => conn.call(timeout, |id| {
                RequestOwned::Transcribe(id, wav.clone(), timeout.as_millis() as u64)
            }),
            Err(_) => {
                let _ = fs::remove_file(&wav);
                let mut inner = shared.lock();
                inner.conn = Some(conn);
                return Outcome::Failed("could not write the clip".to_string());
            }
        };
        let _ = fs::remove_file(&wav);
        let mut inner = shared.lock();
        match result {
            Ok(Reply::Transcript { text, pipeline_ms })
                if !shared.stopped.load(Ordering::SeqCst) =>
            {
                inner.failures = 0;
                inner.retry_at = None;
                inner.conn = Some(conn);
                Outcome::Text { text, pipeline_ms }
            }
            Ok(Reply::Transcript { .. }) => Outcome::NotReady,
            Ok(Reply::Failed { code, message }) => {
                inner.conn = Some(conn);
                shared.fail(&mut inner);
                sweep_runtime_scratch(&shared.config, &scratch_before);
                Outcome::Failed(format!("{code}: {message}"))
            }
            Ok(_) => {
                inner.conn = Some(conn);
                shared.fail(&mut inner);
                sweep_runtime_scratch(&shared.config, &scratch_before);
                Outcome::Failed("unexpected reply".to_string())
            }
            Err(error) => {
                let why = match error {
                    CallError::Timeout => "timed out".to_string(),
                    CallError::Gone => "the worker exited".to_string(),
                    CallError::Protocol(error) => error.to_string(),
                };
                inner.conn = Some(conn);
                shared.fail(&mut inner);
                sweep_runtime_scratch(&shared.config, &scratch_before);
                if shared.stopped.load(Ordering::SeqCst) {
                    return Outcome::NotReady;
                }
                Outcome::Failed(why)
            }
        }
    }

    /// Stop the worker: ask politely, then kill its group. Safe to call twice.
    pub(crate) fn shutdown(&self) {
        let shared = &self.shared;
        shared.stopped.store(true, Ordering::SeqCst);
        let (conn, pgid) = {
            let mut inner = shared.lock();
            inner.state = State::Stopped;
            (inner.conn.take(), inner.pgid.take())
        };
        if let Some(mut conn) = conn {
            let _ = conn.call(shared.config.shutdown_grace, RequestOwned::Shutdown);
            // Closing stdin is the lease; the shim exits on EOF either way.
            conn.stdin = None;
            let started = Instant::now();
            while started.elapsed() < shared.config.shutdown_grace {
                if matches!(conn.child.try_wait(), Ok(Some(_))) {
                    break;
                }
                std::thread::sleep(Duration::from_millis(10));
            }
            drop(conn);
        } else if let Some(pgid) = pgid {
            // A request is in flight on another thread: cancel it.
            kill_group(pgid);
        }
        let own = shared.work.parent().map(Path::to_path_buf);
        if let Some(own) = own {
            let _ = fs::remove_dir_all(own);
        }
    }
}

impl Drop for Supervisor {
    fn drop(&mut self) {
        self.shutdown();
    }
}

/// Begin a start attempt on a background thread.
fn begin_start(shared: &Arc<Shared>) {
    {
        let mut inner = shared.lock();
        if matches!(inner.state, State::Starting) || shared.stopped.load(Ordering::SeqCst) {
            return;
        }
        inner.state = State::Starting;
    }
    let shared = Arc::clone(shared);
    std::thread::spawn(move || {
        let started =
            Conn::spawn(&shared.config, &shared.shim, &shared.work).and_then(|mut conn| {
                let pgid = conn.pgid;
                match conn.call(shared.config.start_timeout, RequestOwned::Hello) {
                    Ok(Reply::Hello { .. }) => Ok((conn, pgid)),
                    Ok(Reply::Failed { code, message }) => {
                        Err(std::io::Error::other(format!("{code}: {message}")))
                    }
                    Ok(_) => Err(std::io::Error::other("unexpected hello reply")),
                    Err(CallError::Timeout) => Err(std::io::Error::other("start timed out")),
                    Err(CallError::Gone) => Err(std::io::Error::other("worker exited at start")),
                    Err(CallError::Protocol(error)) => {
                        Err(std::io::Error::other(error.to_string()))
                    }
                }
            });
        let mut inner = shared.lock();
        if shared.stopped.load(Ordering::SeqCst) {
            return;
        }
        match started {
            Ok((conn, pgid)) => {
                inner.pgid = Some(pgid);
                inner.conn = Some(conn);
                inner.state = State::Ready;
            }
            Err(_) => shared.fail(&mut inner),
        }
    });
}

fn runtime_scratch(config: &Config) -> Vec<PathBuf> {
    let Some(root) = &config.runtime.cache_root else {
        return Vec::new();
    };
    fs::read_dir(root)
        .into_iter()
        .flatten()
        .flatten()
        .filter(|entry| {
            entry
                .file_name()
                .to_string_lossy()
                .starts_with(RUNTIME_SCRATCH_PREFIX)
        })
        .map(|entry| entry.path())
        .collect()
}

/// Remove the runtime scratch a killed request left: what exists now that
/// did not exist before the request.
fn sweep_runtime_scratch(config: &Config, before: &[PathBuf]) {
    for path in runtime_scratch(config) {
        if !before.contains(&path) {
            let _ = fs::remove_dir_all(path);
        }
    }
}

fn make_private_dir(path: &Path) -> std::io::Result<()> {
    use std::os::unix::fs::{DirBuilderExt, PermissionsExt};
    fs::DirBuilder::new()
        .recursive(true)
        .mode(0o700)
        .create(path)?;
    fs::set_permissions(path, fs::Permissions::from_mode(0o700))
}

fn write_private(path: &Path, bytes: &[u8]) -> std::io::Result<()> {
    use std::os::unix::fs::OpenOptionsExt;
    let mut file = fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(path)?;
    file.write_all(bytes)
}

/// A 16 kHz mono PCM16 WAV, owner-only.
fn write_wav(path: &Path, samples: &[f32]) -> std::io::Result<()> {
    let data_len = (samples.len() * 2) as u32;
    let mut bytes = Vec::with_capacity(44 + samples.len() * 2);
    bytes.extend_from_slice(b"RIFF");
    bytes.extend_from_slice(&(36 + data_len).to_le_bytes());
    bytes.extend_from_slice(b"WAVEfmt ");
    bytes.extend_from_slice(&16u32.to_le_bytes());
    bytes.extend_from_slice(&1u16.to_le_bytes()); // PCM
    bytes.extend_from_slice(&1u16.to_le_bytes()); // mono
    bytes.extend_from_slice(&SAMPLE_RATE.to_le_bytes());
    bytes.extend_from_slice(&(SAMPLE_RATE * 2).to_le_bytes());
    bytes.extend_from_slice(&2u16.to_le_bytes());
    bytes.extend_from_slice(&16u16.to_le_bytes());
    bytes.extend_from_slice(b"data");
    bytes.extend_from_slice(&data_len.to_le_bytes());
    for sample in samples {
        let value = (sample.clamp(-1.0, 1.0) * f32::from(i16::MAX)) as i16;
        bytes.extend_from_slice(&value.to_le_bytes());
    }
    write_private(path, &bytes)
}

fn pid_alive(pid: i32) -> bool {
    // SAFETY: signal 0 only probes for existence.
    let ok = unsafe { libc::kill(pid, 0) } == 0;
    ok || std::io::Error::last_os_error().raw_os_error() == Some(libc::EPERM)
}

/// Remove per-process directories whose process is gone (a crashed run).
fn sweep_stale(run_dir: &Path) {
    let Ok(entries) = fs::read_dir(run_dir) else {
        return;
    };
    for entry in entries.flatten() {
        let name = entry.file_name();
        let Some(pid) = name.to_str().and_then(|name| name.parse::<i32>().ok()) else {
            continue;
        };
        if pid != std::process::id() as i32 && !pid_alive(pid) {
            let _ = fs::remove_dir_all(entry.path());
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const STUB: &str = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/tests/fixtures/ane/stub_cli.py"
    );

    fn python_available() -> bool {
        let found = Command::new("python3")
            .arg("--version")
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .is_ok_and(|status| status.success());
        if !found {
            eprintln!("SKIP: no python3 on this machine; the supervisor tests did not run");
        }
        found
    }

    fn config(name: &str) -> Config {
        let run_dir =
            std::env::temp_dir().join(format!("wordink-ane-sup-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&run_dir);
        let mut config = Config::new(
            Runtime {
                interpreter: PathBuf::from("python3"),
                cli_script: PathBuf::from(STUB),
                cache_root: None,
            },
            run_dir,
        );
        config.cli_deadline_ms = 1_000;
        config.request_base = Duration::from_secs(20);
        config.start_timeout = Duration::from_secs(30);
        config.backoff_initial = Duration::from_millis(100);
        config.backoff_cap = Duration::from_millis(400);
        config.shutdown_grace = Duration::from_millis(300);
        config
    }

    fn wait_for(supervisor: &Supervisor, want: State) {
        let started = Instant::now();
        while supervisor.state() != want {
            assert!(
                started.elapsed() < Duration::from_secs(30),
                "stuck in {:?}",
                supervisor.state()
            );
            std::thread::sleep(Duration::from_millis(10));
        }
    }

    fn clip(seconds: f64) -> Vec<f32> {
        vec![0.1; (seconds * f64::from(SAMPLE_RATE)) as usize]
    }

    /// The stub picks its behavior from the clip's name plus a `mode` file in
    /// the clip's directory; tests set the mode through that file.
    fn set_mode(supervisor: &Supervisor, mode: &str) {
        fs::write(supervisor.shared.work.join("mode"), mode).unwrap();
    }

    fn alive(pid: u32) -> bool {
        pid_alive(pid as i32)
    }

    #[test]
    fn start_reaches_ready_and_a_clip_round_trips() {
        if !python_available() {
            return;
        }
        let supervisor = Supervisor::start(config("ready")).unwrap();
        assert!(matches!(supervisor.state(), State::Starting | State::Ready));
        wait_for(&supervisor, State::Ready);
        let outcome = supervisor.try_transcribe(&clip(1.0));
        assert!(
            matches!(&outcome, Outcome::Text { text, .. } if text.starts_with("He hoped")),
            "{outcome:?}"
        );
        let again = supervisor.try_transcribe(&clip(2.0));
        assert!(matches!(again, Outcome::Text { .. }));
    }

    #[test]
    fn a_request_while_starting_returns_not_ready_at_once() {
        if !python_available() {
            return;
        }
        let supervisor = Supervisor::start(config("starting")).unwrap();
        let started = Instant::now();
        let outcome = supervisor.try_transcribe(&clip(1.0));
        // The interpreter takes tens of milliseconds to come up; either way
        // the call itself must not wait for it.
        if supervisor.state() != State::Ready {
            assert_eq!(outcome, Outcome::NotReady);
        }
        assert!(started.elapsed() < Duration::from_secs(1));
    }

    #[test]
    fn backoff_doubles_up_to_its_cap() {
        let config = config("backoff");
        let delays: Vec<_> = (1..=5).map(|n| config.backoff(n).as_millis()).collect();
        assert_eq!(delays, vec![100, 200, 400, 400, 400]);
        assert_eq!(config.backoff(u32::MAX), config.backoff_cap);
    }

    #[test]
    fn stale_directories_of_dead_runs_are_swept_at_start() {
        if !python_available() {
            return;
        }
        let config = config("stale");
        let dead = config.run_dir.join(i32::MAX.to_string());
        fs::create_dir_all(dead.join("work")).unwrap();
        fs::write(dead.join("work/clip-1.wav"), b"x").unwrap();
        let other = config.run_dir.join("not-a-pid");
        fs::create_dir_all(&other).unwrap();
        let run_dir = config.run_dir.clone();
        let supervisor = Supervisor::start(config).unwrap();
        assert!(!dead.exists());
        assert!(
            other.exists(),
            "directories that are not pids are left alone"
        );
        drop(supervisor);
        let _ = fs::remove_dir_all(run_dir);
    }

    fn fast_timeout(config: &mut Config) {
        config.request_base = Duration::from_millis(600);
        config.request_per_clip_second = Duration::ZERO;
    }

    #[test]
    fn a_hanging_worker_is_killed_at_its_deadline_and_degrades() {
        if !python_available() {
            return;
        }
        let mut config = config("hang");
        fast_timeout(&mut config);
        let supervisor = Supervisor::start(config).unwrap();
        wait_for(&supervisor, State::Ready);
        let pid = supervisor.worker_pid().unwrap();
        set_mode(&supervisor, "hang");
        let started = Instant::now();
        let outcome = supervisor.try_transcribe(&clip(1.0));
        assert!(
            matches!(&outcome, Outcome::Failed(why) if why.contains("timed out")),
            "{outcome:?}"
        );
        assert!(started.elapsed() < Duration::from_secs(10));
        assert_eq!(supervisor.state(), State::Degraded);
        assert!(!alive(pid), "the hung worker's process is still alive");
        assert!(
            supervisor
                .shared
                .work
                .read_dir()
                .unwrap()
                .flatten()
                .all(|e| e.file_name() != "clip-1.wav")
        );
    }

    #[test]
    fn a_crash_mid_request_fails_the_request_and_degrades() {
        if !python_available() {
            return;
        }
        let supervisor = Supervisor::start(config("crash")).unwrap();
        wait_for(&supervisor, State::Ready);
        set_mode(&supervisor, "crash");
        let outcome = supervisor.try_transcribe(&clip(1.0));
        assert!(matches!(outcome, Outcome::Failed(_)), "{outcome:?}");
        assert_eq!(supervisor.state(), State::Degraded);
    }

    #[test]
    fn degraded_waits_out_its_backoff_then_restarts_and_success_resets_it() {
        if !python_available() {
            return;
        }
        let supervisor = Supervisor::start(config("recover")).unwrap();
        wait_for(&supervisor, State::Ready);
        set_mode(&supervisor, "fail");
        assert!(matches!(
            supervisor.try_transcribe(&clip(1.0)),
            Outcome::Failed(_)
        ));
        assert_eq!(supervisor.state(), State::Degraded);
        assert_eq!(supervisor.shared.lock().failures, 1);
        // Inside the backoff no start is attempted.
        assert_eq!(supervisor.try_transcribe(&clip(1.0)), Outcome::NotReady);
        assert_eq!(supervisor.state(), State::Degraded);
        // After it, the next request triggers a start and the worker returns.
        std::thread::sleep(Duration::from_millis(150));
        set_mode(&supervisor, "");
        assert_eq!(supervisor.try_transcribe(&clip(1.0)), Outcome::NotReady);
        wait_for(&supervisor, State::Ready);
        assert!(matches!(
            supervisor.try_transcribe(&clip(1.0)),
            Outcome::Text { .. }
        ));
        assert_eq!(supervisor.shared.lock().failures, 0);
    }

    #[test]
    fn a_failed_start_degrades_with_backoff() {
        if !python_available() {
            return;
        }
        let mut config = config("badstart");
        config.runtime.cli_script = PathBuf::from("/nonexistent/cli");
        let supervisor = Supervisor::start(config).unwrap();
        wait_for(&supervisor, State::Degraded);
        assert_eq!(supervisor.try_transcribe(&clip(1.0)), Outcome::NotReady);
        assert!(supervisor.shared.lock().failures >= 1);
    }

    #[test]
    fn a_missing_interpreter_degrades_without_panicking() {
        let mut config = config("nopython");
        config.runtime.interpreter = PathBuf::from("/nonexistent/python");
        let supervisor = Supervisor::start(config).unwrap();
        wait_for(&supervisor, State::Degraded);
    }

    #[test]
    fn shutdown_while_a_request_is_in_flight_cancels_it() {
        if !python_available() {
            return;
        }
        let mut config = config("cancel");
        config.request_base = Duration::from_secs(60);
        let supervisor = Arc::new(Supervisor::start(config).unwrap());
        wait_for(&supervisor, State::Ready);
        let pid = supervisor.worker_pid().unwrap();
        set_mode(&supervisor, "hang");
        let requester = {
            let supervisor = Arc::clone(&supervisor);
            std::thread::spawn(move || supervisor.try_transcribe(&clip(1.0)))
        };
        std::thread::sleep(Duration::from_millis(500));
        supervisor.shutdown();
        let outcome = requester.join().unwrap();
        assert_eq!(outcome, Outcome::NotReady);
        assert!(!alive(pid));
    }

    fn rss_mb(pid: u32) -> f64 {
        let status = fs::read_to_string(format!("/proc/{pid}/status")).unwrap_or_default();
        status
            .lines()
            .find_map(|line| line.strip_prefix("VmRSS:"))
            .and_then(|rest| rest.split_whitespace().next()?.parse::<f64>().ok())
            .map_or(0.0, |kb| kb / 1024.0)
    }

    /// Manual measurement on the target hardware, not CI: pushes real clips
    /// through the installed runtime with the real supervisor and prints the
    /// wall-clock round trip, the runtime's own pipeline time, resident
    /// memory, and whether the worker respawned between clips.
    ///
    /// `WORDINK_ANE_SPIKE_CLIPS=a.wav,b.wav cargo test --bin wordink
    /// ane::worker::tests::spike -- --ignored --nocapture`
    #[test]
    #[ignore = "needs the real runtime and Neural Engine; run by hand"]
    fn spike_warm_session() {
        let clips = std::env::var("WORDINK_ANE_SPIKE_CLIPS").expect("WORDINK_ANE_SPIKE_CLIPS");
        let probe = super::super::probe::probe();
        eprintln!("probe: {:?}", probe.verdict);
        let runtime = probe.runtime.expect("a runtime");
        let run_dir =
            std::env::temp_dir().join(format!("wordink-ane-spike-{}", std::process::id()));
        let started = Instant::now();
        let supervisor = Supervisor::start(Config::new(runtime, run_dir.clone())).unwrap();
        wait_for(&supervisor, State::Ready);
        let first_pid = supervisor.worker_pid().unwrap();
        eprintln!(
            "cold start to Ready: {:.2}s, rss {:.0} MB",
            started.elapsed().as_secs_f64(),
            rss_mb(first_pid)
        );
        for path in clips.split(',') {
            let samples: Vec<f32> = hound::WavReader::open(path)
                .unwrap()
                .into_samples::<i16>()
                .map(|s| f32::from(s.unwrap()) / f32::from(i16::MAX))
                .collect();
            let seconds = samples.len() as f64 / 16_000.0;
            let at = Instant::now();
            let outcome = supervisor.try_transcribe(&samples);
            let wall = at.elapsed().as_secs_f64();
            let pid = supervisor.worker_pid();
            match outcome {
                Outcome::Text { text, pipeline_ms } => eprintln!(
                    "clip {seconds:6.1}s  wall {wall:6.3}s  pipeline {:>8.1}ms  overhead {:5.0}ms  rtf {:5.1}x  rss {:5.0}MB  respawned {}  chars {}",
                    pipeline_ms.unwrap_or(0.0),
                    wall * 1000.0 - pipeline_ms.unwrap_or(0.0),
                    seconds / wall,
                    pid.map_or(0.0, rss_mb),
                    pid != Some(first_pid),
                    text.len()
                ),
                other => eprintln!("clip {seconds:.1}s -> {other:?}"),
            }
        }
        supervisor.shutdown();
        let _ = fs::remove_dir_all(run_dir);
    }

    #[test]
    fn only_scratch_made_during_a_killed_request_is_swept() {
        let mut config = config("scratch");
        let root = config.run_dir.join("cache");
        config.runtime.cache_root = Some(root.clone());
        fs::create_dir_all(root.join("parakeet-transcribe-old")).unwrap();
        fs::create_dir_all(root.join("model-cache")).unwrap();
        let before = runtime_scratch(&config);
        fs::create_dir_all(root.join("parakeet-transcribe-new/inner")).unwrap();
        sweep_runtime_scratch(&config, &before);
        assert!(root.join("parakeet-transcribe-old").exists());
        assert!(root.join("model-cache").exists());
        assert!(!root.join("parakeet-transcribe-new").exists());
        let _ = fs::remove_dir_all(&config.run_dir);
    }

    #[test]
    fn shutdown_leaves_no_child_and_no_files() {
        if !python_available() {
            return;
        }
        let config = config("shutdown");
        let run_dir = config.run_dir.clone();
        let supervisor = Supervisor::start(config).unwrap();
        wait_for(&supervisor, State::Ready);
        let pid = supervisor.worker_pid().unwrap();
        supervisor.shutdown();
        assert_eq!(supervisor.state(), State::Stopped);
        assert!(!alive(pid), "worker still alive");
        let own = run_dir.join(std::process::id().to_string());
        assert!(!own.exists(), "private directory left behind");
        assert_eq!(supervisor.try_transcribe(&clip(1.0)), Outcome::NotReady);
        let _ = fs::remove_dir_all(run_dir);
    }
}
