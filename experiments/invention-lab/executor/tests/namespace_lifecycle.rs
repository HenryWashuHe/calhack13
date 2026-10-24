//! Whole-tree stopping, observed from the host. A method submits, then starts
//! a detached descendant (double fork, new session, stdio closed, SIGTERM and
//! SIGHUP ignored) and exits. Under the namespace sandbox nothing of the method
//! may still run once `execute` returns; under mode `none` the same descendant
//! survives, which shows the probe can see a survivor.
//!
//! Linux only; the namespace case needs root. Local processes, not a claim of
//! safe general untrusted execution.
#![cfg(target_os = "linux")]
use genesis_core::development_method::{ExecutionOutcome, ResourceGrant};
use genesis_core::development_package::{MethodPackage, MethodSourceFile};
use genesis_lab_exec::run::{self, Config};
use genesis_lab_exec::{example, refs, sandbox::Mode};
use std::path::PathBuf;
use std::time::{Duration, Instant};

const CART: &str = r#"{"language":1,"payload_on":"deck","bodies":[
 {"shape":"rect","id":"deck","x":-2.0,"y":0.6,"w":3.6,"h":0.3,"material":"wood"},
 {"shape":"circle","id":"w1","x":-3.5,"y":0.45,"r":0.3,"material":"rubber"},
 {"shape":"circle","id":"w2","x":-0.5,"y":0.45,"r":0.3,"material":"rubber"}],
 "joints":[
 {"a":"deck","b":"w1","kind":"hinge","x":-3.5,"y":0.45,"motor_speed":-5.0},
 {"a":"deck","b":"w2","kind":"hinge","x":-0.5,"y":0.45,"motor_speed":-5.0}]}"#;

/// Submits, then leaves a detached sleeper whose argv carries `marker`, and
/// exits 0 only after the sleeper has started (else exit 3, a failed run).
fn method(marker: &str, flag: &str, linger: bool) -> String {
    let after = if linger {
        "time.sleep(60)"
    } else {
        "sys.exit(0)"
    };
    format!(
        r#"
import json, os, sys, time
sys.stdout.write(json.dumps({{"op": "submit", "design": json.loads('''{CART}''')}}) + "\n")
sys.stdout.flush()
assert json.loads(sys.stdin.readline())["ok"]
FLAG = {flag:?}
if os.fork() == 0:
    os.setsid()
    if os.fork() == 0:
        for fd in (0, 1, 2):
            try: os.close(fd)
            except OSError: pass
        open(FLAG, "w").close()
        code = "import signal, time; signal.signal(signal.SIGTERM, signal.SIG_IGN); signal.signal(signal.SIGHUP, signal.SIG_IGN); time.sleep(120)"
        os.execv(sys.executable, [sys.executable, "-c", code, {marker:?}])
    os._exit(0)
for _ in range(500):
    if os.path.exists(FLAG):
        {after}
    time.sleep(0.01)
sys.exit(3)
"#
    )
}

fn euid() -> u32 {
    std::fs::read_to_string("/proc/self/status")
        .unwrap_or_default()
        .lines()
        .find(|l| l.starts_with("Uid:"))
        .and_then(|l| l.split_whitespace().nth(2))
        .and_then(|v| v.parse().ok())
        .unwrap_or(u32::MAX)
}

/// Host pids whose argv contains `marker`.
fn survivors(marker: &str) -> Vec<u32> {
    let mut found = vec![];
    for entry in std::fs::read_dir("/proc").unwrap().flatten() {
        let Ok(pid) = entry.file_name().to_string_lossy().parse::<u32>() else {
            continue;
        };
        if pid == std::process::id() {
            continue;
        }
        if let Ok(cmd) = std::fs::read(entry.path().join("cmdline")) {
            if cmd.split(|b| *b == 0).any(|arg| arg == marker.as_bytes()) {
                found.push(pid);
            }
        }
    }
    found
}

