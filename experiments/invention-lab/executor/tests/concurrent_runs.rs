//! Two runs of the same slot ID in one process must not share a staging
//! directory. The stage was named from the process ID and the slot ID only, so
//! concurrent runs of `candidate` (any two trials of one runner, or parallel
//! tests) staged into the same path, and one run's teardown removed the files
//! another run was still being started from; that run failed with no
//! submission. Found composing #57's parallel hidden tests with #65; it
//! reproduces on main. Linux, root; local processes only.
#![cfg(target_os = "linux")]
use genesis_core::development_method::{ExecutionOutcome, ResourceGrant};
use genesis_core::development_package::{MethodPackage, MethodSourceFile};
use genesis_lab_exec::run::{self, Config};
use genesis_lab_exec::{example, refs, sandbox::Mode};
use std::path::PathBuf;

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

#[test]
fn concurrent_runs_of_one_slot_id_each_get_their_own_stage() {
    if !root() {
        eprintln!("skipped: the namespace sandbox needs root");
        return;
    }
    let tag = format!("concurrent-{}", std::process::id());
    let dir = PathBuf::from("/var/tmp").join(format!("lab-exec-{tag}"));
    let _ = std::fs::remove_dir_all(&dir);
    let version = run::interpreter_version("python3");
    // Submit, wait for the reply, then exit shortly after.
    let code = format!(
        "import json, sys, time\nsys.stdout.write(json.dumps({{'op': 'submit', 'design': json.loads('''{CART}''')}}) + '\\n')\nsys.stdout.flush()\nsys.stdin.readline()\ntime.sleep(0.2)\n"
    );
    let package = MethodPackage::new(
        "concurrent".into(),
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
            control: refs::content("lab-method/1", "concurrent-control", "1", b"control"),
        },
        Mode::Namespaces,
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
        sandbox: Mode::Namespaces,
        interpreter_version: version,
        stage_root: PathBuf::from("/run/genesis-lab-test").join(&tag),
    };
    let bytes = package.artifact_json().unwrap().into_bytes();
    let outcomes: Vec<_> = std::thread::scope(|scope| {
        let handles: Vec<_> = (0..8u64)
            .map(|n| {
                let (plan, cfg, bytes) = (&plan, &cfg, &bytes);
                let slot = dir.join(format!("slot-{n}"));
                scope.spawn(move || {
                    // Staggered starts, so some runs set up while others tear down.
                    std::thread::sleep(std::time::Duration::from_millis(60 * n));
                    run::execute(plan, "candidate", bytes, &slot, cfg).map(|d| d.receipt.outcome)
                })
            })
            .collect();
        handles.into_iter().map(|h| h.join().unwrap()).collect()
    });
    for outcome in outcomes {
        assert_eq!(outcome.unwrap(), ExecutionOutcome::OutputGenerated);
    }
    let _ = std::fs::remove_dir_all(&dir);
}
