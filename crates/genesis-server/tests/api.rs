//! End-to-end API tests: registry + independent organizations over HTTP,
//! using the labeled development stub for providers.

use genesis_core::capability::{
    ActorArtifact, CapabilityManifest, CaseKind, CaseResult, VerificationReport,
};
use genesis_core::ids::*;
use genesis_core::lifecycle::CapabilityState;
use genesis_core::model::{Constitution, Provider};
use genesis_providers::{FailingProvisioner, ProviderError, ProviderSet};
use genesis_server::registry::RegistryState;
use genesis_server::{build_org, parse_publishers, spawn_org, spawn_registry, OrgConfig};
use reqwest::StatusCode;
use serde_json::{json, Value};
use std::sync::Arc;
use std::time::Duration;

struct Org {
    url: String,
    operator: String,
    agent: String,
    adapter: String,
    http: reqwest::Client,
}

impl Org {
    async fn req(
        &self,
        method: reqwest::Method,
        path: &str,
        token: &str,
        body: Option<Value>,
    ) -> (StatusCode, Value) {
        let mut r = self
            .http
            .request(method, format!("{}/api/v1{}", self.url, path))
            .bearer_auth(token);
        if let Some(b) = body {
            r = r.json(&b);
        }
        let resp = r.send().await.expect("request");
        let status = resp.status();
        let text = resp.text().await.unwrap();
        (
            status,
            serde_json::from_str(&text).unwrap_or(Value::String(text)),
        )
    }
    async fn get(&self, path: &str) -> (StatusCode, Value) {
        self.req(reqwest::Method::GET, path, &self.operator, None)
            .await
    }
    async fn post(&self, path: &str, body: Value) -> (StatusCode, Value) {
        self.req(reqwest::Method::POST, path, &self.operator, Some(body))
            .await
    }
    async fn post_as(&self, token: &str, path: &str, body: Value) -> (StatusCode, Value) {
        self.req(reqwest::Method::POST, path, token, Some(body))
            .await
    }
    async fn state(&self) -> Value {
        self.get("/organization").await.1
    }
    async fn wait_change(&self, id: &str, terminal: &[&str]) -> Value {
        for _ in 0..100 {
            let (_, c) = self.get(&format!("/org-changes/{id}")).await;
            if terminal.contains(&c["status"].as_str().unwrap_or("")) {
                return c;
            }
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
        panic!("change {id} did not reach {terminal:?}");
    }
    async fn events(&self) -> Vec<Value> {
        self.get("/events/log?after=0&limit=5000")
            .await
            .1
            .as_array()
            .unwrap()
            .clone()
    }
}

async fn registry() -> String {
    let publishers =
        parse_publishers("org_a:org_a-registry,org_b:org_b-registry,org_c:org_c-registry");
    let reg = RegistryState::open_in_memory(publishers, vec![]).unwrap();
    let addr = spawn_registry(reg, "127.0.0.1:0").await.unwrap();
    format!("http://{addr}")
}

async fn org(
    id: &str,
    registry_url: &str,
    constitution: Option<Constitution>,
    providers: Option<ProviderSet>,
) -> Org {
    let mut cfg = OrgConfig::dev(id, &format!("Org {id}"));
    cfg.registry_url = Some(registry_url.to_owned());
    cfg.registry_token = Some(format!("{id}-registry"));
    if let Some(c) = constitution {
        cfg.constitution = c;
    }
    let app = build_org(&cfg, providers).unwrap();
    let addr = spawn_org(app, "127.0.0.1:0").await.unwrap();
    Org {
        url: format!("http://{addr}"),
        operator: cfg.tokens.operator.clone(),
        agent: cfg.tokens.agent.clone(),
        adapter: cfg.tokens.adapter.clone(),
        http: reqwest::Client::new(),
    }
}

fn mission_body() -> Value {
    json!({
        "goal": "Run the campus event for 120 attendees",
        "constraints": ["budget $5,000"],
        "success_checks": ["venue for 120 reserved"],
        "roles": [
            {"key": "coordinator", "name": "Mission coordinator", "charter": "Owns the plan",
             "authority": {"scopes": ["mission.coordinate"], "budget_cents": 100000}},
            {"key": "program", "name": "Program lead", "charter": "Owns the schedule",
             "authority": {"scopes": ["program.schedule"], "budget_cents": 50000}, "reports_to": "coordinator"}
        ],
        "responsibilities": [
            {"purpose": "Coordinate the mission", "owner": "coordinator"},
            {"purpose": "Maintain the program schedule", "owner": "program"}
        ]
    })
}

fn role_by_name<'a>(state: &'a Value, name: &str) -> &'a Value {
    state["roles"]
        .as_object()
        .unwrap()
        .values()
        .find(|r| r["name"] == name)
        .unwrap_or_else(|| panic!("role {name}"))
}

