//! Live smoke test for the Elastic integration (docs/ELASTIC.md).
//!
//! Reads the same environment variables as the server and exercises, against
//! the real project: evidence indexing and search, capability indexing and
//! search, Agent Builder specialist creation, reconciliation and readiness,
//! and optionally one converse turn. Prints IDs and outcomes, never keys.
//!
//! ```sh
//! cargo run -p genesis-elastic --bin genesis-elastic-smoke -- --org smoke-org --converse --cleanup
//! ```

use genesis_core::engine::RoleToProvision;
use genesis_core::ids::{ChangeId, OrgId, RoleId};
use genesis_core::model::{AuthorityGrant, Provider};
use genesis_elastic::*;
use genesis_providers::{ProvisionRequest, RoleProvisioner};
use serde_json::{json, Value};
use std::collections::BTreeMap;
use std::time::Duration;

struct Report {
    steps: Vec<Value>,
    failed: bool,
}

impl Report {
    fn step(&mut self, name: &str, ok: bool, detail: Value) {
        println!("[{}] {name}: {detail}", if ok { "PASS" } else { "FAIL" });
        self.failed |= !ok;
        self.steps
            .push(json!({"step": name, "ok": ok, "detail": detail}));
    }
}

fn arg(name: &str) -> Option<String> {
    let args: Vec<String> = std::env::args().collect();
    args.iter()
        .position(|a| a == name)
        .and_then(|i| args.get(i + 1).cloned())
}

fn flag(name: &str) -> bool {
    std::env::args().any(|a| a == name)
}

