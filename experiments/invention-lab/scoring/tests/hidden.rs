//! Comparison scoring on a committed hidden set, after every arm's whole
//! process tree has ended. Arms run as real child processes. Scored paths need
//! the namespace sandbox (root, Linux; skipped otherwise), whose termination
//! covers the whole PID namespace; mode `none` only shows that a parent-only
//! termination is refused. The methods only submit a fixed design.
use genesis_core::development_evaluation::{EvaluationOutcome, RequestStatus};
use genesis_core::development_method::{
    ExecutionOutcome, ExecutionReceipt, ResourceGrant, ResourceUsage,
};
use genesis_core::development_package::{MethodPackage, MethodSourceFile};
use genesis_core::development_trial::{assess_trials, TrialEvidence, TrialPlan};
use genesis_lab_eval::commit::Descriptor;
use genesis_lab_eval::family::Family;
use genesis_lab_exec::run::{self, Termination};
use genesis_lab_exec::store::Store;
use genesis_lab_exec::{example, refs, sandbox::Mode};
use genesis_lab_score::hidden::{comparison_plan, record_termination, score_hidden};
use genesis_lab_score::ScoringError;
use std::path::PathBuf;

const REFERENCE: &str = include_str!("../../evaluator/designs/reference-five-wheel.json");
const SALT: &str = "salt-held-by-the-evaluator-owner";

fn descriptor(count: u32) -> Descriptor {
    Descriptor::new("lab-comparison-1", SALT, Family::RavineCrossing, count)
}

fn dir(name: &str) -> PathBuf {
    std::env::temp_dir().join(format!("lab-hidden-{name}-{}", std::process::id()))
}

/// Root on Linux, from /proc (no libc dependency); false elsewhere.
fn root() -> bool {
    std::fs::read_to_string("/proc/self/status")
        .ok()
        .and_then(|s| {
            s.lines()
                .find(|l| l.starts_with("Uid:"))
                .and_then(|l| l.split_whitespace().nth(2).map(|v| v == "0"))
        })
        .unwrap_or(false)
}

/// The namespace sandbox when available, else `None` for the caller to skip.
fn sandboxed() -> Option<Mode> {
    if root() {
        Some(Mode::Namespaces)
    } else {
        eprintln!("skipped: whole-tree termination needs the namespace sandbox (root)");
        None
    }
}

fn config(name: &str, mode: Mode) -> run::Config {
    run::Config {
        sandbox: mode,
        interpreter_version: run::interpreter_version("python3"),
        stage_root: match mode {
            // Outside the directories the sandbox hides.
            Mode::Namespaces => PathBuf::from("/run/genesis-lab-test"),
            Mode::None => dir(name).join("stage"),
        },
    }
}

