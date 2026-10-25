//! Independent adversarial check of the hidden-scoring closure gate (#57/#214).
//! Runs real sandboxed executions, then asks whether `score_hidden` can be
//! driven to score without executor-observed whole-tree termination evidence.
//! Namespace cases need root; `none` controls show the gate's dependence on
//! observed termination. Linux only.
#![cfg(target_os = "linux")]
use genesis_core::development_method::{ExecutionReceipt, ResourceGrant};
use genesis_core::development_package::{MethodPackage, MethodSourceFile};
use genesis_lab_eval::commit::Descriptor;
use genesis_lab_eval::family::Family;
use genesis_lab_exec::run::{self, Config, Termination};
use genesis_lab_exec::store::Store;
use genesis_lab_exec::{example, refs, sandbox::Mode};
use genesis_lab_score::hidden::{comparison_plan, record_termination, score_hidden};
use genesis_lab_score::ScoringError;
use std::path::{Path, PathBuf};

const SALT: &str = "indep-salt-9c21-αβ";

fn euid_is_root() -> bool {
    std::fs::read_to_string("/proc/self/status")
        .unwrap_or_default()
        .lines()
        .find(|l| l.starts_with("Uid:"))
        .and_then(|l| l.split_whitespace().nth(2))
        .map(|v| v == "0")
        .unwrap_or(false)
}

fn methods_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../executor/methods")
}

fn package(dir: &Path, id: &str, entry: &str, version: &str) -> MethodPackage {
    fn walk(root: &Path, dir: &Path, out: &mut Vec<MethodSourceFile>) {
        for e in std::fs::read_dir(dir).unwrap() {
            let path = e.unwrap().path();
            if path.is_dir() {
                walk(root, &path, out);
            } else {
                out.push(MethodSourceFile {
                    path: path
                        .strip_prefix(root)
                        .unwrap()
                        .to_string_lossy()
                        .into_owned(),
                    content: std::fs::read_to_string(&path).unwrap(),
                });
            }
        }
    }
    let mut files = vec![];
    walk(dir, dir, &mut files);
    MethodPackage::new(
        id.into(),
        "0.1.0".into(),
        refs::runtime(version),
        refs::protocol(),
        entry.into(),
        vec![],
        files,
    )
    .unwrap()
}

struct Ran {
    receipt: ExecutionReceipt,
    output: Vec<u8>,
    termination: Termination,
}

