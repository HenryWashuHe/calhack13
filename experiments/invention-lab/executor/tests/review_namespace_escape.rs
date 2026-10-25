//! Review probes for #65's namespace sandbox, run from the host as root.
//! Each case pairs the namespace run with a mode `none` control that shows the
//! probe can see what it is looking for.
//!
//! - A descendant counted only after it has exec'd and ignored SIGTERM/SIGHUP,
//!   so a clean result can't come from the sleeper never having started.
//! - Forty such descendants at once.
//! - A new user and PID namespace requested from inside the root.
//! - A file descriptor the executor's own process holds without close-on-exec:
//!   can the method reach the host filesystem through it?
//!
//! Linux only; namespace cases need root. Local processes only.
#![cfg(target_os = "linux")]
use genesis_core::development_method::{ExecutionOutcome, ResourceGrant};
use genesis_core::development_package::{MethodPackage, MethodSourceFile};
use genesis_lab_exec::run::{self, Config};
use genesis_lab_exec::{example, refs, sandbox::Mode};
use std::os::fd::AsRawFd;
use std::path::PathBuf;
use std::time::{Duration, Instant};

const CART: &str = r#"{"language":1,"payload_on":"deck","bodies":[
 {"shape":"rect","id":"deck","x":-2.0,"y":0.6,"w":3.6,"h":0.3,"material":"wood"},
 {"shape":"circle","id":"w1","x":-3.5,"y":0.45,"r":0.3,"material":"rubber"},
 {"shape":"circle","id":"w2","x":-0.5,"y":0.45,"r":0.3,"material":"rubber"}],
 "joints":[
 {"a":"deck","b":"w1","kind":"hinge","x":-3.5,"y":0.45,"motor_speed":-5.0},
 {"a":"deck","b":"w2","kind":"hinge","x":-0.5,"y":0.45,"motor_speed":-5.0}]}"#;

/// Submits a valid design, then runs `body`. Exit 0 means the probe found
/// nothing; any other exit (including a crash) is a failed run.
fn source(body: &str) -> String {
    format!(
        r#"
import json, os, stat, sys, time
sys.stdout.write(json.dumps({{"op": "submit", "design": json.loads('''{CART}''')}}) + "\n")
sys.stdout.flush()
assert json.loads(sys.stdin.readline())["ok"]
{body}
"#
    )
}

