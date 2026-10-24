//! The experiment session in-process: every request through the #32 ledger,
//! with the durable state checked after each call. No processes.
use genesis_core::development_experiment::{ExperimentLedger, ExperimentScope};
use genesis_core::development_method::{ExecutionOutcome, ResourceGrant, ResourceUsage};
use genesis_core::development_trial::{assess_trials, TrialEvidence, TrialPlan};
use genesis_lab_eval::design::Design;
use genesis_lab_exec::session::{finish, RunState, Session};
use genesis_lab_exec::store::Store;
use genesis_lab_exec::{example, refs, sandbox::Mode};
use serde_json::{json, Value};

const CART: &str = r#"{"language":1,"payload_on":"deck","bodies":[
 {"shape":"rect","id":"deck","x":-2.0,"y":0.6,"w":3.6,"h":0.3,"material":"wood"},
 {"shape":"circle","id":"w1","x":-3.5,"y":0.45,"r":0.3,"material":"rubber"},
 {"shape":"circle","id":"w2","x":-0.5,"y":0.45,"r":0.3,"material":"rubber"}],
 "joints":[
 {"a":"deck","b":"w1","kind":"hinge","x":-3.5,"y":0.45,"motor_speed":-5.0},
 {"a":"deck","b":"w2","kind":"hinge","x":-0.5,"y":0.45,"motor_speed":-5.0}]}"#;

fn plan(steps: u64) -> TrialPlan {
    let methods = example::ExampleMethods {
        candidate: refs::content("lab-method/1", "cand", "1", b"cand"),
        control: refs::content("lab-method/1", "ctrl", "1", b"ctrl"),
    };
    let grant = ResourceGrant {
        model_calls: 0,
        input_tokens: 0,
        output_tokens: 0,
        wall_time_ms: 10_000,
        simulation_steps: steps,
    };
    example::acceptance_plan(&methods, Mode::None, "Python 3", 2, grant)
}

fn session<'a>(plan: &'a TrialPlan, name: &str) -> Session<'a> {
    let dir = std::env::temp_dir().join(format!("lab-session-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let scope = ExperimentScope {
        plan,
        slot_id: "candidate",
    };
    Session {
        plan,
        slot_id: "candidate",
        store: Store::open(&dir).unwrap(),
        ledger: ExperimentLedger::new(scope).unwrap(),
        state: RunState {
            started: true,
            method: Some(plan.expected_binding("candidate").unwrap().method),
            ..Default::default()
        },
    }
}

fn ask(s: &mut Session<'_>, req: Value) -> Value {
    s.handle(&req.to_string()).unwrap()
}

fn on_disk(s: &Session<'_>) -> ExperimentLedger {
    serde_json::from_slice(&s.store.read("ledger.json").unwrap()).unwrap()
}

fn cart() -> Value {
    serde_json::from_str(CART).unwrap()
}

#[test]
fn experiments_are_charged_cumulatively_and_each_call_is_durable() {
    let p = plan(3 * 2400);
    let mut s = session(&p, "budget");
    let mut used = 0;
    loop {
        let r = ask(
            &mut s,
            json!({"op":"experiment","instance":"ravine-crossing/1/0","design":cart()}),
        );
        if !r["ok"].as_bool().unwrap() {
            assert!(
                r["refused"].as_str().unwrap().contains("BudgetExceeded"),
                "{r}"
            );
            break;
        }
        used += r["steps"].as_u64().unwrap();
        assert_eq!(r["remaining_steps"].as_u64().unwrap(), 3 * 2400 - used);
        // What the method was told is exactly what is on disk.
        assert_eq!(on_disk(&s), s.ledger);
    }
    assert!(
        3 * 2400 - used < 2400,
        "refused only when a full episode no longer fits"
    );
    let records = s.ledger.records().len();
    assert!(records >= 3);
    assert_eq!(
        on_disk(&s).records().len(),
        records,
        "the refusal left no record"
    );
}

#[test]
fn hidden_unlisted_and_malformed_requests_are_refused_without_a_record() {
    let p = plan(7200);
    let mut s = session(&p, "refusals");
    for instance in [
        format!("ravine-crossing/1/{}", 1u64 << 63),
        "ravine-crossing/1/99".into(),
        "return-mix/1/0".into(),
    ] {
        let r = ask(
            &mut s,
            json!({"op":"experiment","instance":instance,"design":cart()}),
        );
        assert!(
            r["refused"]
                .as_str()
                .unwrap()
                .contains("not a public instance"),
            "{r}"
        );
    }
    assert!(!s.handle("not json").unwrap()["ok"].as_bool().unwrap());
    assert!(!ask(&mut s, json!({"op":"score"}))["ok"].as_bool().unwrap());
    assert!(s.ledger.records().is_empty());
}

