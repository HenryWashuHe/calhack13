//! Uses the real retained receiving records from #116 (B's revised child
//! against fixed cart: Accepted 3/6 vs 2/6; against random search: Rejected
//! 3/6 vs 4/6). They are copied into a temporary records root; nothing runs.
use super::*;
use genesis_core::development_trial::TrialPlan;

const EVIDENCE: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../docs/invention-lab/evidence"
);

pub(crate) fn copy_dir(from: &Path, to: &Path) {
    std::fs::create_dir_all(to).unwrap();
    for entry in std::fs::read_dir(from).unwrap() {
        let entry = entry.unwrap();
        let target = to.join(entry.file_name());
        if entry.file_type().unwrap().is_dir() {
            copy_dir(&entry.path(), &target);
        } else {
            std::fs::copy(entry.path(), target).unwrap();
        }
    }
}

/// B's revised child package (`4f455dc6…`), the candidate in both trials.
pub(crate) fn package() -> String {
    std::fs::read_to_string(format!(
        "{EVIDENCE}/live-method-20261024-b-revision/method-package.json"
    ))
    .unwrap()
}

/// A temporary records root holding both retained trials, and a receiver
/// configured for exactly the runtime, protocol, environment and evaluator
/// those trials used.
pub(crate) struct Records {
    pub(crate) dir: tempfile::TempDir,
    pub(crate) receiver: MethodReceiver,
    pub(crate) reference: ExactContentRef,
}

impl Records {
    pub(crate) fn new() -> Self {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().join("records");
        let receiving = format!("{EVIDENCE}/live-method-20261024-b-revision-receiver");
        for trial in ["vs-fixed-cart", "vs-random-search"] {
            copy_dir(&Path::new(&receiving).join(trial), &root.join(trial));
        }
        let plan: TrialPlan =
            serde_json::from_slice(&std::fs::read(root.join("vs-fixed-cart/plan.json")).unwrap())
                .unwrap();
        let envelope: serde_json::Value = serde_json::from_str(&package()).unwrap();
        let field = |k: &str| serde_json::from_value(envelope["package"][k].clone()).unwrap();
        let receiver = MethodReceiver {
            runtime: field("runtime"),
            protocol: field("protocol"),
            environments: vec![plan.development_environment.clone()],
            evaluators: vec![plan.context.evaluator.clone()],
            allowed_dependencies: vec![],
            records_root: root,
            acceptance_plans: vec![],
        };
        let reference = plan.slots[0].method.clone();
        Self {
            dir,
            receiver,
            reference,
        }
    }
    pub(crate) fn selected(&self) -> SelectedMethod {
        SelectedMethod {
            publisher: OrgId::new("org_example_a"),
            reference: self.reference.clone(),
            package_json: package(),
        }
    }
    fn assess(&self, trial: &str) -> Result<AcceptanceEvidence, MethodAcceptanceError> {
        assess_selected(
            &self.receiver,
            &OrgId::new("org_example_b"),
            &self.selected(),
            trial,
        )
    }
}

#[test]
fn retained_records_recompute_both_real_verdicts() {
    let f = Records::new();
    let accepted = f.assess("vs-fixed-cart").unwrap();
    assert_eq!(
        accepted.acceptance,
        Acceptance::Accepted {
            candidate: 3,
            control: 2,
            of: 6
        }
    );
    assert_eq!(accepted.public_task.id, "rough-transport");
    assert!(accepted.files.contains_key("plan.json"));
    assert!(accepted
        .files
        .keys()
        .any(|k| k.starts_with("candidate-score/artifacts/")));
    let rejected = f.assess("vs-random-search").unwrap();
    assert!(matches!(rejected.acceptance, Acceptance::Rejected { .. }));
}