/// A method that submits `design` (or exits without submitting) and stops.
fn method(cfg: &run::Config, design: Option<&str>) -> MethodPackage {
    let source = match design {
        Some(d) => format!(
            "import json, sys\nprint(json.dumps({{'op': 'submit', 'design': json.loads({})}}), flush=True)\nassert json.loads(sys.stdin.readline())['ok']\n",
            serde_json::to_string(d).unwrap()
        ),
        None => "pass\n".into(),
    };
    MethodPackage::new(
        "fixed-design".into(),
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
    .unwrap()
}

struct Arms {
    name: String,
    plan: TrialPlan,
    cand: ExecutionReceipt,
    ctrl: ExecutionReceipt,
    ended: Vec<Termination>,
}

/// Freezes a comparison over two fixed-design methods, runs both arms as
/// processes and collects the executor's termination evidence.
fn arms(
    mode: Mode,
    name: &str,
    count: u32,
    cand: Option<&str>,
    ctrl: Option<&str>,
    tweak: impl Fn(&mut TrialPlan),
) -> Arms {
    let _ = std::fs::remove_dir_all(dir(name));
    let cfg = config(name, mode);
    let (pc, pk) = (method(&cfg, cand), method(&cfg, ctrl));
    let methods = example::ExampleMethods {
        candidate: pc.reference().unwrap(),
        control: pk.reference().unwrap(),
    };
    let grant = ResourceGrant {
        model_calls: 0,
        input_tokens: 0,
        output_tokens: 0,
        wall_time_ms: 20_000,
        simulation_steps: 2400,
    };
    let base = example::acceptance_plan(&methods, mode, &cfg.interpreter_version, 2, grant);
    let mut plan = comparison_plan(&base, "t1", &descriptor(count));
    tweak(&mut plan);
    let mut receipts = vec![];
    let mut ended = vec![];
    for (slot, package) in [("candidate", &pc), ("control", &pk)] {
        let state = dir(name).join(slot);
        let bytes = package.artifact_json().unwrap();
        let done = run::execute(&plan, slot, bytes.as_bytes(), &state, &cfg).unwrap();
        ended.push(
            run::termination(slot, &state)
                .unwrap()
                .expect("executor saw the end"),
        );
        receipts.push(done.receipt);
    }
    let ctrl = receipts.pop().unwrap();
    let cand = receipts.pop().unwrap();
    Arms {
        name: name.into(),
        plan,
        cand,
        ctrl,
        ended,
    }
}

impl Arms {
    fn output(&self) -> Vec<u8> {
        std::fs::read(
            dir(&self.name)
                .join("candidate/artifacts")
                .join(&self.cand.outputs[0].sha256),
        )
        .unwrap()
    }
    fn closed(&self) -> [(&str, &ExecutionReceipt); 2] {
        [("candidate", &self.cand), ("control", &self.ctrl)]
    }
    /// A fresh scorer store, with the executor's termination evidence recorded.
    fn store(&self, tag: &str) -> Store {
        let store = bare_store(&format!("{}-{tag}", self.name));
        for t in &self.ended {
            record_termination(&store, t).unwrap();
        }
        store
    }
}

fn bare_store(name: &str) -> Store {
    let d = dir(name).join("score");
    let _ = std::fs::remove_dir_all(&d);
    Store::open(&d).unwrap()
}

fn nothing_simulated(store: &Store) -> bool {
    std::fs::read_dir(store.path("artifacts"))
        .map(|mut d| d.next().is_none())
        .unwrap_or(true)
}

#[test]
fn a_closed_comparison_is_scored_on_the_committed_draw_and_validates() {
    let Some(mode) = sandboxed() else { return };
    let a = arms(mode, "ok", 3, Some(REFERENCE), None, |_| {});
    let p = &a.plan;
    p.validate().unwrap();
    assert!(
        p.development_instances
            .iter()
            .all(|r| refs::resolve_instance(r).is_some()),
        "development stays on public instances"
    );
    assert_eq!(a.cand.outcome, ExecutionOutcome::OutputGenerated);
    let store = a.store("scored");
    let receipt = score_hidden(
        p,
        "candidate",
        &a.output(),
        &a.closed(),
        &descriptor(3),
        SALT,
        &store,
    )
    .unwrap();
    let EvaluationOutcome::Scored { metrics } = &receipt.outcome else {
        panic!("{:?}", receipt.outcome)
    };
    let m: serde_json::Value = serde_json::from_slice(
        &store
            .read(&format!("artifacts/{}", metrics.sha256))
            .unwrap(),
    )
    .unwrap();
    assert_eq!(m["of"], 3);
    let ids: Vec<_> = m["instances"]
        .as_array()
        .unwrap()
        .iter()
        .map(|r| r["instance"].as_str().unwrap().to_owned())
        .collect();
    assert_eq!(ids, ["hidden/0", "hidden/1", "hidden/2"]);

    // The private trace names hidden seeds under the published commitment.
    let trace: serde_json::Value = serde_json::from_slice(
        &store
            .read(&format!("artifacts/{}", receipt.trace.sha256))
            .unwrap(),
    )
    .unwrap();
    for row in trace["runs"].as_array().unwrap() {
        let r = &row["receipt"];
        assert_eq!(r["hidden_commitment"], descriptor(3).commitment.as_str());
        assert!(genesis_lab_eval::score::is_hidden_seed(
            r["seed"].as_u64().unwrap()
        ));
    }
    match receipt.usage.simulation_steps {
        ResourceUsage::Measured { value } => assert!(value > 0 && value <= 3 * 2400),
        ref other => panic!("{other:?}"),
    }

    let rows = assess_trials(
        p,
        &[
            TrialEvidence {
                slot_id: "candidate".into(),
                execution: a.cand.clone(),
                scoring: Some(receipt),
            },
            TrialEvidence {
                slot_id: "control".into(),
                execution: a.ctrl.clone(),
                scoring: None,
            },
        ],
    )
    .unwrap();
    assert_eq!(rows[0].scoring_status, RequestStatus::Scored);
    assert_eq!(rows[1].scoring_status, RequestStatus::Missing);
}

#[test]
fn hidden_scoring_refuses_before_simulating_anything() {
    let Some(mode) = sandboxed() else { return };
    let a = arms(mode, "refuse", 3, Some(REFERENCE), Some(REFERENCE), |_| {});
    let p = &a.plan;
    let bytes = a.output();
    let closed = a.closed();
    let (cand, ctrl) = (&a.cand, &a.ctrl);
    let refused = |tag: &str,
                   p: &TrialPlan,
                   closure: &[(&str, &ExecutionReceipt)],
                   d: &Descriptor,
                   salt: &str,
                   store: Store| {
        let r = score_hidden(p, "candidate", &bytes, closure, d, salt, &store);
        assert!(matches!(r, Err(ScoringError::Refused(_))), "{tag}: {r:?}");
        assert!(nothing_simulated(&store), "{tag}: simulated after refusing");
    };

    refused(
        "salt",
        p,
        &closed,
        &descriptor(3),
        "a guessed salt",
        a.store("salt"),
    );
    // A different descriptor than the plan pinned: another draw size, or the
    // same size under another comparison (which the step grant can't catch).
    refused(
        "descriptor",
        p,
        &closed,
        &descriptor(4),
        SALT,
        a.store("descriptor"),
    );
    let other = Descriptor::new("another-comparison", SALT, Family::RavineCrossing, 3);
    refused(
        "comparison",
        p,
        &closed,
        &other,
        SALT,
        a.store("comparison"),
    );
    // Not closed: the control arm is absent, appears twice, or a stranger.
    refused(
        "open",
        p,
        &[("candidate", cand)],
        &descriptor(3),
        SALT,
        a.store("open"),
    );
    refused(
        "twice",
        p,
        &[("candidate", cand), ("control", ctrl), ("control", ctrl)],
        &descriptor(3),
        SALT,
        a.store("twice"),
    );
    refused(
        "stranger",
        p,
        &[("candidate", cand), ("control", ctrl), ("other", ctrl)],
        &descriptor(3),
        SALT,
        a.store("stranger"),
    );
    // Not closed: no termination evidence at all, only one arm's, or
    // evidence bound to a different receipt.
    refused(
        "unattested",
        p,
        &closed,
        &descriptor(3),
        SALT,
        bare_store("refuse-unattested"),
    );
    let one = bare_store("refuse-one");
    record_termination(&one, &a.ended[0]).unwrap();
    refused("one", p, &closed, &descriptor(3), SALT, one);
    let foreign = bare_store("refuse-foreign");
    record_termination(&foreign, &a.ended[0]).unwrap();
    record_termination(
        &foreign,
        &Termination {
            execution_sha256: a.ended[0].execution_sha256.clone(),
            ..a.ended[1].clone()
        },
    )
    .unwrap();
    refused("foreign", p, &closed, &descriptor(3), SALT, foreign);
    // An acceptance plan is never scored on the hidden set.
    let mut acceptance = p.clone();
    acceptance.scoring_instances =
        genesis_core::development_trial::ScoringInstances::Development {};
    refused(
        "acceptance",
        &acceptance,
        &closed,
        &descriptor(3),
        SALT,
        a.store("acceptance"),
    );
    // Substituted output bytes.
    let store = a.store("bytes");
    let mut other: serde_json::Value = serde_json::from_str(REFERENCE).unwrap();
    other["bodies"][0]["w"] = 3.0.into();
    assert!(matches!(
        score_hidden(
            p,
            "candidate",
            &serde_json::to_vec(&other).unwrap(),
            &closed,
            &descriptor(3),
            SALT,
            &store
        ),
        Err(ScoringError::Refused(_))
    ));
    assert!(nothing_simulated(&store));

    // No wall time to score with.
    let zero = arms(
        mode,
        "zero-wall",
        3,
        Some(REFERENCE),
        Some(REFERENCE),
        |p| p.scoring_grant.wall_time_ms = 0,
    );
    let store = zero.store("zero");
    assert!(matches!(
        score_hidden(
            &zero.plan,
            "candidate",
            &zero.output(),
            &zero.closed(),
            &descriptor(3),
            SALT,
            &store
        ),
        Err(ScoringError::Refused(_))
    ));
    assert!(nothing_simulated(&store));

    // One episode short of the draw.
    let short = arms(mode, "short", 3, Some(REFERENCE), Some(REFERENCE), |p| {
        p.scoring_grant.simulation_steps -= 1
    });
    let store = short.store("short");
    assert!(matches!(
        score_hidden(
            &short.plan,
            "candidate",
            &short.output(),
            &short.closed(),
            &descriptor(3),
            SALT,
            &store
        ),
        Err(ScoringError::Refused(_))
    ));
    assert!(nothing_simulated(&store));
}

#[test]
fn a_slot_without_output_has_nothing_to_score() {
    let Some(mode) = sandboxed() else { return };
    let a = arms(mode, "none", 2, None, Some(REFERENCE), |_| {});
    assert_eq!(a.cand.outcome, ExecutionOutcome::Failed);
    let store = a.store("none");
    assert!(matches!(
        score_hidden(
            &a.plan,
            "candidate",
            b"",
            &a.closed(),
            &descriptor(2),
            SALT,
            &store
        ),
        Err(ScoringError::NoOutput)
    ));
}

#[test]
fn a_parent_only_termination_does_not_close_the_comparison() {
    // Mode none: the executor reaps the method process but can't vouch for
    // processes it started, so hidden scoring stays closed.
    let a = arms(
        Mode::None,
        "parent-only",
        2,
        Some(REFERENCE),
        Some(REFERENCE),
        |_| {},
    );
    assert!(a.ended.iter().all(|t| !t.whole_tree));
    let store = a.store("parent-only");
    let r = score_hidden(
        &a.plan,
        "candidate",
        &a.output(),
        &a.closed(),
        &descriptor(2),
        SALT,
        &store,
    );
    assert!(matches!(r, Err(ScoringError::Refused(_))), "{r:?}");
    assert!(nothing_simulated(&store));
}
