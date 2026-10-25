//! Invalid-range preflight only: no method, simulator, interpreter or sandbox runs.
use std::{
    fs,
    process::Command,
    time::{SystemTime, UNIX_EPOCH},
};

#[test]
fn invalid_seed_ranges_fail_before_reading_a_candidate_or_creating_output() {
    let root = std::env::temp_dir().join(format!(
        "genesis-seed-preflight-{}-{}",
        std::process::id(),
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    fs::create_dir(&root).unwrap();
    for (i, (first, count, expected)) in [
        (1_u64 << 63, 1, "reserved hidden-seed domain"),
        ((1_u64 << 63) - 1, 2, "reserved hidden-seed domain"),
        (u64::MAX, 1, "overflows"),
    ]
    .into_iter()
    .enumerate()
    {
        let output_dir = root.join(format!("output-{i}"));
        let missing_candidate = root.join("candidate-must-not-be-read.json");
        let output = Command::new(env!("CARGO_BIN_EXE_lab-exec"))
            .args(["example-plan", "none", &count.to_string(), "2400", "10000"])
            .arg(&output_dir)
            .arg("rough-transport")
            .arg(&missing_candidate)
            .arg(first.to_string())
            .output()
            .unwrap();
        assert!(!output.status.success());
        assert!(String::from_utf8_lossy(&output.stderr).contains(expected));
        assert!(
            !output_dir.exists(),
            "range validation must precede all output writes"
        );
        assert!(output.stdout.is_empty());
    }
    assert_eq!(fs::read_dir(&root).unwrap().count(), 0);
    fs::remove_dir(&root).unwrap();
}