struct Seeded {
    mission: String,
    observation: String,
    requirement: String,
    coordinator: String,
}

async fn seed(o: &Org) -> Seeded {
    let (s, m) = o.post("/missions", mission_body()).await;
    assert_eq!(s, StatusCode::CREATED, "{m}");
    let mission = m["mission_id"].as_str().unwrap().to_owned();
    let (s, obs) = o
        .post_as(
            &o.agent,
            &format!("/missions/{mission}/observations"),
            json!({"source": "venue-portal", "summary": "Main hall closed on the event date", "evidence_ref": "fixture://closure"}),
        )
        .await;
    assert_eq!(s, StatusCode::CREATED, "{obs}");
    let observation = obs["observation_id"].as_str().unwrap().to_owned();
    let (s, req) = o
        .post_as(
            &o.agent,
            &format!("/missions/{mission}/requirements"),
            json!({"summary": "Relocate the event", "evidence": [observation], "acceptance": "venue for 120 reserved"}),
        )
        .await;
    assert_eq!(s, StatusCode::CREATED, "{req}");
    let state = o.state().await;
    Seeded {
        mission,
        observation,
        requirement: req["requirement_id"].as_str().unwrap().to_owned(),
        coordinator: role_by_name(&state, "Mission coordinator")["role_id"]
            .as_str()
            .unwrap()
            .to_owned(),
    }
}

fn relocation_change(rev: u64, s: &Seeded, client_request_id: &str) -> Value {
    json!({
        "client_request_id": client_request_id,
        "proposal": {
            "expected_org_revision": rev,
            "trigger_evidence_ids": [s.observation],
            "rationale": "Venue closure creates a continuing relocation responsibility",
            "budget_limit_cents": 200000,
            "operations": [
                {"op": "CreateRole", "key": "relocation", "name": "Relocation lead",
                 "charter": "Secure a replacement venue for 120 attendees",
                 "authority": {"scopes": ["ledger.reserve_venue", "capability.venue_lookup"], "budget_cents": 150000},
                 "reports_to": {"kind": "existing", "role_id": s.coordinator},
                 "provider": "elastic", "justification": "independent_ownership"},
                {"op": "AssignResponsibility",
                 "target": {"kind": "from_requirement", "key": "relocate", "requirement_id": s.requirement,
                            "purpose": "Relocate the event", "trigger": "venue availability changes",
                            "termination": "event held"},
                 "owner": {"kind": "new", "key": "relocation"}},
                {"op": "RecruitSpecialist", "key": "logistics", "specialist_address": "agent1qlogistics",
                 "protocol": "genesis-contract/1", "scope": "shuttle plan", "supervising_role": {"kind": "new", "key": "relocation"}}
            ]
        }
    })
}

/// Seed Org A and commit the relocation change. Returns (seeded, change record).
async fn relocate(o: &Org) -> (Seeded, Value) {
    let s = seed(o).await;
    let rev = o.state().await["revision"].as_u64().unwrap();
    let (status, rec) = o
        .post_as(
            &o.agent,
            "/org-changes",
            relocation_change(rev, &s, "chg-1"),
        )
        .await;
    assert_eq!(status, StatusCode::ACCEPTED, "{rec}");
    assert_eq!(rec["status"], "VALIDATED");
    let done = o
        .wait_change(
            rec["change_id"].as_str().unwrap(),
            &["COMMITTED", "FAILED", "REJECTED"],
        )
        .await;
    assert_eq!(done["status"], "COMMITTED", "{done}");
    (s, done)
}

