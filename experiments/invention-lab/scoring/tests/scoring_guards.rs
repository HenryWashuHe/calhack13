//! Guards a mutation pass found untested: one task family per acceptance plan,
//! and no hidden scoring under a zero wall grant.
use genesis_core::development_experiment::{ExperimentLedger, ExperimentScope};
use genesis_core::development_method::{ExecutionOutcome, ResourceGrant, ResourceUsage};
use genesis_lab_eval::family::Family;
use genesis_lab_exec::session::{finish, RunState, Session};
use genesis_lab_exec::store::Store;
use genesis_lab_exec::{example, refs, sandbox::Mode};
use genesis_lab_score::{score_slot, ScoringError};
use serde_json::json;

const REFERENCE: &str = include_str!("../../evaluator/designs/reference-five-wheel.json");

#[test]
fn an_acceptance_plan_mixing_task_families_is_refused() {
    let methods = example::ExampleMethods {
        candidate: refs::content("lab-method/1", "cand", "1", b"cand"),
        control: refs::content("lab-method/1", "ctrl", "1", b"ctrl"),
    };
    let grant = ResourceGrant {
        model_calls: 0,
        input_tokens: 0,
        output_tokens: 0,
        wall_time_ms: 10_000,
        simulation_steps: 2400,
    };
    let mut p = example::acceptance_plan(&methods, Mode::None, "Python 3", 2, grant);
    // A rough-transport instance among ravine ones, with a context that
    // otherwise matches what the adapter computes for the first family.
    p.development_instances
        .push(refs::instance_ref(Family::RoughTransport, 0));
    let ids: Vec<String> = p
        .development_instances
        .iter()
        .map(|r| r.id.clone())
        .collect();
    p.context = refs::scoring_context(Family::RavineCrossing, &ids);
    p.scoring_grant.simulation_steps = 3 * 2400;
    p.validate().unwrap();

    let dir = std::env::temp_dir().join(format!("lab-score-mixed-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let mut s = Session {
        plan: &p,
        slot_id: "candidate",
        store: Store::open(&dir.join("slot")).unwrap(),
        ledger: ExperimentLedger::new(ExperimentScope {
            plan: &p,
            slot_id: "candidate",
        })
        .unwrap(),
        state: RunState {
            started: true,
            method: Some(p.expected_binding("candidate").unwrap().method),
            ..Default::default()
        },
    };
    let design: serde_json::Value = serde_json::from_str(REFERENCE).unwrap();
    let reply = s
        .handle(&json!({"op": "submit", "design": design}).to_string())
        .unwrap();
    assert_eq!(reply["ok"], true);
    let execution = finish(
        &mut s,
        refs::environment(Mode::None, "Python 3"),
        false,
        ExecutionOutcome::OutputGenerated,
        ResourceUsage::Measured { value: 5 },
    )
    .unwrap()
    .receipt;
    let bytes = std::fs::read(
        dir.join("slot/artifacts")
            .join(&execution.outputs[0].sha256),
    )
    .unwrap();
    let store = Store::open(&dir.join("score")).unwrap();
    assert!(matches!(
        score_slot(&p, "candidate", &execution, &bytes, &store),
        Err(ScoringError::Refused(_))
    ));
    assert!(!store.exists("evaluation.json"), "nothing was scored");
}
