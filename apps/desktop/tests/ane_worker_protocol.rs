//! The Neural Engine worker shim against the stub CLI: the JSON-lines
//! protocol, reply-channel isolation, failure handling, and scratch cleanup.
//! No runtime, model, driver, or network is touched.

use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, ChildStdin, Command, Stdio};
use std::sync::mpsc::{Receiver, channel};
use std::time::{Duration, Instant};

use serde_json::{Value, json};

const SHIM: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/resources/ane/ane_worker.py");
const STUB: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/tests/fixtures/ane/stub_cli.py"
);
const WAIT: Duration = Duration::from_secs(30);

fn python() -> Option<&'static str> {
    let found = Command::new("python3")
        .arg("--version")
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .is_ok_and(|status| status.success());
    if !found {
        eprintln!("SKIP: no python3 on this machine; the worker shim tests did not run");
    }
    found.then_some("python3")
}

struct Shim {
    child: Child,
    stdin: Option<ChildStdin>,
    lines: Receiver<String>,
    work: PathBuf,
}

impl Shim {
    fn start(name: &str, env: &[(&str, &str)]) -> Option<Self> {
        let python = python()?;
        let work =
            std::env::temp_dir().join(format!("wordink-ane-shim-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&work);
        let mut command = Command::new(python);
        command
            .args([
                SHIM,
                "--cli-script",
                STUB,
                "--deadline-ms",
                "1000",
                "--work-dir",
            ])
            .arg(&work)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null());
        for (key, value) in env {
            command.env(key, value);
        }
        let mut child = command.spawn().expect("spawn shim");
        let stdout = child.stdout.take().unwrap();
        let (sender, lines) = channel();
        std::thread::spawn(move || {
            for line in BufReader::new(stdout).lines().map_while(Result::ok) {
                if sender.send(line).is_err() {
                    break;
                }
            }
        });
        let stdin = child.stdin.take();
        Some(Self {
            child,
            stdin,
            lines,
            work,
        })
    }

    fn send_raw(&mut self, bytes: &[u8]) {
        let stdin = self.stdin.as_mut().unwrap();
        stdin.write_all(bytes).unwrap();
        stdin.flush().unwrap();
    }

    fn send(&mut self, request: Value) {
        self.send_raw(format!("{request}\n").as_bytes());
    }

    fn recv(&self) -> Value {
        let line = self.lines.recv_timeout(WAIT).expect("a reply line");
        serde_json::from_str(&line).unwrap_or_else(|_| panic!("reply was not JSON: {line:?}"))
    }

    fn call(&mut self, request: Value) -> Value {
        self.send(request);
        self.recv()
    }

    fn hello(&mut self) -> Value {
        self.call(json!({"id": 1, "op": "hello"}))
    }

    fn transcribe(&mut self, id: u64, wav: &str) -> Value {
        self.call(json!({"id": id, "op": "transcribe", "wav": wav, "deadline_ms": 8000}))
    }

    /// Close stdin (the lease) and wait for the exit.
    fn finish(&mut self) -> Option<i32> {
        self.stdin = None;
        let started = Instant::now();
        while started.elapsed() < WAIT {
            if let Some(status) = self.child.try_wait().unwrap() {
                return status.code();
            }
            std::thread::sleep(Duration::from_millis(20));
        }
        let _ = self.child.kill();
        panic!("the shim did not exit");
    }

    fn scratch_entries(&self) -> Vec<String> {
        entries(&self.work)
    }
}

impl Drop for Shim {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
        let _ = std::fs::remove_dir_all(&self.work);
    }
}

fn entries(dir: &Path) -> Vec<String> {
    std::fs::read_dir(dir)
        .map(|read| {
            read.flatten()
                .map(|e| e.file_name().to_string_lossy().into_owned())
                .collect()
        })
        .unwrap_or_default()
}

fn recorded_transcript() -> (String, f64) {
    let text = include_str!("fixtures/ane/cli-match.json");
    let report: Value = serde_json::from_str(text).unwrap();
    (
        report["transcript"].as_str().unwrap().to_string(),
        report["total_pipeline_ms"].as_f64().unwrap(),
    )
}

#[test]
fn transcribe_round_trip_returns_the_recorded_transcript() {
    let Some(mut shim) = Shim::start("roundtrip", &[]) else {
        return;
    };
    let hello = shim.hello();
    assert_eq!(hello["ok"], true);
    assert_eq!(hello["protocol"], 1);
    let (transcript, pipeline_ms) = recorded_transcript();
    let reply = shim.transcribe(7, "/tmp/clip.wav");
    assert_eq!(reply["id"], 7);
    assert_eq!(reply["ok"], true);
    assert_eq!(reply["transcript"], transcript);
    assert_eq!(reply["pipeline_ms"].as_f64(), Some(pipeline_ms));
    // Two in a row are served in order with their own ids.
    assert_eq!(shim.transcribe(8, "/tmp/clip.wav")["id"], 8);
    assert_eq!(shim.finish(), Some(0));
}