/// Starts `count` detached sleepers. Each one marks itself started only after
/// exec and after ignoring SIGTERM/SIGHUP; the method exits 0 once all have.
fn detached(marker: &str, dir: &str, count: usize) -> String {
    source(&format!(
        r#"
DIR = {dir:?}
os.makedirs(DIR, exist_ok=True)
for i in range({count}):
    if os.fork() == 0:
        os.setsid()
        if os.fork() == 0:
            for fd in (0, 1, 2):
                try: os.close(fd)
                except OSError: pass
            code = ("import signal, sys, time; signal.signal(signal.SIGTERM, signal.SIG_IGN); "
                    "signal.signal(signal.SIGHUP, signal.SIG_IGN); "
                    "open(sys.argv[2], 'w').close(); time.sleep(120)")
            os.execv(sys.executable, [sys.executable, "-c", code, {marker:?}, os.path.join(DIR, str(i))])
        os._exit(0)
    os.wait()
for _ in range(1000):
    if len(os.listdir(DIR)) == {count}:
        sys.exit(0)
    time.sleep(0.01)
sys.exit(3)
"#
    ))
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

fn survivors(marker: &str) -> Vec<u32> {
    let mut found = vec![];
    for entry in std::fs::read_dir("/proc").unwrap().flatten() {
        let Ok(pid) = entry.file_name().to_string_lossy().parse::<u32>() else {
            continue;
        };
        if let Ok(cmd) = std::fs::read(entry.path().join("cmdline")) {
            if cmd.split(|b| *b == 0).any(|arg| arg == marker.as_bytes()) {
                found.push(pid);
            }
        }
    }
    found
}

fn reap(marker: &str) -> Vec<u32> {
    let start = Instant::now();
    let mut left = survivors(marker);
    while left.is_empty() && start.elapsed() < Duration::from_secs(5) {
        std::thread::sleep(Duration::from_millis(20));
        left = survivors(marker);
    }
    for pid in &left {
        // SAFETY: kill(2) takes plain integers.
        unsafe { libc::kill(*pid as i32, libc::SIGKILL) };
    }
    left
}

fn run_source(mode: Mode, tag: &str, src: String) -> ExecutionOutcome {
    let base = if mode == Mode::Namespaces {
        PathBuf::from("/var/tmp")
    } else {
        std::env::temp_dir()
    };
    let dir = base.join(format!("lab-exec-escape-{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let version = run::interpreter_version("python3");
    let package = MethodPackage::new(
        format!("escape-{tag}"),
        "1".into(),
        refs::runtime(&version),
        refs::protocol(),
        "m.py".into(),
        vec![],
        vec![MethodSourceFile {
            path: "m.py".into(),
            content: src,
        }],
    )
    .unwrap();
    let plan = example::acceptance_plan(
        &example::ExampleMethods {
            candidate: package.reference().unwrap(),
            control: refs::content("lab-method/1", "escape-control", "1", b"control"),
        },
        mode,
        &version,
        3,
        ResourceGrant {
            model_calls: 0,
            input_tokens: 0,
            output_tokens: 0,
            wall_time_ms: 20_000,
            simulation_steps: 7200,
        },
    );
    let cfg = Config {
        sandbox: mode,
        interpreter_version: version,
        stage_root: if mode == Mode::Namespaces {
            PathBuf::from("/run/genesis-lab-review").join(tag)
        } else {
            dir.join("stage")
        },
    };
    let bytes = package.artifact_json().unwrap().into_bytes();
    let done = run::execute(&plan, "candidate", &bytes, &dir.join("slot"), &cfg).unwrap();
    let _ = std::fs::remove_dir_all(&dir);
    done.receipt.outcome
}

fn marker(tag: &str) -> String {
    format!("genesis-escape-{tag}-{}", std::process::id())
}

fn sandbox_dir(mode: Mode, tag: &str) -> String {
    if mode == Mode::Namespaces {
        "/tmp/started".into()
    } else {
        std::env::temp_dir()
            .join(format!(
                "lab-exec-escape-flags-{tag}-{}",
                std::process::id()
            ))
            .display()
            .to_string()
    }
}

macro_rules! needs_root {
    () => {
        if euid() != 0 {
            eprintln!("skipped: the namespace sandbox needs root");
            return;
        }
    };
}

#[test]
fn an_execed_signal_ignoring_descendant_is_gone_when_execute_returns() {
    needs_root!();
    let m = marker("one");
    let src = detached(&m, &sandbox_dir(Mode::Namespaces, "one"), 1);
    assert_eq!(
        run_source(Mode::Namespaces, "one", src),
        ExecutionOutcome::OutputGenerated,
        "the sleeper never confirmed it started"
    );
    let left = survivors(&m);
    assert!(left.is_empty(), "descendants still running: {left:?}");
}

#[test]
fn forty_descendants_are_all_gone_when_execute_returns() {
    needs_root!();
    let m = marker("forty");
    let src = detached(&m, &sandbox_dir(Mode::Namespaces, "forty"), 40);
    assert_eq!(
        run_source(Mode::Namespaces, "forty", src),
        ExecutionOutcome::OutputGenerated,
        "not every sleeper confirmed it started"
    );
    let left = survivors(&m);
    assert!(left.is_empty(), "descendants still running: {left:?}");
}

#[test]
fn control_mode_none_leaves_the_confirmed_descendants_running() {
    let m = marker("none");
    let src = detached(&m, &sandbox_dir(Mode::None, "none"), 3);
    assert_eq!(
        run_source(Mode::None, "none", src),
        ExecutionOutcome::OutputGenerated
    );
    let left = reap(&m);
    assert_eq!(left.len(), 3, "control should see all three: {left:?}");
}

const NEW_NAMESPACES: &str = r#"
try:
    os.unshare(os.CLONE_NEWUSER | os.CLONE_NEWPID)
except OSError:
    sys.exit(0)
sys.exit(5)
"#;

#[test]
fn the_method_cannot_create_user_and_pid_namespaces() {
    needs_root!();
    assert_eq!(
        run_source(Mode::Namespaces, "userns", source(NEW_NAMESPACES)),
        ExecutionOutcome::OutputGenerated,
        "unshare(CLONE_NEWUSER | CLONE_NEWPID) succeeded inside the root"
    );
}

#[test]
fn control_root_without_the_sandbox_can_create_them() {
    needs_root!();
    assert_eq!(
        run_source(Mode::None, "userns-none", source(NEW_NAMESPACES)),
        ExecutionOutcome::Failed
    );
}

/// Looks for any inherited descriptor beyond stdio that is a directory, and
/// reads `secret` relative to it after fchdir. Exit 5: host file read.
const INHERITED_DIRECTORY: &str = r#"
for fd in range(3, 1024):
    try:
        st = os.fstat(fd)
    except OSError:
        continue
    if stat.S_ISDIR(st.st_mode):
        try:
            os.fchdir(fd)
            if open("secret").read() == "host-only":
                sys.exit(5)
        except OSError:
            pass
sys.exit(0)
"#;

/// A host directory, readable by `nobody`, held open without close-on-exec in
/// this test process, as a launching shell or supervisor might leave one.
fn leak_host_directory(tag: &str) -> (std::fs::File, PathBuf) {
    use std::os::unix::fs::PermissionsExt;
    let dir =
        PathBuf::from("/var/tmp").join(format!("lab-exec-secret-{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::set_permissions(&dir, std::fs::Permissions::from_mode(0o755)).unwrap();
    std::fs::write(dir.join("secret"), "host-only").unwrap();
    std::fs::set_permissions(dir.join("secret"), std::fs::Permissions::from_mode(0o644)).unwrap();
    let file = std::fs::File::open(&dir).unwrap();
    // SAFETY: fcntl on a descriptor this function owns.
    unsafe { libc::fcntl(file.as_raw_fd(), libc::F_SETFD, 0) };
    (file, dir)
}

#[test]
fn an_inherited_host_directory_is_not_reachable_from_the_root() {
    needs_root!();
    let (file, dir) = leak_host_directory("ns");
    let outcome = run_source(Mode::Namespaces, "fd", source(INHERITED_DIRECTORY));
    drop(file);
    let _ = std::fs::remove_dir_all(&dir);
    assert_eq!(
        outcome,
        ExecutionOutcome::OutputGenerated,
        "the method read a host file through an inherited directory descriptor"
    );
}

#[test]
fn control_without_the_sandbox_the_inherited_directory_is_reachable() {
    let (file, dir) = leak_host_directory("none");
    let outcome = run_source(Mode::None, "fd-none", source(INHERITED_DIRECTORY));
    drop(file);
    let _ = std::fs::remove_dir_all(&dir);
    assert_eq!(outcome, ExecutionOutcome::Failed);
}