#[test]
fn records_that_do_not_belong_to_the_selection_or_receiver_are_errors() {
    let f = Records::new();
    for trial in ["", "../vs-fixed-cart", ".hidden", "a/b"] {
        assert_eq!(
            f.assess(trial).unwrap_err(),
            MethodAcceptanceError::TrialName
        );
    }
    assert!(matches!(
        f.assess("absent").unwrap_err(),
        MethodAcceptanceError::Record(_)
    ));
    let own = SelectedMethod {
        publisher: OrgId::new("org_example_b"),
        ..f.selected()
    };
    let org = OrgId::new("org_example_b");
    assert_eq!(
        assess_selected(&f.receiver, &org, &own, "vs-fixed-cart").unwrap_err(),
        MethodAcceptanceError::OwnPublication
    );
    let mut other = f.selected();
    other.reference.sha256 = "0".repeat(64);
    assert!(matches!(
        assess_selected(&f.receiver, &org, &other, "vs-fixed-cart").unwrap_err(),
        MethodAcceptanceError::Binding(_)
    ));
    assert!(matches!(
        assess_selected(
            &f.receiver,
            &OrgId::new("org_example_c"),
            &f.selected(),
            "vs-fixed-cart"
        )
        .unwrap_err(),
        MethodAcceptanceError::Binding(_)
    ));
    for change in 0..3 {
        let mut receiver = f.receiver.clone();
        match change {
            0 => receiver.runtime.sha256 = "1".repeat(64),
            1 => receiver.environments.clear(),
            _ => receiver.evaluators.clear(),
        }
        assert!(matches!(
            assess_selected(&receiver, &org, &f.selected(), "vs-fixed-cart").unwrap_err(),
            MethodAcceptanceError::Incompatible(_)
        ));
    }
    // A changed metrics byte no longer matches its scoring receipt.
    let read = f.assess("vs-fixed-cart").unwrap().files;
    let named = read
        .keys()
        .find(|k| k.starts_with("candidate-score/artifacts/"))
        .unwrap();
    let metrics = f.receiver.records_root.join("vs-fixed-cart").join(named);
    let mut bytes = std::fs::read(&metrics).unwrap();
    let at = bytes.iter().position(|b| *b == b'e').unwrap();
    bytes[at] = b'E';
    std::fs::write(&metrics, bytes).unwrap();
    let err = f.assess("vs-fixed-cart").unwrap_err();
    assert!(
        matches!(
            err,
            MethodAcceptanceError::Binding(_) | MethodAcceptanceError::Record(_)
        ),
        "{err:?}"
    );
}

#[test]
fn missing_scoring_is_insufficient_and_record_bounds_are_exact() {
    let f = Records::new();
    let root = f.receiver.records_root.clone();
    std::fs::remove_file(root.join("vs-random-search/control-score/evaluation.json")).unwrap();
    assert!(matches!(
        f.assess("vs-random-search").unwrap().acceptance,
        Acceptance::Insufficient { .. }
    ));
    for (len, ok) in [(128, true), (129, false)] {
        let name = "t".repeat(len);
        copy_dir(&root.join("vs-fixed-cart"), &root.join(&name));
        let result = f.assess(&name);
        assert_eq!(result.is_ok(), ok, "{len}: {result:?}");
    }
    let plan = root.join("vs-fixed-cart/plan.json");
    let mut bytes = std::fs::read(&plan).unwrap();
    bytes.resize(1024 * 1024, b' ');
    std::fs::write(&plan, &bytes).unwrap();
    assert!(f.assess("vs-fixed-cart").is_ok());
    bytes.push(b' ');
    std::fs::write(&plan, &bytes).unwrap();
    assert_eq!(
        f.assess("vs-fixed-cart").unwrap_err(),
        MethodAcceptanceError::Record("plan.json".into())
    );
}

#[cfg(unix)]
#[test]
fn a_symlinked_retained_record_is_refused() {
    let f = Records::new();
    let trial = f.receiver.records_root.join("vs-fixed-cart");
    std::fs::rename(
        trial.join("criterion.json"),
        f.dir.path().join("criterion.json"),
    )
    .unwrap();
    std::os::unix::fs::symlink(
        f.dir.path().join("criterion.json"),
        trial.join("criterion.json"),
    )
    .unwrap();
    assert_eq!(
        f.assess("vs-fixed-cart").unwrap_err(),
        MethodAcceptanceError::Record("criterion.json".into())
    );
    let root = &f.receiver.records_root;
    std::os::unix::fs::symlink(root.join("vs-random-search"), root.join("linked")).unwrap();
    assert!(matches!(
        f.assess("linked").unwrap_err(),
        MethodAcceptanceError::Record(_)
    ));
}