fn run_once(mode: Mode, tag: &str, linger: bool) -> (ExecutionOutcome, String) {
    let marker = format!("genesis-lifecycle-{tag}-{}", std::process::id());
    let base = if mode == Mode::Namespaces {
        PathBuf::from("/var/tmp")
    } else {
        std::env::temp_dir()
    };
    let dir = base.join(format!("lab-exec-lifecycle-{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    // Inside the namespace root /tmp is the sandbox's own tmpfs.
    let flag = if mode == Mode::Namespaces {
        "/tmp/started".to_owned()
    } else {
        dir.join("started").display().to_string()
    };
    let version = run::interpreter_version("python3");
    let package = MethodPackage::new(
        format!("lifecycle-{tag}"),
        "1".into(),
        refs::runtime(&version),
        refs::protocol(),
        "m.py".into(),
        vec![],
        vec![MethodSourceFile {
            path: "m.py".into(),
            content: method(&marker, &flag, linger),
        }],
    )
    .unwrap();
    let plan = example::acceptance_plan(
        &example::ExampleMethods {
            candidate: package.reference().unwrap(),
            control: refs::content("lab-method/1", "lifecycle-control", "1", b"control"),
        },
        mode,
        &version,
        3,
        ResourceGrant {
            model_calls: 0,
            input_tokens: 0,
            output_tokens: 0,
            wall_time_ms: if linger { 3_000 } else { 20_000 },
            simulation_steps: 7200,
        },
    );
    let cfg = Config {
        sandbox: mode,
        interpreter_version: version,
        stage_root: if mode == Mode::Namespaces {
            // Per test: concurrent runs in one process share pid and slot id.
            PathBuf::from("/run/genesis-lab-test").join(tag)
        } else {
            dir.join("stage")
        },
    };
    let bytes = package.artifact_json().unwrap().into_bytes();
    let done = run::execute(&plan, "candidate", &bytes, &dir.join("slot"), &cfg).unwrap();
    (done.receipt.outcome, marker)
}

#[test]
fn a_detached_descendant_does_not_outlive_a_namespace_run() {
    if euid() != 0 {
        eprintln!("skipped: the namespace sandbox needs root");
        return;
    }
    let (outcome, marker) = run_once(Mode::Namespaces, "ns", false);
    // OutputGenerated means the method saw its descendant start, then exited 0.
    assert_eq!(outcome, ExecutionOutcome::OutputGenerated);
    let left = survivors(&marker);
    assert!(left.is_empty(), "descendants still running: {left:?}");
}

#[test]
fn a_detached_descendant_does_not_outlive_a_namespace_run_killed_at_its_deadline() {
    if euid() != 0 {
        eprintln!("skipped: the namespace sandbox needs root");
        return;
    }
    // The method itself keeps running, so the executor kills it at the deadline.
    let (outcome, marker) = run_once(Mode::Namespaces, "ns-deadline", true);
    assert_eq!(outcome, ExecutionOutcome::Interrupted);
    let left = survivors(&marker);
    assert!(left.is_empty(), "descendants still running: {left:?}");
}

#[test]
fn the_probe_sees_the_same_descendant_survive_without_the_sandbox() {
    let (outcome, marker) = run_once(Mode::None, "none", false);
    assert_eq!(outcome, ExecutionOutcome::OutputGenerated);
    // The sleeper execs right after the flag is written; allow it a moment.
    let start = Instant::now();
    let mut left = survivors(&marker);
    while left.is_empty() && start.elapsed() < Duration::from_secs(5) {
        std::thread::sleep(Duration::from_millis(20));
        left = survivors(&marker);
    }
    for pid in &left {
        let _ = std::process::Command::new("kill")
            .args(["-KILL", &pid.to_string()])
            .status();
    }
    assert!(
        !left.is_empty(),
        "mode none should leave the descendant running (control)"
    );
}
