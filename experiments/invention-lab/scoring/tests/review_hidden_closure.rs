//! Independent closure boundary: native local subprocesses in explicit Mode::None.
//! This is not Linux namespace or cloud evidence.
#![cfg(unix)]
use genesis_core::development_experiment::{ExperimentLedger, ExperimentScope};
use genesis_core::development_method::{
    ExecutionOutcome, ExecutionReceipt, ResourceGrant, ResourceUsage,
};
use genesis_core::development_package::{MethodPackage, MethodSourceFile};
use genesis_core::development_trial::TrialPlan;
use genesis_lab_eval::commit::Descriptor;
use genesis_lab_eval::family::Family;
use genesis_lab_exec::run::{execute, interpreter_version, recover, Config};
use genesis_lab_exec::session::{finish, RunState, Session};
use genesis_lab_exec::store::Store;
use genesis_lab_exec::{example, refs, sandbox::Mode};
use genesis_lab_score::hidden::{comparison_plan, score_hidden};
use std::os::unix::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

const SALT: &str = "independent-review-hidden-salt";
const DESIGN: &str = include_str!("../../evaluator/designs/reference-five-wheel.json");

fn config(root: &Path) -> Config {
    Config {
        sandbox: Mode::None,
        interpreter_version: interpreter_version("python3"),
        stage_root: root.join("stage"),
    }
}

#[test]
#[ignore = "Subprocess entry point invoked only by the supervised closure test"]
fn review_executor_child_process() {
    let root = PathBuf::from(std::env::var("GENESIS_HIDDEN_REVIEW_ROOT").unwrap());
    let p: TrialPlan =
        serde_json::from_slice(&std::fs::read(root.join("plan.json")).unwrap()).unwrap();
    let package = std::fs::read(root.join("package.json")).unwrap();
    execute(
        &p,
        "candidate",
        &package,
        &root.join("candidate"),
        &config(&root),
    )
    .unwrap();
}

struct Cleanup {
    runner: Child,
}
impl Drop for Cleanup {
    fn drop(&mut self) {
        // The runner starts its own process group; every Python descendant is
        // owned by this fixture, including orphans after the runner is killed.
        let group = format!("-{}", self.runner.id());
        let _ = Command::new("/bin/kill")
            .args(["-KILL", &group])
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status();
        let _ = self.runner.kill();
        let _ = self.runner.wait();
    }
}

fn known_finished_control(p: &TrialPlan, root: &Path, cfg: &Config) -> ExecutionReceipt {
    let scope = ExperimentScope {
        plan: p,
        slot_id: "control",
    };
    let mut session = Session {
        plan: p,
        slot_id: "control",
        store: Store::open(&root.join("control")).unwrap(),
        ledger: ExperimentLedger::new(scope).unwrap(),
        state: RunState {
            started: true,
            method: Some(p.expected_binding("control").unwrap().method),
            ..Default::default()
        },
    };
    finish(
        &mut session,
        cfg.environment(),
        false,
        ExecutionOutcome::Failed,
        ResourceUsage::Measured { value: 1 },
    )
    .unwrap()
    .receipt
}

