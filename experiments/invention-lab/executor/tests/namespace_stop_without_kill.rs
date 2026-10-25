//! Stopping must not depend on a `kill` program. With no `kill` on PATH (a
//! stripped PATH, or the spawn failing for lack of processes or descriptors)
//! a namespace run past its deadline previously never stopped: the stop path
//! believed it had signalled the namespace and then waited forever. Found by
//! an independent reviewer of #65. Linux, root; local processes only.
#![cfg(target_os = "linux")]
use genesis_core::development_method::{ExecutionOutcome, ResourceGrant};
use genesis_core::development_package::{MethodPackage, MethodSourceFile};
use genesis_lab_exec::run::{self, Config};
use genesis_lab_exec::{example, refs, sandbox::Mode};
use std::path::PathBuf;
use std::time::Duration;

const CART: &str = r#"{"language":1,"payload_on":"deck","bodies":[
 {"shape":"rect","id":"deck","x":-2.0,"y":0.6,"w":3.6,"h":0.3,"material":"wood"},
 {"shape":"circle","id":"w1","x":-3.5,"y":0.45,"r":0.3,"material":"rubber"},
 {"shape":"circle","id":"w2","x":-0.5,"y":0.45,"r":0.3,"material":"rubber"}],
 "joints":[
 {"a":"deck","b":"w1","kind":"hinge","x":-3.5,"y":0.45,"motor_speed":-5.0},
 {"a":"deck","b":"w2","kind":"hinge","x":-0.5,"y":0.45,"motor_speed":-5.0}]}"#;

fn root() -> bool {
    std::fs::read_to_string("/proc/self/status")
        .unwrap_or_default()
        .lines()
        .find(|l| l.starts_with("Uid:"))
        .and_then(|l| l.split_whitespace().nth(2))
        == Some("0")
}

/// A PATH holding every program of the usual system directories except `kill`.
fn path_without_kill(dir: &std::path::Path) -> String {
    std::fs::create_dir_all(dir).unwrap();
    for src in ["/usr/local/bin", "/usr/bin", "/bin", "/usr/sbin", "/sbin"] {
        for entry in std::fs::read_dir(src).into_iter().flatten().flatten() {
            let name = entry.file_name();
            let link = dir.join(&name);
            if name != "kill" && !link.exists() {
                let _ = std::os::unix::fs::symlink(entry.path(), link);
            }
        }
    }
    dir.display().to_string()
}

#[test]
fn a_run_past_its_deadline_stops_without_a_kill_program() {
    if !root() {
        eprintln!("skipped: the namespace sandbox needs root");
        return;
    }
    let tag = format!("nokill-{}", std::process::id());
    let dir = PathBuf::from("/var/tmp").join(format!("lab-exec-{tag}"));
    let _ = std::fs::remove_dir_all(&dir);
    let version = run::interpreter_version("python3");
    // This test binary holds only this test, so changing PATH affects nothing else.
    std::env::set_var("PATH", path_without_kill(&dir.join("bin")));
    let code = format!(
        "import json, sys, time\nsys.stdout.write(json.dumps({{'op': 'submit', 'design': json.loads('''{CART}''')}}) + '\\n')\nsys.stdout.flush()\nsys.stdin.readline()\ntime.sleep(120)\n"
    );
    let package = MethodPackage::new(
        "nokill".into(),
        "1".into(),
        refs::runtime(&version),
        refs::protocol(),
        "m.py".into(),
        vec![],
        vec![MethodSourceFile {
            path: "m.py".into(),
            content: code,
        }],
    )
    .unwrap();
    let plan = example::acceptance_plan(
        &example::ExampleMethods {
            candidate: package.reference().unwrap(),
            control: refs::content("lab-method/1", "nokill-control", "1", b"control"),
        },
        Mode::Namespaces,
        &version,
        3,
        ResourceGrant {
            model_calls: 0,
            input_tokens: 0,
            output_tokens: 0,
            wall_time_ms: 2_000,
            simulation_steps: 7200,
        },
    );
    let cfg = Config {
        sandbox: Mode::Namespaces,
        interpreter_version: version,
        stage_root: PathBuf::from("/run/genesis-lab-test").join(&tag),
    };
    let bytes = package.artifact_json().unwrap().into_bytes();
    let slot = dir.join("slot");
    let (tx, rx) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        let _ = tx
            .send(run::execute(&plan, "candidate", &bytes, &slot, &cfg).map(|d| d.receipt.outcome));
    });
    let outcome = rx
        .recv_timeout(Duration::from_secs(60))
        .expect("a 2 s run was still not stopped after 60 s")
        .unwrap();
    assert_eq!(outcome, ExecutionOutcome::Interrupted);
}