#[tokio::main]
async fn main() {
    let _ = dotenvy::dotenv(); // keys may live in a git-ignored .env
    let org = arg("--org").unwrap_or_else(|| "genesis-smoke".into());
    let run = format!("{}", chrono::Utc::now().timestamp());
    let token = format!("smoketoken{run}");
    let mut r = Report {
        steps: vec![],
        failed: false,
    };

    // 1. Data plane.
    match ElasticConfig::from_env().map(ElasticClient::new) {
        None => r.step(
            "data config",
            false,
            json!("set ELASTICSEARCH_URL and ELASTIC_DATA_API_KEY"),
        ),
        Some(Err(e)) => r.step("data config", false, json!(e.to_string())),
        Some(Ok(es)) => {
            r.step("data config", true, json!({"evidence_index": es.evidence_index(&org), "capability_index": es.capability_index()}));
            let doc = EvidenceDoc {
                doc_id: format!("{org}:observation:smoke-{run}"),
                org_id: org.clone(),
                kind: DocKind::Observation,
                title: format!("Smoke observation {token}"),
                body: "Written by genesis-elastic-smoke".into(),
                mission_id: None,
                status: None,
                revision: 1,
                refs: BTreeMap::new(),
            };
            let indexed = async {
                es.ensure_indices(&org).await?;
                es.index_evidence(&org, &[doc]).await
            }
            .await;
            r.step(
                "evidence index",
                indexed.is_ok(),
                json!(indexed.map_err(|e| e.to_string())),
            );
            let mut found = None;
            for _ in 0..20 {
                if let Ok(h) = es
                    .search_evidence(&org, &token, &EvidenceFilter::default(), 5)
                    .await
                {
                    if let Some(hit) = h.into_iter().next() {
                        found = Some(hit.doc.doc_id);
                        break;
                    }
                }
                tokio::time::sleep(Duration::from_millis(500)).await;
            }
            r.step("evidence search", found.is_some(), json!({"doc_id": found}));
            let cap = CapabilityDoc {
                doc_id: format!("smoke-cap-{run}@0.0.1"),
                capability_id: format!("smoke-cap-{run}"),
                version: "0.0.1".into(),
                publisher: org.clone(),
                intent: format!("Smoke capability {token}"),
                applicability_conditions: vec![],
                known_limitations: vec!["smoke test document".into()],
                required_permissions: vec![],
                source_digest: "smoke".into(),
                state: "PUBLISHED".into(),
            };
            let indexed = es.index_capabilities(&[cap]).await;
            r.step(
                "capability index",
                indexed.is_ok(),
                json!(indexed.map_err(|e| e.to_string())),
            );
            let mut cap_found = None;
            for _ in 0..20 {
                if let Ok(h) = es.search_capabilities(&token, None, 5).await {
                    if let Some(hit) = h.into_iter().next() {
                        cap_found = Some(hit.doc.capability_id);
                        break;
                    }
                }
                tokio::time::sleep(Duration::from_millis(500)).await;
            }
            r.step(
                "capability search",
                cap_found.is_some(),
                json!({"capability_id": cap_found}),
            );
        }
    }

    // 2. Agent Builder.
    match AgentBuilderConfig::from_env().map(AgentBuilder::new) {
        None => r.step(
            "agent builder config",
            false,
            json!("set ELASTIC_KIBANA_URL and ELASTIC_MANAGEMENT_API_KEY"),
        ),
        Some(Err(e)) => r.step("agent builder config", false, json!(e.to_string())),
        Some(Ok(ab)) => {
            r.step(
                "agent builder config",
                true,
                json!({"evidence_tool": ab.evidence_tool_id(&org)}),
            );
            let req = ProvisionRequest {
                org_id: OrgId::new(&org),
                change_id: ChangeId::new(format!("smoke-{run}")),
                role: RoleToProvision {
                    key: "smoke".into(),
                    role_id: RoleId::new(format!("role-smoke-{run}")),
                    name: "Smoke specialist".into(),
                    charter: "Answer questions from the organization's evidence".into(),
                    authority: AuthorityGrant::default(),
                    provider: Provider::Elastic,
                },
            };
            let created = ab.provision(&req).await;
            r.step(
                "create specialist",
                created.is_ok(),
                json!(created
                    .as_ref()
                    .map(|b| &b.provider_agent_id)
                    .map_err(|e| e.to_string())),
            );
            if let Ok(binding) = created {
                let again = ab.provision(&req).await;
                r.step(
                    "reconcile (no duplicate)",
                    again
                        .as_ref()
                        .is_ok_and(|b| b.provider_agent_id == binding.provider_agent_id),
                    json!(again
                        .map(|b| b.provider_agent_id)
                        .map_err(|e| e.to_string())),
                );
                let ready = ab.check_ready(&binding).await;
                r.step(
                    "readiness",
                    matches!(ready, Ok(true)),
                    json!(ready.map_err(|e| e.to_string())),
                );
                if flag("--converse") {
                    let turn = ab
                        .converse(
                            &binding.provider_agent_id,
                            &format!("Search the organization's evidence for {token} and say what you find, citing IDs."),
                            None,
                        )
                        .await;
                    let ok = turn.as_ref().is_ok_and(|t| {
                        t.tool_calls.iter().any(|c| c.ends_with("-evidence-search"))
                    });
                    r.step(
                        "converse uses evidence tool",
                        ok,
                        match turn {
                            Ok(t) => json!({"conversation_id": t.conversation_id, "status": t.status,
                                            "tool_calls": t.tool_calls,
                                            "message_excerpt": t.message.chars().take(300).collect::<String>()}),
                            Err(e) => json!(e.to_string()),
                        },
                    );
                }
                if flag("--cleanup") {
                    let a = ab.delete("agents", &binding.provider_agent_id).await;
                    r.step(
                        "cleanup agent",
                        a.is_ok(),
                        json!(a.map_err(|e| e.to_string())),
                    );
                }
            }
        }
    }

    let report = json!({"org": org, "run": run, "steps": r.steps, "ok": !r.failed});
    if let Some(path) = arg("--report") {
        let _ = std::fs::write(
            &path,
            serde_json::to_string_pretty(&report).unwrap_or_default(),
        );
        println!("report written to {path}");
    }
    println!(
        "{}",
        if r.failed {
            "SMOKE TEST FAILED"
        } else {
            "SMOKE TEST PASSED"
        }
    );
    std::process::exit(if r.failed { 1 } else { 0 });
}
