//! Can the Neural Engine engine exist on this machine, and if not, why not.
//!
//! The probe is cheap and never loads a model. In order it checks the target
//! platform, the device node, the runtime's launcher (and the interpreter and
//! CLI script that launcher names), the runtime's own `mlx-omarchy-info
//! --json` Neural Engine fields, and whether the runtime's reference model is
//! prepared. It blocks on a short subprocess, so callers run it off the UI
//! thread; [`cached`] remembers the verdict for a short while.

use std::io::Read;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::{Duration, Instant};

use serde::Deserialize;

/// The device node the Neural Engine driver exposes.
const DEVICE_NODE: &str = "/dev/accel/accel0";
/// The runtime's launcher, as installed.
const LAUNCHER: &str = "mlx-omarchy-parakeet";
/// The runtime's hardware report tool.
const INFO_TOOL: &str = "mlx-omarchy-info";
/// Environment override: the launcher's full path.
pub(crate) const RUNTIME_ENV: &str = "WORDINK_ANE_RUNTIME";
/// Environment override: the interpreter that runs the CLI script.
pub(crate) const PYTHON_ENV: &str = "WORDINK_ANE_PYTHON";
/// How long a verdict is reused.
pub(crate) const TTL: Duration = Duration::from_secs(30);
/// The info tool answers in milliseconds; a hang is a failure.
const INFO_TIMEOUT: Duration = Duration::from_secs(5);
/// Where the reference's pinned fixture sits inside a prepared cache entry:
/// `<cache root>/<org>/<model>/<revision>/audio/fixture.flac`.
const PREPARED_MARKER: [&str; 2] = ["audio", "fixture.flac"];