fn manifest(publisher: &str, version: &str, verified: bool) -> CapabilityManifest {
    let case = |n: &str, k: CaseKind| CaseResult {
        name: n.into(),
        kind: k,
        passed: verified,
        detail: String::new(),
    };
    CapabilityManifest {
        capability_id: CapabilityId::new("cap_venue_availability"),
        version: version.into(),
        publisher: OrgId::new(publisher),
        source_digest: genesis_core::capability::sha256_hex(format!("src-{version}").as_bytes()),
        intent: "Read venue availability, capacity and price from a venue portal".into(),
        applicability_conditions: vec!["portal lists venues with capacity and dates".into()],
        known_limitations: vec!["fixture portal layout v1".into()],
        input_schema: json!({"type": "object", "properties": {"date": {"type": "string"}, "min_capacity": {"type": "integer"}}}),
        output_schema: json!({"type": "array"}),
        required_permissions: ["capability.venue_lookup".to_owned()].into(),
        artifact: ActorArtifact {
            actor_id: "apify-actor-1".into(),
            build_id: "build-1".into(),
            build_number: "0.0.1".into(),
        },
        role_template: None,
        responsibility_template: Some("Monitor venue availability".into()),
        workflow_procedure: None,
        local_configuration_schema: Value::Null,
        acceptance_suite_ref: "suite://venue/v1".into(),
        verification: VerificationReport {
            passed: verified,
            cases: vec![
                case("variant-a", CaseKind::Success),
                case("variant-b", CaseKind::Success),
                case("missing-capacity", CaseKind::Unsupported),
            ],
            tested_environment: "fixture portal v1".into(),
            limitations: vec![],
            oracle_ref: "oracle://venue/v1".into(),
            verified_at: chrono::Utc::now(),
        },
        provenance: vec!["org_a change".into()],
        state: if verified {
            CapabilityState::Approved
        } else {
            CapabilityState::Testing
        },
        published_at: None,
    }
}

#[tokio::test]
async fn principals_are_enforced() {
    let reg = registry().await;
    let a = org("org_a", &reg, None, None).await;
    let (s, _) = a.post_as(&a.agent, "/missions", mission_body()).await;
    assert_eq!(s, StatusCode::FORBIDDEN);
    let (s, _) = a
        .req(reqwest::Method::GET, "/organization", "wrong-token", None)
        .await;
    assert_eq!(s, StatusCode::UNAUTHORIZED);
    let (s, h) = a.req(reqwest::Method::GET, "/health", "", None).await;
    assert_eq!(s, StatusCode::OK);
    assert_eq!(h["dev_stub_enabled"], true);
}

#[tokio::test]
async fn change_saga_commits_new_role_with_labeled_dev_stub_binding() {
    let reg = registry().await;
    let a = org("org_a", &reg, None, None).await;
    let (s, done) = relocate(&a).await;
    let state = a.state().await;
    assert_eq!(done["committed_revision"], state["revision"]);
    let role = role_by_name(&state, "Relocation lead");
    assert_eq!(role["state"], "ACTIVE");
    assert_eq!(role["binding"]["provider"], "dev_stub");
    assert!(role["binding"]["provider_agent_id"]
        .as_str()
        .unwrap()
        .starts_with("devstub-elastic-"));
    assert_eq!(state["requirements"][&s.requirement]["status"], "ASSIGNED");

    let types: Vec<String> = a
        .events()
        .await
        .iter()
        .map(|e| e["type"].as_str().unwrap().to_owned())
        .collect();
    for t in [
        "change_proposed",
        "role_provisioned",
        "role_created",
        "role_activated",
        "responsibility_assigned",
        "contract_requested",
    ] {
        assert!(types.contains(&t.to_owned()), "missing {t} in {types:?}");
    }
    let provisioned = a
        .events()
        .await
        .into_iter()
        .find(|e| e["type"] == "role_provisioned")
        .unwrap();
    assert_eq!(provisioned["payload"]["dev_stub"], true);

    // Same client request ID returns the stored record instead of a second change.
    let (status, again) = a
        .post_as(&a.agent, "/org-changes", relocation_change(0, &s, "chg-1"))
        .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(again["change_id"], done["change_id"]);
}