#[test]
fn a_recovered_receipt_does_not_prove_the_method_stopped_before_hidden_scoring() {
    let root = std::env::temp_dir().join(format!(
        "genesis-hidden-live-closure-{}",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(&root).unwrap();
    let cfg = config(&root);
    let marker = root.join("method.json");
    // After the acknowledged submission, remain alive for at most 30 seconds,
    // recording a counter without reading stdin again. The test also kills it.
    let source = format!(
        "import json, os, sys, time\nfrom pathlib import Path\ndesign = json.loads({design})\nprint(json.dumps({{'op':'submit','design':design}}), flush=True)\nreply = json.loads(sys.stdin.readline())\nassert reply['ok']\nmarker = Path({marker})\nfor i in range(3000):\n    marker.write_text(json.dumps({{'pid':os.getpid(),'counter':i}}))\n    time.sleep(0.01)\n",
        design = serde_json::to_string(DESIGN).unwrap(),
        marker = serde_json::to_string(marker.to_str().unwrap()).unwrap(),
    );
    let package = MethodPackage::new(
        "review-live-method".into(),
        "1".into(),
        cfg.runtime(),
        refs::protocol(),
        "method.py".into(),
        vec![],
        vec![MethodSourceFile {
            path: "method.py".into(),
            content: source,
        }],
    )
    .unwrap();
    let methods = example::ExampleMethods {
        candidate: package.reference().unwrap(),
        control: package.reference().unwrap(),
    };
    let base = example::acceptance_plan(
        &methods,
        Mode::None,
        &cfg.interpreter_version,
        1,
        ResourceGrant {
            model_calls: 0,
            input_tokens: 0,
            output_tokens: 0,
            wall_time_ms: 30_000,
            simulation_steps: 2400,
        },
    );
    let descriptor = Descriptor::new("review-recovery-closure", SALT, Family::RavineCrossing, 1);
    let p = comparison_plan(&base, "t1", &descriptor);
    std::fs::write(root.join("plan.json"), serde_json::to_vec(&p).unwrap()).unwrap();
    std::fs::write(root.join("package.json"), package.artifact_json().unwrap()).unwrap();
    let child = Command::new(std::env::current_exe().unwrap())
        .args([
            "--ignored",
            "--exact",
            "review_executor_child_process",
            "--nocapture",
        ])
        .process_group(0)
        .env("GENESIS_HIDDEN_REVIEW_ROOT", &root)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .unwrap();
    let mut cleanup = Cleanup { runner: child };
    let start = Instant::now();
    let initial = loop {
        if let Ok(bytes) = std::fs::read(&marker) {
            if let Ok(value) = serde_json::from_slice::<serde_json::Value>(&bytes) {
                break value;
            }
        }
        assert!(
            start.elapsed() < Duration::from_secs(10),
            "method never acknowledged its submission"
        );
        assert!(
            cleanup.runner.try_wait().unwrap().is_none(),
            "runner stopped before submission"
        );
        std::thread::sleep(Duration::from_millis(10));
    };
    assert!(initial["pid"].as_u64().is_some());
    cleanup.runner.kill().unwrap();
    cleanup.runner.wait().unwrap();
    let recovered = recover(&p, "candidate", &root.join("candidate"), &cfg)
        .unwrap()
        .receipt;
    assert_eq!(recovered.outcome, ExecutionOutcome::Unknown);
    assert_eq!(recovered.outputs.len(), 1);
    let control = known_finished_control(&p, &root, &cfg);
    let output = std::fs::read(
        root.join("candidate/artifacts")
            .join(&recovered.outputs[0].sha256),
    )
    .unwrap();
    let store = Store::open(&root.join("score")).unwrap();
    let result = score_hidden(
        &p,
        "candidate",
        &output,
        &[("candidate", &recovered), ("control", &control)],
        &descriptor,
        SALT,
        &store,
    );
    let read_marker = || {
        let started = Instant::now();
        loop {
            if let Ok(bytes) = std::fs::read(&marker) {
                if let Ok(value) = serde_json::from_slice::<serde_json::Value>(&bytes) {
                    break value;
                }
            }
            assert!(
                started.elapsed() < Duration::from_secs(1),
                "marker unreadable"
            );
            std::thread::sleep(Duration::from_millis(1));
        }
    };
    let before = read_marker();
    std::thread::sleep(Duration::from_millis(30));
    let after = read_marker();
    assert!(
        after["counter"].as_u64().unwrap() > before["counter"].as_u64().unwrap(),
        "fixture must demonstrate the method remained alive after recovery/scoring"
    );
    assert!(
        result.is_err(),
        "a recovery receipt admitted hidden scoring while its method was still running: {:?}",
        result.as_ref().map(|r| &r.outcome)
    );
}