/// The installed runtime, as the worker needs to spawn it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Runtime {
    /// The interpreter that runs the CLI script (spawned directly, no shell).
    pub(crate) interpreter: PathBuf,
    /// The CLI script the launcher runs.
    pub(crate) cli_script: PathBuf,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum Verdict {
    /// Cannot work here; the reason is for the card.
    Unavailable(String),
    /// The runtime works but its reference model is not downloaded yet.
    NotPrepared,
    Available,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Probe {
    pub(crate) verdict: Verdict,
    /// Present for `NotPrepared` and `Available`.
    pub(crate) runtime: Option<Runtime>,
}

impl Probe {
    fn unavailable(reason: impl Into<String>) -> Self {
        Self {
            verdict: Verdict::Unavailable(reason.into()),
            runtime: None,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum DeviceAccess {
    Missing,
    NotWritable,
    Usable,
}

/// Everything the probe reads from the machine, so tests can supply it.
pub(crate) struct Inputs {
    pub(crate) linux_aarch64: bool,
    pub(crate) runtime_override: Option<PathBuf>,
    pub(crate) python_override: Option<PathBuf>,
    pub(crate) path_dirs: Vec<PathBuf>,
    pub(crate) home: Option<PathBuf>,
    pub(crate) cache_root: Option<PathBuf>,
    pub(crate) device_node: PathBuf,
}

impl Inputs {
    pub(crate) fn from_host() -> Self {
        let var = |name: &str| std::env::var_os(name).filter(|value| !value.is_empty());
        let home = dirs::home_dir();
        let cache_root = var("MLX_OMARCHY_CACHE_DIR")
            .map(|base| PathBuf::from(base).join("parakeet-reference"))
            .or_else(|| {
                var("XDG_CACHE_HOME")
                    .map(|base| PathBuf::from(base).join("mlx-omarchy/parakeet-reference"))
            })
            .or_else(|| {
                home.as_ref()
                    .map(|home| home.join(".cache/mlx-omarchy/parakeet-reference"))
            });
        Self {
            linux_aarch64: cfg!(all(target_os = "linux", target_arch = "aarch64")),
            runtime_override: var(RUNTIME_ENV).map(PathBuf::from),
            python_override: var(PYTHON_ENV).map(PathBuf::from),
            path_dirs: var("PATH")
                .map(|path| std::env::split_paths(&path).collect())
                .unwrap_or_default(),
            home,
            cache_root,
            device_node: PathBuf::from(DEVICE_NODE),
        }
    }
}

/// Probe this machine.
pub(crate) fn probe() -> Probe {
    probe_with(&Inputs::from_host(), &run_info)
}

/// The verdict, reused for [`TTL`].
pub(crate) fn cached() -> Probe {
    static CACHE: Mutex<Option<(Instant, Probe)>> = Mutex::new(None);
    let mut slot = CACHE
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    reuse_or_probe(&mut slot, Instant::now(), probe)
}

fn reuse_or_probe(
    slot: &mut Option<(Instant, Probe)>,
    now: Instant,
    compute: impl FnOnce() -> Probe,
) -> Probe {
    if let Some((at, probe)) = slot.as_ref()
        && now.saturating_duration_since(*at) < TTL
    {
        return probe.clone();
    }
    let fresh = compute();
    *slot = Some((now, fresh.clone()));
    fresh
}

/// `run_info` runs the info tool and returns its stdout.
pub(crate) fn probe_with(
    inputs: &Inputs,
    run_info: &dyn Fn(&Path) -> Result<String, String>,
) -> Probe {
    if !inputs.linux_aarch64 {
        return Probe::unavailable("only on Linux on Apple silicon");
    }
    match device_access(&inputs.device_node) {
        DeviceAccess::Missing => {
            return Probe::unavailable(
                "no Neural Engine device found (the Neural Engine driver is not loaded)",
            );
        }
        DeviceAccess::NotWritable => {
            return Probe::unavailable(
                "the Neural Engine device is not accessible to this user (check its group)",
            );
        }
        DeviceAccess::Usable => {}
    }
    let Some(launcher) = find_launcher(inputs) else {
        return Probe::unavailable("the mlx-omarchy runtime was not found");
    };
    let Some(runtime) = read_launcher(&launcher, inputs.python_override.as_deref()) else {
        return Probe::unavailable("the mlx-omarchy runtime launcher was not understood");
    };
    if !runtime.cli_script.is_file() {
        return Probe::unavailable("the mlx-omarchy runtime is incomplete (CLI script missing)");
    }
    let info_tool = launcher
        .parent()
        .map(|dir| dir.join(INFO_TOOL))
        .filter(|path| path.is_file())
        .or_else(|| find_on(&inputs.path_dirs, INFO_TOOL));
    let Some(info_tool) = info_tool else {
        return Probe::unavailable("the mlx-omarchy runtime has no info tool");
    };
    let info = match run_info(&info_tool) {
        Ok(text) => text,
        Err(error) => {
            return Probe::unavailable(format!("the runtime's hardware report failed: {error}"));
        }
    };
    if let Some(reason) = neural_engine_problem(&info) {
        return Probe::unavailable(reason);
    }
    let verdict = if inputs.cache_root.as_deref().is_some_and(is_prepared) {
        Verdict::Available
    } else {
        Verdict::NotPrepared
    };
    Probe {
        verdict,
        runtime: Some(runtime),
    }
}

#[cfg(unix)]
fn device_access(path: &Path) -> DeviceAccess {
    use std::os::unix::ffi::OsStrExt;
    if std::fs::metadata(path).is_err() {
        return DeviceAccess::Missing;
    }
    let Ok(c_path) = std::ffi::CString::new(path.as_os_str().as_bytes()) else {
        return DeviceAccess::Missing;
    };
    // SAFETY: `c_path` is a valid NUL-terminated string for the call.
    let ok = unsafe { libc::access(c_path.as_ptr(), libc::R_OK | libc::W_OK) } == 0;
    if ok {
        DeviceAccess::Usable
    } else {
        DeviceAccess::NotWritable
    }
}

#[cfg(not(unix))]
fn device_access(_path: &Path) -> DeviceAccess {
    DeviceAccess::Missing
}

fn find_on(dirs: &[PathBuf], name: &str) -> Option<PathBuf> {
    dirs.iter()
        .map(|dir| dir.join(name))
        .find(|candidate| candidate.is_file())
}

/// Override, then `PATH`, then the runtime's usual per-user install location.
fn find_launcher(inputs: &Inputs) -> Option<PathBuf> {
    if let Some(path) = &inputs.runtime_override {
        return path.is_file().then(|| path.clone());
    }
    find_on(&inputs.path_dirs, LAUNCHER).or_else(|| {
        inputs
            .home
            .as_ref()
            .map(|home| home.join(".local/bin").join(LAUNCHER))
            .filter(|path| path.is_file())
    })
}

/// The launcher is a shell script whose `exec <interpreter> <script> "$@"`
/// line names what to run. The interpreter is taken from there (or the
/// override) and spawned directly; no shell and no `PATH` lookup of `python`.
fn read_launcher(launcher: &Path, python_override: Option<&Path>) -> Option<Runtime> {
    let text = std::fs::read_to_string(launcher).ok()?;
    let line = text
        .lines()
        .map(str::trim)
        .find_map(|line| line.strip_prefix("exec "))?;
    let mut words = line
        .split_whitespace()
        .map(|word| word.trim_matches(['"', '\'']));
    let named_interpreter = PathBuf::from(words.next()?);
    let cli_script = PathBuf::from(words.next()?);
    if !named_interpreter.is_absolute() || !cli_script.is_absolute() {
        return None;
    }
    let interpreter = match python_override {
        Some(path) if path.is_absolute() => path.to_path_buf(),
        Some(_) => return None,
        None => named_interpreter,
    };
    Some(Runtime {
        interpreter,
        cli_script,
    })
}

#[derive(Deserialize)]
struct Info {
    ane: Option<AneInfo>,
}

#[derive(Default, Deserialize)]
struct AneInfo {
    #[serde(default)]
    fdt_node: u8,
    #[serde(default)]
    accel0: u8,
    #[serde(default)]
    module_present: u8,
    #[serde(default)]
    available: u8,
}

/// Why the runtime says the Neural Engine cannot be used, or `None`.
fn neural_engine_problem(info_json: &str) -> Option<String> {
    let Ok(info) = serde_json::from_str::<Info>(info_json) else {
        return Some("the runtime's hardware report was not understood".to_string());
    };
    let ane = info.ane.unwrap_or_default();
    if ane.available == 1 {
        return None;
    }
    let why = if ane.fdt_node == 0 {
        "this machine's device tree has no Neural Engine"
    } else if ane.module_present == 0 {
        "the Neural Engine driver module is not loaded"
    } else if ane.accel0 == 0 {
        "the Neural Engine device is missing"
    } else {
        "the runtime reports the Neural Engine unavailable"
    };
    Some(format!("no usable Neural Engine: {why}"))
}

/// The runtime's reference model is downloaded: some cache entry holds the
/// pinned fixture.
fn is_prepared(cache_root: &Path) -> bool {
    let subdirs = |dir: &Path| -> Vec<PathBuf> {
        std::fs::read_dir(dir)
            .into_iter()
            .flatten()
            .flatten()
            .map(|entry| entry.path())
            .filter(|path| path.is_dir())
            .collect()
    };
    subdirs(cache_root)
        .iter()
        .flat_map(|org| subdirs(org))
        .flat_map(|model| subdirs(&model))
        .any(|revision| {
            PREPARED_MARKER
                .iter()
                .fold(revision, |p, part| p.join(part))
                .is_file()
        })
}

fn run_info(tool: &Path) -> Result<String, String> {
    use std::process::{Command, Stdio};
    let mut child = Command::new(tool)
        .arg("--json")
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|error| error.to_string())?;
    let mut stdout = child.stdout.take().ok_or("no output pipe")?;
    let reader = std::thread::spawn(move || {
        let mut text = String::new();
        stdout.read_to_string(&mut text).map(|_| text)
    });
    let started = Instant::now();
    loop {
        match child.try_wait() {
            Ok(Some(status)) if status.success() => break,
            Ok(Some(status)) => return Err(format!("exited with {status}")),
            Ok(None) if started.elapsed() < INFO_TIMEOUT => {
                std::thread::sleep(Duration::from_millis(10));
            }
            Ok(None) => {
                let _ = child.kill();
                let _ = child.wait();
                return Err("timed out".to_string());
            }
            Err(error) => return Err(error.to_string()),
        }
    }
    reader
        .join()
        .map_err(|_| "reader panicked".to_string())?
        .map_err(|error| error.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::Cell;
    use std::fs;

    const READY: &str = include_str!("../../../tests/fixtures/ane/info-ready.json");
    const NO_ANE: &str = include_str!("../../../tests/fixtures/ane/info-no-ane.json");

    /// A fake runtime install under a scratch directory.
    struct Rig {
        root: PathBuf,
    }

    impl Rig {
        fn new(name: &str) -> Self {
            let root = std::env::temp_dir()
                .join(format!("wordink-ane-probe-{name}-{}", std::process::id()));
            let _ = fs::remove_dir_all(&root);
            fs::create_dir_all(root.join("bin")).unwrap();
            fs::create_dir_all(root.join("share")).unwrap();
            fs::write(root.join("share/cli"), "#!/usr/bin/env python3\n").unwrap();
            fs::write(
                root.join("bin").join(LAUNCHER),
                format!(
                    "#!/usr/bin/env bash\nexec {0}/venv/bin/python {0}/share/cli \"$@\"\n",
                    root.display()
                ),
            )
            .unwrap();
            fs::write(root.join("bin").join(INFO_TOOL), "#!/bin/sh\n").unwrap();
            Self { root }
        }

        fn prepare(&self) {
            let dir = self.root.join("cache/org/model/rev/audio");
            fs::create_dir_all(&dir).unwrap();
            fs::write(dir.join("fixture.flac"), b"x").unwrap();
        }

        fn inputs(&self) -> Inputs {
            Inputs {
                linux_aarch64: true,
                runtime_override: None,
                python_override: None,
                path_dirs: vec![self.root.join("bin")],
                home: Some(self.root.join("home")),
                cache_root: Some(self.root.join("cache")),
                // A writable device node that always exists on Linux.
                device_node: PathBuf::from("/dev/null"),
            }
        }
    }

    impl Drop for Rig {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.root);
        }
    }

    fn info(text: &'static str) -> impl Fn(&Path) -> Result<String, String> {
        move |_| Ok(text.to_string())
    }

    #[test]
    fn prepared_reference_is_available_and_unprepared_is_not() {
        let rig = Rig::new("prep");
        let ready = probe_with(&rig.inputs(), &info(READY));
        assert_eq!(ready.verdict, Verdict::NotPrepared);
        let runtime = ready.runtime.unwrap();
        assert_eq!(runtime.interpreter, rig.root.join("venv/bin/python"));
        assert_eq!(runtime.cli_script, rig.root.join("share/cli"));

        rig.prepare();
        assert_eq!(
            probe_with(&rig.inputs(), &info(READY)).verdict,
            Verdict::Available
        );
    }

    #[test]
    fn missing_neural_engine_names_itself() {
        let rig = Rig::new("noane");
        rig.prepare();
        let probe = probe_with(&rig.inputs(), &info(NO_ANE));
        assert!(probe.runtime.is_none());
        let Verdict::Unavailable(reason) = probe.verdict else {
            panic!("expected unavailable");
        };
        assert!(reason.contains("Neural Engine"), "{reason}");
    }

    #[test]
    fn device_node_states_have_distinct_reasons() {
        let rig = Rig::new("device");
        let reason = |inputs: &Inputs| match probe_with(inputs, &info(READY)).verdict {
            Verdict::Unavailable(reason) => reason,
            other => format!("{other:?}"),
        };
        let mut inputs = rig.inputs();

        inputs.device_node = rig.root.join("missing");
        let missing = reason(&inputs);

        let locked = rig.root.join("locked");
        fs::write(&locked, b"").unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(&locked, fs::Permissions::from_mode(0o000)).unwrap();
        }
        inputs.device_node = locked;
        let not_writable = reason(&inputs);

        inputs.device_node = PathBuf::from("/dev/null");
        let usable = reason(&inputs);

        assert!(missing.contains("no Neural Engine device"), "{missing}");
        assert!(not_writable.contains("not accessible"), "{not_writable}");
        assert_eq!(usable, "NotPrepared");
    }

    #[test]
    fn runtime_not_found_spawns_nothing() {
        let rig = Rig::new("norun");
        let mut inputs = rig.inputs();
        inputs.path_dirs.clear();
        let spawned = Cell::new(false);
        let probe = probe_with(&inputs, &|_| {
            spawned.set(true);
            Ok(READY.to_string())
        });
        assert!(!spawned.get());
        assert!(
            matches!(probe.verdict, Verdict::Unavailable(reason) if reason.contains("not found"))
        );
    }

    #[test]
    fn override_and_home_locations_are_searched() {
        let rig = Rig::new("where");
        rig.prepare();
        let mut inputs = rig.inputs();
        inputs.path_dirs.clear();
        // The usual per-user location.
        fs::create_dir_all(rig.root.join("home/.local")).unwrap();
        fs::rename(rig.root.join("bin"), rig.root.join("home/.local/bin")).unwrap();
        assert_eq!(
            probe_with(&inputs, &info(READY)).verdict,
            Verdict::Available
        );
        // An explicit override wins over everything else.
        inputs.runtime_override = Some(rig.root.join("nowhere"));
        assert!(matches!(
            probe_with(&inputs, &info(READY)).verdict,
            Verdict::Unavailable(_)
        ));
    }

    #[test]
    fn python_override_replaces_the_launcher_interpreter() {
        let rig = Rig::new("py");
        let mut inputs = rig.inputs();
        inputs.python_override = Some(PathBuf::from("/opt/py/bin/python"));
        let runtime = probe_with(&inputs, &info(READY)).runtime.unwrap();
        assert_eq!(runtime.interpreter, PathBuf::from("/opt/py/bin/python"));
        inputs.python_override = Some(PathBuf::from("python"));
        assert!(probe_with(&inputs, &info(READY)).runtime.is_none());
    }

    #[test]
    fn other_targets_short_circuit_without_touching_the_filesystem() {
        let inputs = Inputs {
            linux_aarch64: false,
            runtime_override: None,
            python_override: None,
            path_dirs: vec![],
            home: None,
            cache_root: None,
            device_node: PathBuf::from("/dev/null"),
        };
        let probe = probe_with(&inputs, &|_| panic!("must not run anything"));
        assert!(matches!(probe.verdict, Verdict::Unavailable(_)));
    }

    #[test]
    fn verdict_is_reused_within_the_ttl_and_recomputed_after() {
        let calls = Cell::new(0);
        let compute = || {
            calls.set(calls.get() + 1);
            Probe::unavailable("x")
        };
        let mut slot = None;
        let start = Instant::now();
        reuse_or_probe(&mut slot, start, compute);
        reuse_or_probe(&mut slot, start + TTL / 2, compute);
        assert_eq!(calls.get(), 1);
        reuse_or_probe(&mut slot, start + TTL + Duration::from_secs(1), compute);
        assert_eq!(calls.get(), 2);
    }

    #[test]
    fn fixtures_name_no_host_or_user() {
        for text in [READY, NO_ANE] {
            assert!(!text.contains("/home/") && !text.contains("/Users/"));
        }
    }
}