#[tokio::test]
async fn invalid_and_conflicting_proposals_are_rejected_and_recorded() {
    let reg = registry().await;
    let a = org("org_a", &reg, None, None).await;
    let s = seed(&a).await;
    let rev = a.state().await["revision"].as_u64().unwrap();

    let mut bad = relocation_change(rev, &s, "chg-bad");
    bad["proposal"]["operations"][0]["authority"]["scopes"] = json!(["treasury.wire_funds"]);
    let (status, err) = a.post_as(&a.agent, "/org-changes", bad).await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "{err}");
    assert_eq!(err["error"], "missing_authority");
    let id = err["details"]["change_id"].as_str().unwrap();
    let (_, rec) = a.get(&format!("/org-changes/{id}")).await;
    assert_eq!(rec["status"], "REJECTED");

    let (status, err) = a
        .post_as(
            &a.agent,
            "/org-changes",
            relocation_change(rev - 1, &s, "chg-stale"),
        )
        .await;
    assert_eq!(status, StatusCode::CONFLICT);
    assert_eq!(err["error"], "revision_conflict");
}

#[tokio::test]
async fn failed_provisioning_leaves_organization_unchanged() {
    let reg = registry().await;
    let providers = ProviderSet::new().with_provisioner(
        Provider::Elastic,
        Arc::new(FailingProvisioner {
            provider: Provider::Elastic,
            error: ProviderError::Http {
                status: 403,
                body: "missing feature_agentBuilder privilege".into(),
            },
        }),
    );
    let a = org("org_a", &reg, None, Some(providers)).await;
    let s = seed(&a).await;
    let before = a.state().await;
    let rev = before["revision"].as_u64().unwrap();
    let (_, rec) = a
        .post_as(
            &a.agent,
            "/org-changes",
            relocation_change(rev, &s, "chg-f"),
        )
        .await;
    let done = a
        .wait_change(
            rec["change_id"].as_str().unwrap(),
            &["COMMITTED", "FAILED", "REJECTED"],
        )
        .await;
    assert_eq!(done["status"], "FAILED");
    let after = a.state().await;
    assert_eq!(after["revision"], before["revision"]);
    assert_eq!(after["roles"].as_object().unwrap().len(), 2);
    assert_eq!(after["requirements"][&s.requirement]["status"], "UNMET");
    let failed = a
        .events()
        .await
        .into_iter()
        .find(|e| e["type"] == "provisioning_failed")
        .unwrap();
    assert!(failed["payload"]["error"].as_str().unwrap().contains("403"));
}