/// Runs candidate+control of one comparison plan and returns per-slot evidence.
fn run_comparison(
    mode: Mode,
    tag: &str,
) -> (
    genesis_core::development_trial::TrialPlan,
    Descriptor,
    Ran,
    Ran,
) {
    let version = run::interpreter_version("python3");
    let cand_pkg = package(
        &methods_dir().join("fixed-cart"),
        &format!("indep-cand-{tag}"),
        "fixed_cart.py",
        &version,
    );
    let ctrl_pkg = package(
        &methods_dir().join("random-search"),
        &format!("indep-ctrl-{tag}"),
        "random_search.py",
        &version,
    );
    let methods = example::ExampleMethods {
        candidate: cand_pkg.reference().unwrap(),
        control: ctrl_pkg.reference().unwrap(),
    };
    let grant = ResourceGrant {
        model_calls: 0,
        input_tokens: 0,
        output_tokens: 0,
        wall_time_ms: 30_000,
        simulation_steps: 7200,
    };
    let base = example::acceptance_plan(&methods, mode, &version, 2, grant);
    let d = Descriptor::new(&format!("indep-cmp-{tag}"), SALT, Family::RavineCrossing, 3);
    let plan = comparison_plan(&base, &format!("indep-trial-{tag}"), &d);

    let base_dir = std::env::temp_dir().join(format!("indep-hidden-{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&base_dir);
    let cfg = Config {
        sandbox: mode,
        interpreter_version: version,
        stage_root: if mode == Mode::Namespaces {
            PathBuf::from("/run/genesis-lab-indep").join(tag)
        } else {
            base_dir.join("stage")
        },
    };
    let run_slot = |slot: &str, pkg: &MethodPackage| -> Ran {
        let dir = base_dir.join(slot);
        let bytes = pkg.artifact_json().unwrap().into_bytes();
        let done = run::execute(&plan, slot, &bytes, &dir, &cfg).unwrap();
        let receipt = done.receipt;
        assert!(
            !receipt.outputs.is_empty(),
            "slot {slot} produced no output (outcome {:?})",
            receipt.outcome
        );
        let store = Store::open(&dir).unwrap();
        let output = store
            .read(&format!("artifacts/{}", receipt.outputs[0].sha256))
            .unwrap();
        let termination = run::termination(slot, &dir)
            .unwrap()
            .expect("a finished run has termination");
        Ran {
            receipt,
            output,
            termination,
        }
    };
    let cand = run_slot("candidate", &cand_pkg);
    let ctrl = run_slot("control", &ctrl_pkg);
    (plan, d, cand, ctrl)
}

fn fresh_store(tag: &str) -> (Store, PathBuf) {
    let dir = std::env::temp_dir().join(format!("indep-score-store-{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    (Store::open(&dir).unwrap(), dir)
}

fn is_refused(
    r: &Result<genesis_core::development_evaluation::EvaluationReceipt, ScoringError>,
) -> bool {
    matches!(r, Err(ScoringError::Refused(_)))
}

#[test]
fn namespaces_closure_scores_only_with_observed_whole_tree_termination() {
    if !euid_is_root() {
        eprintln!("skipped: namespace sandbox needs root");
        return;
    }
    let (plan, d, cand, ctrl) = run_comparison(Mode::Namespaces, "ns");
    // The executor observed whole-tree termination in namespaces mode.
    assert!(
        cand.termination.whole_tree,
        "candidate whole_tree must be observed true"
    );
    assert!(
        ctrl.termination.whole_tree,
        "control whole_tree must be observed true"
    );

    let closure: Vec<(&str, &ExecutionReceipt)> =
        vec![("candidate", &cand.receipt), ("control", &ctrl.receipt)];

    // Positive: proper closure with both whole-tree termination records -> scores.
    let (store, _d1) = fresh_store("pos");
    record_termination(&store, &cand.termination).unwrap();
    record_termination(&store, &ctrl.termination).unwrap();
    let ok = score_hidden(&plan, "candidate", &cand.output, &closure, &d, SALT, &store);
    let receipt = ok.unwrap_or_else(|e| panic!("proper closure should score, got {e:?}"));
    assert!(
        matches!(
            receipt.outcome,
            genesis_core::development_evaluation::EvaluationOutcome::Scored { .. }
        ),
        "proper closure must yield a Scored receipt, got {:?}",
        receipt.outcome
    );

    // Adversarial 1: a slot has no termination record -> refuse.
    let (store, _d2) = fresh_store("missing");
    record_termination(&store, &cand.termination).unwrap(); // control omitted
    let r = score_hidden(&plan, "candidate", &cand.output, &closure, &d, SALT, &store);
    assert!(
        is_refused(&r),
        "missing termination record must refuse, got {r:?}"
    );

    // Adversarial 2: termination present but whole_tree=false (parent-only) -> refuse.
    let (store, _d3) = fresh_store("partial");
    record_termination(&store, &cand.termination).unwrap();
    let mut partial = ctrl.termination.clone();
    partial.whole_tree = false;
    record_termination(&store, &partial).unwrap();
    let r = score_hidden(&plan, "candidate", &cand.output, &closure, &d, SALT, &store);
    assert!(
        is_refused(&r),
        "parent-only termination must refuse, got {r:?}"
    );

    // Adversarial 3: termination not bound to the exact receipt (sha mismatch) -> refuse.
    let (store, _d4) = fresh_store("sha");
    record_termination(&store, &cand.termination).unwrap();
    let mut wrong = ctrl.termination.clone();
    wrong.execution_sha256 = "0".repeat(64);
    record_termination(&store, &wrong).unwrap();
    let r = score_hidden(&plan, "candidate", &cand.output, &closure, &d, SALT, &store);
    assert!(
        is_refused(&r),
        "receipt-sha mismatch must refuse, got {r:?}"
    );

    // Adversarial 4: wrong salt (does not open the descriptor) -> refuse.
    let (store, _d5) = fresh_store("salt");
    record_termination(&store, &cand.termination).unwrap();
    record_termination(&store, &ctrl.termination).unwrap();
    let r = score_hidden(
        &plan,
        "candidate",
        &cand.output,
        &closure,
        &d,
        "not-the-salt",
        &store,
    );
    assert!(is_refused(&r), "wrong salt must refuse, got {r:?}");
}

#[test]
fn none_mode_is_never_certified_whole_tree_so_hidden_scoring_refuses() {
    // mode none reaps only the method process; descendants may survive, so the
    // executor must not certify whole-tree termination and hidden scoring must
    // refuse. Runs without root.
    let (plan, d, cand, ctrl) = run_comparison(Mode::None, "none");
    assert!(
        !cand.termination.whole_tree,
        "none mode must NOT certify whole_tree"
    );
    assert!(
        !ctrl.termination.whole_tree,
        "none mode must NOT certify whole_tree"
    );
    let closure: Vec<(&str, &ExecutionReceipt)> =
        vec![("candidate", &cand.receipt), ("control", &ctrl.receipt)];
    let (store, _d) = fresh_store("none");
    record_termination(&store, &cand.termination).unwrap();
    record_termination(&store, &ctrl.termination).unwrap();
    let r = score_hidden(&plan, "candidate", &cand.output, &closure, &d, SALT, &store);
    assert!(
        is_refused(&r),
        "none-mode (uncertified) termination must refuse, got {r:?}"
    );
}
