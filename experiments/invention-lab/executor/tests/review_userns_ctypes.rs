//! The user-namespace probe from `review_namespace_escape.rs`, made independent
//! of the interpreter version. That file calls `os.unshare`, which exists only
//! from Python 3.12; on 3.11 it raises `AttributeError`, so the namespace case
//! fails and its `none` control passes for the wrong reason (both are just a
//! crash). This probe calls `unshare(2)` through ctypes instead, and the control
//! has to leave a marker, so a crash can't pass it.
//!
//! What it shows: the method, chrooted as `nobody`, can't create a user (or
//! PID) namespace even on a host that lets unprivileged users create user
//! namespaces. The kernel refuses `CLONE_NEWUSER` to a chrooted caller. The test
//! also records whether the host itself permits it outside a chroot, so a pass
//! on a permissive host is visibly not a host-policy pass.
//!
//! Linux only; needs root. Local processes only. Leaves the original file as is.
#![cfg(target_os = "linux")]
use genesis_core::development_method::{ExecutionOutcome, ResourceGrant};
use genesis_core::development_package::{MethodPackage, MethodSourceFile};
use genesis_lab_exec::run::{self, Config};
use genesis_lab_exec::{example, refs, sandbox::Mode};
use std::path::{Path, PathBuf};
use std::process::Command;

const CART: &str = r#"{"language":1,"payload_on":"deck","bodies":[
 {"shape":"rect","id":"deck","x":-2.0,"y":0.6,"w":3.6,"h":0.3,"material":"wood"},
 {"shape":"circle","id":"w1","x":-3.5,"y":0.45,"r":0.3,"material":"rubber"},
 {"shape":"circle","id":"w2","x":-0.5,"y":0.45,"r":0.3,"material":"rubber"}],
 "joints":[
 {"a":"deck","b":"w1","kind":"hinge","x":-3.5,"y":0.45,"motor_speed":-5.0},
 {"a":"deck","b":"w2","kind":"hinge","x":-0.5,"y":0.45,"motor_speed":-5.0}]}"#;

/// Exit 0: refused (EPERM/EINVAL). Exit 5: a new user and PID namespace was
/// created; `marker`, when given, is written first. Anything else crashes.
fn probe(marker: Option<&Path>) -> String {
    let mark = match marker {
        Some(p) => format!(
            "    open({:?}, 'w').write('created')\n",
            p.display().to_string()
        ),
        None => String::new(),
    };
    format!(
        r#"
import ctypes, json, sys
sys.stdout.write(json.dumps({{"op": "submit", "design": json.loads('''{CART}''')}}) + "\n")
sys.stdout.flush()
assert json.loads(sys.stdin.readline())["ok"]
libc = ctypes.CDLL(None, use_errno=True)
CLONE_NEWUSER, CLONE_NEWPID = 0x10000000, 0x20000000
if libc.unshare(CLONE_NEWUSER | CLONE_NEWPID) != 0:
    sys.exit(0)
if True:
{mark}    sys.exit(5)
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

fn run_source(mode: Mode, tag: &str, src: String) -> ExecutionOutcome {
    let base = if mode == Mode::Namespaces {
        PathBuf::from("/var/tmp")
    } else {
        std::env::temp_dir()
    };
    let dir = base.join(format!("lab-exec-userns-{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let version = run::interpreter_version("python3");
    let package = MethodPackage::new(
        format!("userns-{tag}"),
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
            control: refs::content("lab-method/1", "userns-control", "1", b"control"),
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

/// Whether `nobody`, unchrooted, may create a user namespace on this host.
fn host_permits_unprivileged_userns() -> Option<bool> {
    let out = Command::new("setpriv")
        .args([
            "--reuid",
            "65534",
            "--regid",
            "65534",
            "--clear-groups",
            "--no-new-privs",
        ])
        .args(["unshare", "-U", "true"])
        .output()
        .ok()?;
    Some(out.status.success())
}

#[test]
fn the_chrooted_method_cannot_create_a_user_namespace_whatever_the_host_allows() {
    if euid() != 0 {
        eprintln!("skipped: the namespace sandbox needs root");
        return;
    }
    eprintln!(
        "host lets an unchrooted nobody create a user namespace: {:?}",
        host_permits_unprivileged_userns()
    );
    assert_eq!(
        run_source(Mode::Namespaces, "userns-ctypes", probe(None)),
        ExecutionOutcome::OutputGenerated,
        "unshare(CLONE_NEWUSER | CLONE_NEWPID) succeeded inside the method root"
    );
}

#[test]
fn control_without_the_sandbox_creates_them_and_leaves_its_marker() {
    if euid() != 0 {
        eprintln!("skipped: the control runs as root to create namespaces");
        return;
    }
    let marker =
        std::env::temp_dir().join(format!("lab-exec-userns-marker-{}", std::process::id()));
    let _ = std::fs::remove_file(&marker);
    assert_eq!(
        run_source(Mode::None, "userns-ctypes-none", probe(Some(&marker))),
        ExecutionOutcome::Failed
    );
    assert_eq!(
        std::fs::read_to_string(&marker).ok().as_deref(),
        Some("created"),
        "the control failed without creating the namespaces, so it shows nothing"
    );
    let _ = std::fs::remove_file(&marker);
}