#[tokio::test]
async fn dispatch_results_are_idempotent_and_stale_epochs_are_fenced() {
    let reg = registry().await;
    let a = org("org_a", &reg, None, None).await;
    let (s, done) = relocate(&a).await;
    let state = a.state().await;
    let relocation = role_by_name(&state, "Relocation lead")["role_id"]
        .as_str()
        .unwrap()
        .to_owned();
    let rsp = done["allocation"]["responsibilities"]["relocate"]
        .as_str()
        .unwrap()
        .to_owned();

    // The program lead cannot dispatch relocation work.
    let program = role_by_name(&state, "Program lead")["role_id"]
        .as_str()
        .unwrap()
        .to_owned();
    let (st, _) = a
        .post("/jobs", json!({"client_request_id": "j0", "role_id": program, "responsibility_id": rsp, "task": "x"}))
        .await;
    assert_eq!(st, StatusCode::FORBIDDEN);

    let (st, job) = a
        .post(
            "/jobs",
            json!({"client_request_id": "j1", "role_id": relocation, "responsibility_id": rsp,
                   "task": "reserve replacement venue", "required_scope": "ledger.reserve_venue"}),
        )
        .await;
    assert_eq!(st, StatusCode::CREATED, "{job}");
    let job_id = job["job_id"].as_str().unwrap().to_owned();
    // Outbox dev stub moves the job to RUNNING.
    for _ in 0..50 {
        if a.get(&format!("/jobs/{job_id}")).await.1["status"] == "RUNNING" {
            break;
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    assert_eq!(
        a.get(&format!("/jobs/{job_id}")).await.1["status"],
        "RUNNING"
    );

    // Provider success without domain verification is VERIFYING, not success.
    let result = |verified: bool| {
        json!({"attempt_id": job["attempt_id"], "epoch": job["epoch"], "outcome": "succeeded",
               "provider_status": "SUCCEEDED", "output": {"venue": "Hall B", "capacity": 150},
               "operation_id": "op-reserve-hall-b", "domain_verified": verified})
    };
    let (st, r) = a
        .post_as(
            &a.adapter,
            &format!("/jobs/{job_id}/results"),
            result(false),
        )
        .await;
    assert_eq!(st, StatusCode::ACCEPTED, "{r}");
    assert_eq!(r["job"]["status"], "VERIFYING");
    let (st, r) = a
        .post_as(&a.adapter, &format!("/jobs/{job_id}/results"), result(true))
        .await;
    assert_eq!(st, StatusCode::OK, "{r}");
    assert_eq!(r["job"]["status"], "SUCCEEDED");
    // Duplicate delivery: no second ledger effect.
    let (st, r) = a
        .post_as(&a.adapter, &format!("/jobs/{job_id}/results"), result(true))
        .await;
    assert_eq!(st, StatusCode::OK);
    assert_eq!(r["duplicate"], true);
    assert_eq!(a.get("/ledger").await.1.as_array().unwrap().len(), 1);

    // Dispatch a second job, then hand the responsibility off and retire the role.
    let (_, job2) = a
        .post("/jobs", json!({"client_request_id": "j2", "role_id": relocation, "responsibility_id": rsp, "task": "confirm shuttles"}))
        .await;
    let rev = a.state().await["revision"].as_u64().unwrap();
    let (st, rec) = a
        .post_as(
            &a.agent,
            "/org-changes",
            json!({"client_request_id": "chg-retire", "proposal": {
                "expected_org_revision": rev, "trigger_evidence_ids": [s.observation],
                "rationale": "Relocation complete; monitoring continues in a persistent role",
                "operations": [
                    {"op": "CreateRole", "key": "monitor", "name": "Venue monitor", "charter": "Watch the booked venue",
                     "authority": {"scopes": ["monitor.venue"], "budget_cents": 5000}, "provider": "creao", "justification": "lifecycle"},
                    {"op": "RetireRole", "role_id": relocation, "handoffs": [{"responsibility_id": rsp, "to": {"kind": "new", "key": "monitor"}}]}
                ]}}),
        )
        .await;
    assert_eq!(st, StatusCode::ACCEPTED, "{rec}");
    let d = a
        .wait_change(
            rec["change_id"].as_str().unwrap(),
            &["COMMITTED", "FAILED", "REJECTED"],
        )
        .await;
    assert_eq!(d["status"], "COMMITTED", "{d}");
    let state = a.state().await;
    assert_eq!(state["roles"][&relocation]["state"], "RETIRED");

    // Late result from the old epoch is rejected and archived.
    let (st, r) = a
        .post_as(
            &a.adapter,
            &format!("/jobs/{}/results", job2["job_id"].as_str().unwrap()),
            json!({"attempt_id": job2["attempt_id"], "epoch": job2["epoch"], "outcome": "succeeded",
                   "provider_status": "SUCCEEDED", "operation_id": "op-late", "domain_verified": true}),
        )
        .await;
    assert_eq!(st, StatusCode::CONFLICT, "{r}");
    assert_eq!(r["error"], "stale_epoch");
    assert_eq!(
        a.get(&format!("/jobs/{}", job2["job_id"].as_str().unwrap()))
            .await
            .1["status"],
        "RESULT_REJECTED"
    );
    assert_eq!(a.get("/ledger").await.1.as_array().unwrap().len(), 1);
    // The retired role can no longer dispatch.
    let (st, _) = a
        .post("/jobs", json!({"client_request_id": "j3", "role_id": relocation, "responsibility_id": rsp, "task": "x"}))
        .await;
    assert_eq!(st, StatusCode::FORBIDDEN);
}

#[tokio::test]
async fn unverified_capability_cannot_be_published() {
    let reg = registry().await;
    let a = org("org_a", &reg, None, None).await;
    let (st, err) = a
        .post(
            "/capabilities/publish",
            serde_json::to_value(manifest("org_a", "1.0.0", false)).unwrap(),
        )
        .await;
    assert_eq!(st, StatusCode::UNPROCESSABLE_ENTITY, "{err}");
    assert_eq!(err["error"], "verification_failed");
    // A cannot publish under B's name.
    let (st, _) = a
        .post(
            "/capabilities/publish",
            serde_json::to_value(manifest("org_b", "1.0.0", true)).unwrap(),
        )
        .await;
    assert_eq!(st, StatusCode::FORBIDDEN);
}

/// Install, accept, attach and run a capability inside one organization.
async fn adopt(o: &Org, role_id: &str, rsp: &str, observation: &str, input: Value) -> Value {
    let (st, inst) = o
        .post(
            "/capability-installations",
            json!({"capability_id": "cap_venue_availability", "version": "1.0.0"}),
        )
        .await;
    assert_eq!(st, StatusCode::CREATED, "{inst}");
    assert_eq!(inst["synthesis_calls"], 0);
    let inst_id = inst["installation_id"].as_str().unwrap().to_owned();

    // Running before local acceptance is refused.
    let (st, _) = o
        .post("/capability-runs", json!({"client_request_id": "early", "installation_id": inst_id, "role_id": role_id, "responsibility_id": rsp, "input": input}))
        .await;
    assert_eq!(st, StatusCode::UNPROCESSABLE_ENTITY);

    let (st, _) = o
        .post(
            &format!("/capability-installations/{inst_id}/acceptance"),
            json!({"passed": true, "report_ref": "local://acceptance/1"}),
        )
        .await;
    assert_eq!(st, StatusCode::OK);
    let rev = o.state().await["revision"].as_u64().unwrap();
    let (st, rec) = o
        .post(
            "/org-changes",
            json!({"client_request_id": format!("attach-{role_id}"), "proposal": {
                "expected_org_revision": rev, "trigger_evidence_ids": [observation],
                "rationale": "attach the verified venue capability",
                "operations": [{"op": "AttachCapability", "role": {"kind": "existing", "role_id": role_id},
                                "capability": {"capability_id": "cap_venue_availability", "version": "1.0.0"}}]}}),
        )
        .await;
    assert_eq!(st, StatusCode::ACCEPTED, "{rec}");
    let d = o
        .wait_change(
            rec["change_id"].as_str().unwrap(),
            &["COMMITTED", "FAILED", "REJECTED"],
        )
        .await;
    assert_eq!(d["status"], "COMMITTED", "{d}");
    let (st, job) = o
        .post("/capability-runs", json!({"client_request_id": format!("run-{role_id}"), "installation_id": inst_id, "role_id": role_id, "responsibility_id": rsp, "input": input}))
        .await;
    assert_eq!(st, StatusCode::ACCEPTED, "{job}");
    assert_eq!(job["status"], "RUNNING");
    assert_eq!(job["provider_refs"]["build_number"], "0.0.1");
    job
}

#[tokio::test]
async fn second_organization_inherits_capability_without_synthesis() {
    let reg = registry().await;
    let a = org("org_a", &reg, None, None).await;
    let b = org("org_b", &reg, None, None).await;

    // Org A relocates, publishes and uses the verified capability.
    let (sa, done) = relocate(&a).await;
    let state_a = a.state().await;
    let relocation = role_by_name(&state_a, "Relocation lead")["role_id"]
        .as_str()
        .unwrap()
        .to_owned();
    let rsp_a = done["allocation"]["responsibilities"]["relocate"]
        .as_str()
        .unwrap()
        .to_owned();
    let (st, published) = a
        .post(
            "/capabilities/publish",
            serde_json::to_value(manifest("org_a", "1.0.0", true)).unwrap(),
        )
        .await;
    assert_eq!(st, StatusCode::CREATED, "{published}");
    assert_eq!(published["state"], "PUBLISHED");
    adopt(
        &a,
        &relocation,
        &rsp_a,
        &sa.observation,
        json!({"date": "2026-11-14", "min_capacity": 120}),
    )
    .await;

    // Org B: its own mission, its own role; discovers by its own need.
    let (_, m) = b
        .post(
            "/missions",
            json!({"goal": "Host a 300-person conference", "roles": [
                {"key": "venue", "name": "Venue ops", "charter": "Owns venue logistics",
                 "authority": {"scopes": ["capability.venue_lookup"], "budget_cents": 10000}}],
                "responsibilities": [{"purpose": "Keep the venue booked", "owner": "venue"}]}),
        )
        .await;
    let mission_b = m["mission_id"].as_str().unwrap().to_owned();
    let (_, obs) = b
        .post(
            &format!("/missions/{mission_b}/observations"),
            json!({"source": "operator", "summary": "Need to check venue availability"}),
        )
        .await;
    let (st, hits) = b
        .post(
            "/capabilities/search",
            json!({"query": "venue availability capacity"}),
        )
        .await;
    assert_eq!(st, StatusCode::OK);
    assert_eq!(hits[0]["capability_id"], "cap_venue_availability");
    assert_eq!(hits[0]["publisher"], "org_a");

    let state_b = b.state().await;
    let venue_role = role_by_name(&state_b, "Venue ops")["role_id"]
        .as_str()
        .unwrap()
        .to_owned();
    let rsp_b = state_b["responsibilities"]
        .as_object()
        .unwrap()
        .keys()
        .next()
        .unwrap()
        .clone();
    let job = adopt(
        &b,
        &venue_role,
        &rsp_b,
        obs["observation_id"].as_str().unwrap(),
        json!({"date": "2027-03-02", "min_capacity": 300}),
    )
    .await;
    assert_eq!(job["input"]["min_capacity"], 300);

    // Isolation canary: B holds none of A's private state.
    let state_b = b.state().await;
    let b_text = state_b.to_string();
    assert!(!b_text.contains(&sa.mission));
    assert!(!b_text.contains(&relocation));
    assert!(!b_text.contains("agent1qlogistics"));
    let installed = b
        .events()
        .await
        .into_iter()
        .find(|e| e["type"] == "capability_installed")
        .unwrap();
    assert_eq!(installed["payload"]["synthesis_calls"], 0);
    assert_eq!(
        installed["payload"]["source_digest"],
        published["source_digest"]
    );
}

#[tokio::test]
async fn incompatible_organization_rejects_installation() {
    let reg = registry().await;
    let a = org("org_a", &reg, None, None).await;
    let (st, _) = a
        .post(
            "/capabilities/publish",
            serde_json::to_value(manifest("org_a", "1.0.0", true)).unwrap(),
        )
        .await;
    assert_eq!(st, StatusCode::CREATED);
    let mut narrow = genesis_server::demo_constitution();
    narrow.scopes.remove("capability.venue_lookup");
    let c = org("org_c", &reg, Some(narrow), None).await;
    let (st, err) = c
        .post(
            "/capability-installations",
            json!({"capability_id": "cap_venue_availability", "version": "1.0.0"}),
        )
        .await;
    assert_eq!(st, StatusCode::UNPROCESSABLE_ENTITY, "{err}");
    assert_eq!(err["error"], "incompatible_capability");
}

#[tokio::test]
async fn sse_replays_from_cursor() {
    let reg = registry().await;
    let a = org("org_a", &reg, None, None).await;
    seed(&a).await;
    let resp = a
        .http
        .get(format!(
            "{}/api/v1/events?after=1&token={}",
            a.url, a.operator
        ))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let mut resp = resp;
    let mut buf = String::new();
    let deadline = tokio::time::Instant::now() + Duration::from_secs(3);
    while !buf.contains("requirement_recorded") && tokio::time::Instant::now() < deadline {
        match tokio::time::timeout(Duration::from_millis(500), resp.chunk()).await {
            Ok(Ok(Some(c))) => buf.push_str(&String::from_utf8_lossy(&c)),
            _ => break,
        }
    }
    assert!(buf.contains("event: domain_event"));
    assert!(buf.contains("mission_created"));
    assert!(buf.contains("requirement_recorded"));
    assert!(
        !buf.contains("organization_initialized"),
        "cursor should skip seq 1"
    );
}

#[tokio::test]
async fn state_survives_restart_from_sqlite() {
    let dir = tempfile::tempdir().unwrap();
    let reg = registry().await;
    let mut cfg = OrgConfig::dev("org_a", "Org A");
    cfg.db = Some(dir.path().join("a.db"));
    cfg.registry_url = Some(reg.clone());
    cfg.registry_token = Some("org_a-registry".into());
    let app = build_org(&cfg, None).unwrap();
    let addr = spawn_org(app, "127.0.0.1:0").await.unwrap();
    let a = Org {
        url: format!("http://{addr}"),
        operator: cfg.tokens.operator.clone(),
        agent: cfg.tokens.agent.clone(),
        adapter: cfg.tokens.adapter.clone(),
        http: reqwest::Client::new(),
    };
    relocate(&a).await;
    let before = a.state().await;
    let reopened = build_org(&cfg, None).unwrap();
    let after = serde_json::to_value(reopened.store.current_state().unwrap()).unwrap();
    assert_eq!(before, after);
    // A database for another org is refused.
    let mut wrong = cfg.clone();
    wrong.org_id = OrgId::new("org_b");
    assert!(build_org(&wrong, None).is_err());
}
