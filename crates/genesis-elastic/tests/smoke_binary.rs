//! The live smoke-test binary, run against the in-process fakes.

use genesis_elastic::fake;
use std::process::Command;

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn smoke_binary_passes_against_fakes_and_prints_no_keys() {
    let (es, _) = fake::start().await;
    let (kibana_url, kibana) = fake::start_kibana().await;
    let es_url = es.elasticsearch_url().to_owned();
    let out = tokio::task::spawn_blocking(move || {
        Command::new(env!("CARGO_BIN_EXE_genesis-elastic-smoke"))
            .args(["--org", "smoke-org", "--converse", "--cleanup"])
            .env("ELASTICSEARCH_URL", es_url)
            .env("ELASTIC_DATA_API_KEY", "data-key-canary")
            .env("ELASTIC_KIBANA_URL", kibana_url)
            .env("ELASTIC_MANAGEMENT_API_KEY", "mgmt-key-canary")
            .output()
            .unwrap()
    })
    .await
    .unwrap();
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(
        out.status.success(),
        "{stdout}\n{}",
        String::from_utf8_lossy(&out.stderr)
    );
    for step in [
        "evidence search",
        "capability search",
        "create specialist",
        "reconcile (no duplicate)",
        "readiness",
        "converse uses evidence tool",
        "cleanup agent",
    ] {
        assert!(
            stdout.contains(&format!("[PASS] {step}")),
            "missing {step}:\n{stdout}"
        );
    }
    assert!(!stdout.contains("key-canary"));
    assert!(
        kibana.lock().unwrap().agents.is_empty(),
        "cleanup removed the agent"
    );
}