#[test]
fn invalid_designs_are_recorded_at_zero_steps() {
    let p = plan(7200);
    let mut s = session(&p, "invalid");
    let r = ask(
        &mut s,
        json!({"op":"experiment","instance":"ravine-crossing/1/1","design":{"not":"a design"}}),
    );
    assert_eq!(
        (r["outcome"].as_str(), r["ok"].as_bool()),
        (Some("invalid"), Some(true)),
        "{r}"
    );
    let mut out_of_zone = cart();
    out_of_zone["bodies"][0]["x"] = 5.0.into();
    let r = ask(
        &mut s,
        json!({"op":"experiment","instance":"ravine-crossing/1/1","design":out_of_zone}),
    );
    assert_eq!(r["outcome"].as_str(), Some("invalid"), "{r}");
    assert_eq!(r["remaining_steps"].as_u64(), Some(7200));
    let records = s.ledger.records();
    assert_eq!(records.len(), 2);
    assert_eq!(records[0].call.design.schema, "lab-design-raw/1");
    assert_eq!(records[1].call.design.schema, "lab-design/1");
    for r in records {
        assert_eq!(
            r.settlement.as_ref().unwrap().simulation_steps,
            ResourceUsage::Measured { value: 0 }
        );
    }
}

#[test]
fn one_normalized_output_is_retained_and_the_receipt_validates() {
    let p = plan(7200);
    let mut s = session(&p, "finish");
    ask(
        &mut s,
        json!({"op":"experiment","instance":"ravine-crossing/1/0","design":cart()}),
    );
    let r = ask(&mut s, json!({"op":"submit","design":cart()}));
    let digest = serde_json::from_str::<Design>(CART).unwrap().digest();
    assert_eq!(
        r["output"].as_str(),
        Some(digest.as_str()),
        "the output is the normalized design"
    );
    assert!(!ask(&mut s, json!({"op":"submit","design":cart()}))["ok"]
        .as_bool()
        .unwrap());
    let done = finish(
        &mut s,
        refs::environment(Mode::None, "Python 3"),
        false,
        ExecutionOutcome::OutputGenerated,
        ResourceUsage::Measured { value: 1234 },
    )
    .unwrap();
    let receipt = &done.receipt;
    assert_eq!(receipt.outputs.len(), 1);
    assert_eq!(receipt.outputs[0].sha256, digest);
    assert!(matches!(
        receipt.usage.model_calls,
        ResourceUsage::Unknown { .. }
    ));
    let transcript = std::fs::read(&done.transcript_path).unwrap();
    assert_eq!(refs::sha256_hex(&transcript), receipt.transcript.sha256);
    let scope = ExperimentScope {
        plan: &p,
        slot_id: "candidate",
    };
    on_disk(&s).validate_execution(scope, receipt).unwrap();
    let rows = assess_trials(
        &p,
        &[TrialEvidence {
            slot_id: "candidate".into(),
            execution: receipt.clone(),
            scoring: None,
        }],
    )
    .unwrap();
    assert_eq!(rows.len(), 2, "the control slot stays listed as missing");
    assert_eq!(
        rows[0].development_outcome,
        Some(ExecutionOutcome::OutputGenerated)
    );
    assert_eq!(rows[1].development_outcome, None);
    // A sealed session accepts no more experiments.
    let r = ask(
        &mut s,
        json!({"op":"experiment","instance":"ravine-crossing/1/0","design":cart()}),
    );
    assert!(!r["ok"].as_bool().unwrap());
}

#[test]
fn a_sealed_session_accepts_nothing_and_the_receipt_comes_last() {
    let p = plan(7200);
    let mut s = session(&p, "sealed");
    finish(
        &mut s,
        refs::environment(Mode::None, "Python 3"),
        false,
        ExecutionOutcome::Failed,
        ResourceUsage::Measured { value: 1 },
    )
    .unwrap();
    let before = s.store.read("run.json").unwrap();
    for req in [
        json!({"op":"submit","design":cart()}),
        json!({"op":"experiment","instance":"ravine-crossing/1/0","design":cart()}),
        json!({"op":"instances"}),
    ] {
        let r = ask(&mut s, req);
        assert_eq!(
            r["refused"].as_str(),
            Some("the execution is sealed"),
            "{r}"
        );
    }
    assert_eq!(s.store.read("run.json").unwrap(), before);
    let state: RunState = serde_json::from_slice(&before).unwrap();
    assert!(state.sealed && state.finished && state.submission.is_none());
}
