//! Independent review regressions for sealed evidence and interrupted finalization.
//! Pure local sessions; no processes, namespaces, models or provider calls.
use genesis_core::development_experiment::{ExperimentLedger, ExperimentScope};
use genesis_core::development_method::{ExecutionOutcome, ResourceGrant, ResourceUsage};
use genesis_core::development_trial::TrialPlan;
use genesis_lab_exec::run::{recover, Config};
use genesis_lab_exec::session::{finish, RunState, Session};
use genesis_lab_exec::store::Store;
use genesis_lab_exec::{example, refs, sandbox::Mode};
use serde_json::json;

fn plan() -> TrialPlan {
    example::acceptance_plan(
        &example::ExampleMethods {
            candidate: refs::content("lab-method/1", "candidate", "1", b"candidate"),
            control: refs::content("lab-method/1", "control", "1", b"control"),
        },
        Mode::None,
        "Python review fixture",
        1,
        ResourceGrant {
            model_calls: 0,
            input_tokens: 0,
            output_tokens: 0,
            wall_time_ms: 10_000,
            simulation_steps: 7200,
        },
    )
}

fn session<'a>(plan: &'a TrialPlan, name: &str) -> Session<'a> {
    let dir = std::env::temp_dir().join(format!("genesis-review-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let mut s = Session {
        plan,
        slot_id: "candidate",
        store: Store::open(&dir).unwrap(),
        ledger: ExperimentLedger::new(ExperimentScope {
            plan,
            slot_id: "candidate",
        })
        .unwrap(),
        state: RunState {
            started: true,
            method: Some(plan.expected_binding("candidate").unwrap().method),
            ..Default::default()
        },
    };
    // Persist the initial, unsealed state as the real runner does before spawn.
    s.save().unwrap();
    s.state.started = true;
    s
}

#[test]
fn failed_final_ledger_write_leaves_a_recoverable_execution() {
    let p = plan();
    let mut s = session(&p, "final-save");
    // Fail only the final ledger write; transcript, receipt and run state can
    // all be written. This models a storage failure between the durable files.
    let blocker = s.store.path(".ledger.json.tmp");
    std::fs::create_dir(&blocker).unwrap();
    assert!(finish(
        &mut s,
        refs::environment(Mode::None, "Python review fixture"),
        false,
        ExecutionOutcome::Failed,
        ResourceUsage::Measured { value: 1 },
    )
    .is_err());
    std::fs::remove_dir(&blocker).unwrap();
    let dir = s.store.path("");
    let cfg = Config {
        sandbox: Mode::None,
        interpreter_version: "Python review fixture".into(),
        stage_root: dir.join("unused-stage"),
    };
    let recovery = recover(&p, "candidate", &dir, &cfg);
    assert!(
        recovery.is_ok(),
        "a failed finish must be recoverable without re-executing: {:?}",
        recovery.err()
    );
    let recovered = recovery.unwrap();
    let disk_ledger: ExperimentLedger =
        serde_json::from_slice(&s.store.read("ledger.json").unwrap()).unwrap();
    disk_ledger
        .validate_execution(s.scope(), &recovered.receipt)
        .unwrap();
    std::fs::remove_dir_all(dir).unwrap();
}

#[test]
fn sealed_no_output_execution_refuses_a_late_first_submission() {
    let p = plan();
    let mut s = session(&p, "late-submit");
    let done = finish(
        &mut s,
        refs::environment(Mode::None, "Python review fixture"),
        false,
        ExecutionOutcome::Failed,
        ResourceUsage::Measured { value: 1 },
    )
    .unwrap();
    assert!(done.receipt.outputs.is_empty());
    let before = s.store.read("run.json").unwrap();
    let reply = s
        .handle(
            &json!({
                "op": "submit", "design": {
                    "language": 1, "payload_on": "deck",
                    "bodies": [{"shape":"rect", "id":"deck", "x":-2.0, "y":0.6,
                        "w":1.0, "h":0.3, "material":"wood"}], "joints": []
                }
            })
            .to_string(),
        )
        .unwrap();
    assert_eq!(
        reply["ok"], false,
        "a sealed execution cannot accept an output: {reply}"
    );
    assert_eq!(s.store.read("run.json").unwrap(), before);
    std::fs::remove_dir_all(s.store.path("")).unwrap();
}
