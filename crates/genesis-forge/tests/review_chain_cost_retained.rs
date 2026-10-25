//! Read-only historical compatibility; the retained method is never executed.
use serde_json::{json, Value};
use std::{
    collections::BTreeMap,
    fs,
    path::{Path, PathBuf},
    process::Command,
};

fn snapshot(path: &Path) -> BTreeMap<PathBuf, Vec<u8>> {
    let mut result = BTreeMap::new();
    for entry in fs::read_dir(path).unwrap() {
        let path = entry.unwrap().path();
        if path.is_dir() {
            result.extend(snapshot(&path));
        } else {
            result.insert(path.clone(), fs::read(path).unwrap());
        }
    }
    result
}

struct Temp(PathBuf);
impl Drop for Temp {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

#[test]
fn historical_b_source_chain_keeps_exact_generation_costs_and_unresolved_trial() {
    let evidence = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../docs/invention-lab/evidence/live-method-20261024-b-revision");
    let before = snapshot(&evidence);
    let read = |name: &str| -> Value {
        serde_json::from_slice(&fs::read(evidence.join(name)).unwrap()).unwrap()
    };
    let allocation = read("allocation.json");
    let roster = read("roster.json");
    let chain = read("chain.json");
    let temporary = Temp(std::env::temp_dir().join(format!(
        "genesis-chain-retained-{}-{}", std::process::id(),
        std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos()
    )));
    fs::create_dir(&temporary.0).unwrap();
    let write = |name: &str, value: Value| {
        fs::write(temporary.0.join(name), serde_json::to_vec(&value).unwrap()).unwrap();
    };
    // This is a new read-only view of an already retained source-only chain,
    // not a retroactive declaration of the receiving trials or their costs.
    write(
        "scope.json",
        json!({"id":"historical-source-only-audit",
        "generations":roster["attempts"], "trial":null, "generated_links":[],
        "total_grant":allocation["limits"]["total_grant"]}),
    );
    write(
        "cost.json",
        json!({"schema":"genesis-method-cost-input/1",
        "scope":"scope.json", "generation_checkpoints":[evidence.join("generation.json")],
        "trial":null}),
    );
    write(
        "input.json",
        json!({"schema":"genesis-chain-cost-input/1","stages":[{
        "stage_id":chain["stages"][0]["limits"]["id"], "target":{
            "kind":"generation", "cost_manifest":"cost.json",
            "allocation":evidence.join("allocation.json"),
            "admission":evidence.join("admission.json")}}]}),
    );
    let output = Command::new(env!("CARGO_BIN_EXE_genesis-forge"))
        .arg("method-chain-cost-assess")
        .arg("--chain")
        .arg(evidence.join("chain.json"))
        .arg("--manifest")
        .arg(temporary.0.join("input.json"))
        .env_remove("ANTHROPIC_API_KEY")
        .env_remove("GENESIS_LLM_API_KEY")
        .env("GENESIS_LLM_PROVIDER", "invalid-no-provider-access")
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let report: Value = serde_json::from_slice(&output.stdout).unwrap();
    let totals = report["combined"].as_array().unwrap();
    assert_eq!(
        totals
            .iter()
            .map(|r| r["known_lower_bound"].as_u64().unwrap())
            .collect::<Vec<_>>(),
        [1, 20798, 5741, 40223, 0]
    );
    assert!(totals
        .iter()
        .all(|r| r["unknown"] == true && r["overflow"] == false && r["limit_exceeded"] == false));
    assert_eq!(report["stages"].as_array().unwrap().len(), 1);
    assert_eq!(report["stages"][0]["unresolved_trial"], true);
    assert_eq!(
        report["stages"][0]["observed"]["report"]["admission"]["reserved_attempts"],
        1
    );
    assert_eq!(snapshot(&evidence), before);
}
