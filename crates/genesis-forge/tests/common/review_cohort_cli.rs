//! The only subprocess is the offline cohort CLI over inert JSON fixtures.
use super::*;
use std::{
    fs,
    path::PathBuf,
    process::{Command, Output},
    sync::atomic::{AtomicU64, Ordering},
};
static NEXT: AtomicU64 = AtomicU64::new(0);
struct Files(PathBuf);
impl Files {
    fn new(r: &[RetainedSelection; 8]) -> Self {
        let path = std::env::temp_dir().join(format!(
            "review-cohort-cli-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir(&path).unwrap();
        fs::create_dir(path.join("inputs")).unwrap();
        fs::write(
            path.join("inputs/declaration.json"),
            serde_json::to_vec(&declaration(r)).unwrap(),
        )
        .unwrap();
        for (i, selection) in r.iter().enumerate() {
            fs::write(
                path.join(format!("inputs/{i}.json")),
                serde_json::to_vec(selection).unwrap(),
            )
            .unwrap();
        }
        // Arrival order differs deliberately from declared and output order.
        fs::write(path.join("inputs/manifest.json"), serde_json::to_vec(&json!({"schema":"genesis-process-cohort-input/1", "declaration":"declaration.json", "selections":(0..8).rev().map(|i|format!("{i}.json")).collect::<Vec<_>>()})).unwrap()).unwrap();
        Self(path)
    }
    fn run(&self, prepare: bool) -> Output {
        let mut command = Command::new(env!("CARGO_BIN_EXE_genesis-forge"));
        command.current_dir(&self.0).args([
            if prepare {
                "process-cohort-prepare"
            } else {
                "process-cohort-check"
            },
            "--cohort",
            "cohort.json",
        ]);
        if prepare {
            command.args(["--manifest", "inputs/manifest.json"]);
        }
        command.output().unwrap()
    }
}
impl Drop for Files {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

#[test]
fn review_cli_keeps_selected_full_references_and_rechecks_without_sources() {
    let mut r = records();
    let i = trial(&mut r[0], 2);
    let mut e = fixture::evidence(&i.plan, "candidate");
    let zero = ResourceUsage::Measured { value: 0 };
    e.execution.usage = Usage {
        model_calls: zero.clone(),
        input_tokens: zero.clone(),
        output_tokens: zero.clone(),
        wall_time_ms: zero.clone(),
        simulation_steps: zero,
    };
    let binding = i.plan.expected_binding("candidate").unwrap();
    let request = i
        .plan
        .evaluation_request_for_accounting("candidate", &e.execution)
        .unwrap()
        .unwrap();
    let metrics = serde_json::to_vec(&json!({"instances":i.plan.development_instances.iter().map(|r|json!({"instance":r.id,"success":false})).collect::<Vec<_>>()})).unwrap();
    let scoring = e.scoring.as_mut().unwrap();
    scoring.request_sha256 = request.identity_hash(&e.execution, &binding).unwrap();
    scoring.request = request;
    scoring.usage = e.execution.usage.clone();
    scoring.outcome = EvaluationOutcome::Scored {
        metrics: ExactContentRef {
            domain: "genesis-lab".into(),
            schema: "lab-scoring-metrics/1".into(),
            id: "lab-scoring-metrics/1".into(),
            version: "1".into(),
            sha256: format!("{:x}", Sha256::digest(&metrics)),
        },
    };
    let selected =
        json!({"round":3,"method":i.plan.slots[0].method,"plan":i.plan.reference().unwrap()});
    i.evidence = Some(e);
    i.metrics = Some(metrics);
    recapture(&mut r[0]);
    let f = Files::new(&r);
    let result = f.run(true);
    assert!(
        result.status.success(),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
    let summary: serde_json::Value = serde_json::from_slice(&result.stdout).unwrap();
    let cell = summary["selections"]
        .as_array()
        .unwrap()
        .iter()
        .find(|c| c["arm"] == "workflow" && c["family"] == "ravine-crossing")
        .unwrap();
    assert_eq!(cell["selected"], selected);
    assert_eq!(cell["selection_sha256"], r[0].sha256());
    assert_eq!(
        summary["selections"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|c| !c["selected"].is_null())
            .count(),
        1
    );
    let before = fs::read(f.0.join("cohort.json")).unwrap();
    let retained: serde_json::Value = serde_json::from_slice(&before).unwrap();
    assert_eq!(summary["cohort_sha256"], canonical_sha256(&retained));
    assert_eq!(
        summary["report_sha256"],
        canonical_sha256(&retained["report"])
    );
    fs::remove_dir_all(f.0.join("inputs")).unwrap();
    let check = f.run(false);
    assert!(check.status.success());
    assert_eq!(
        serde_json::from_slice::<serde_json::Value>(&check.stdout).unwrap(),
        summary
    );
    assert_eq!(fs::read(f.0.join("cohort.json")).unwrap(), before);
}

#[test]
fn review_aggregate_limit_refuses_individually_valid_selections_without_leftovers() {
    let mut r = records();
    for selection in &mut r {
        for round in 0..3 {
            trial(selection, round).plan.model_settings.id = "m".repeat(350_000);
        }
        recapture(selection);
        assert!(serde_json::to_vec(selection).unwrap().len() < 8 * 1024 * 1024);
    }
    assert!(assess_cohort(&declaration(&r), &r).is_ok());
    let f = Files::new(&r);
    let out = f.run(true);
    assert_eq!(out.status.code(), Some(2));
    assert!(String::from_utf8_lossy(&out.stderr).contains("8 MiB"));
    assert!(!f.0.join("cohort.json").exists());
    assert!(!f.0.join("cohort.json.lock").exists());
    assert_eq!(fs::read_dir(&f.0).unwrap().count(), 1);
}

#[test]
fn review_empty_path_and_duplicate_json_keys_fail_without_publication() {
    for duplicate in [false, true] {
        let f = Files::new(&records());
        let path = f.0.join("inputs/manifest.json");
        let mut value: serde_json::Value =
            serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
        let bytes = if duplicate {
            let raw = serde_json::to_string(&value).unwrap();
            format!(
                "{{\"schema\":\"genesis-process-cohort-input/1\",{}",
                &raw[1..]
            )
            .into_bytes()
        } else {
            value["declaration"] = "".into();
            serde_json::to_vec(&value).unwrap()
        };
        fs::write(path, bytes).unwrap();
        assert_eq!(f.run(true).status.code(), Some(2));
        assert!(!f.0.join("cohort.json").exists());
        assert!(!f.0.join("cohort.json.lock").exists());
    }
}