#[test]
fn runtime_output_cannot_corrupt_the_reply_channel() {
    let Some(mut shim) = Shim::start("banner", &[]) else {
        return;
    };
    shim.hello();
    // The stub prints a banner before the report, and writes to fd 1 and stderr.
    for name in ["/tmp/banner.wav", "/tmp/stderr.wav"] {
        let reply = shim.transcribe(2, name);
        assert_eq!(reply["ok"], true, "{name}: {reply}");
        assert_eq!(reply["transcript"], recorded_transcript().0);
    }
}

#[test]
fn a_refusal_replies_then_the_shim_exits() {
    for name in ["/tmp/refuse.wav", "/tmp/exit.wav"] {
        let Some(mut shim) = Shim::start("refuse", &[]) else {
            return;
        };
        shim.hello();
        let reply = shim.transcribe(3, name);
        assert_eq!(reply["ok"], false, "{reply}");
        assert_eq!(reply["code"], "refused");
        if name.contains("refuse") {
            assert!(
                reply["message"]
                    .as_str()
                    .unwrap()
                    .contains("reference cache")
            );
        }
        assert_eq!(shim.finish(), Some(3));
    }
}

#[test]
fn a_bad_status_fails_the_request_and_keeps_serving() {
    let Some(mut shim) = Shim::start("badstatus", &[]) else {
        return;
    };
    shim.hello();
    for (wav, code) in [
        ("/tmp/fail.wav", "bad_status"),
        ("/tmp/garbage.wav", "bad_output"),
    ] {
        let reply = shim.transcribe(4, wav);
        assert_eq!(reply["ok"], false);
        assert_eq!(reply["code"], code);
    }
    assert_eq!(shim.transcribe(5, "/tmp/clip.wav")["ok"], true);
}

#[test]
fn a_self_test_mismatch_fails_hello() {
    let Some(mut shim) = Shim::start("selftest", &[("STUB_SELFTEST", "mismatch")]) else {
        return;
    };
    let hello = shim.hello();
    assert_eq!(hello["ok"], false);
    assert_eq!(hello["code"], "selftest");
    assert_eq!(shim.finish(), Some(3));
}

#[test]
fn eof_exits_promptly_and_scratch_is_removed_on_every_path() {
    let Some(mut shim) = Shim::start("scratch", &[]) else {
        return;
    };
    shim.hello();
    assert!(
        shim.scratch_entries().is_empty(),
        "self-test scratch left behind"
    );
    shim.transcribe(2, "/tmp/clip.wav");
    assert!(
        shim.scratch_entries().is_empty(),
        "scratch left after success"
    );
    shim.transcribe(3, "/tmp/fail.wav");
    assert!(
        shim.scratch_entries().is_empty(),
        "scratch left after failure"
    );
    let started = Instant::now();
    assert_eq!(shim.finish(), Some(0));
    assert!(started.elapsed() < Duration::from_secs(5));
    assert!(shim.scratch_entries().is_empty());
}

#[test]
fn a_refusal_leaves_no_scratch_either() {
    let Some(mut shim) = Shim::start("refusescratch", &[]) else {
        return;
    };
    shim.hello();
    shim.transcribe(2, "/tmp/refuse.wav");
    shim.finish();
    assert!(shim.scratch_entries().is_empty());
}

#[test]
fn malformed_and_oversized_lines_are_rejected_without_crashing() {
    let Some(mut shim) = Shim::start("malformed", &[]) else {
        return;
    };
    shim.hello();
    shim.send_raw(b"this is not json\n");
    assert_eq!(shim.recv()["code"], "bad_request");
    shim.send_raw(b"[1, 2]\n");
    assert_eq!(shim.recv()["code"], "bad_request");
    let mut huge = vec![b'x'; 200 * 1024];
    huge.push(b'\n');
    shim.send_raw(&huge);
    assert_eq!(shim.recv()["code"], "too_large");
    assert_eq!(
        shim.call(json!({"id": 9, "op": "nope"}))["code"],
        "bad_request"
    );
    let relative = shim.call(json!({"id": 10, "op": "transcribe", "wav": "clip.wav"}));
    assert_eq!(relative["code"], "bad_request");
    assert_eq!(shim.transcribe(11, "/tmp/clip.wav")["ok"], true);
}
