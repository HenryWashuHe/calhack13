//! Real registry/intake/kernel with scripted planning and local stub provisioning.
//! Source is inert data: no model, generated method, or hosted provider is run.
use genesis_core::{
    development_method::ExactContentRef,
    development_package::{MethodPackage, MethodSourceFile},
    ids::{ObservationId, OrgId},
};
use genesis_planner::{
    agent::{PlanOutcome, Planner},
    kernel::KernelClient,
    llm::ReplayClient,
    runner::Runner,
};
use genesis_server::{
    build_org,
    registry::{
        methods::{MethodPublication, MethodSearch},
        RegistryClient, RegistryState,
    },
    spawn_org, spawn_registry, OrgConfig,
};
use serde_json::{json, Value};
use std::{sync::Arc, time::Duration};

const NEED: &str = "Develop a search method for a new rough-terrain requirement";
const ACCEPTANCE: &str = "A bounded local evaluation supports adoption or a recorded decline";
const SOURCE: &str = "# inert method fixture: never execute this source\n";

struct Receiver {
    url: String,
    cfg: OrgConfig,
    mission: String,
    lead: String,
    request: Value,
    http: reqwest::Client,
}
impl Receiver {
    async fn new() -> Self {
        let registry = RegistryState::open_in_memory(
            [("publisher-token".into(), OrgId::new("org_a"))].into(),
            vec!["receiver-token".into()],
        )
        .unwrap();
        let addr = spawn_registry(registry, "127.0.0.1:0").await.unwrap();
        let registry_url = format!("http://{addr}");
        let exact = |id: &str| ExactContentRef {
            domain: "fixture".into(),
            schema: "public/1".into(),
            id: id.into(),
            version: "1".into(),
            sha256: "a".repeat(64),
        };
        let package = MethodPackage::new(
            "rough-search".into(),
            "1".into(),
            exact("runtime"),
            exact("protocol"),
            "method.py".into(),
            vec![],
            vec![MethodSourceFile {
                path: "method.py".into(),
                content: SOURCE.into(),
            }],
        )
        .unwrap();
        RegistryClient::new(&registry_url, "publisher-token")
            .publish_method(
                &OrgId::new("org_a"),
                &MethodPublication {
                    reference: package.reference().unwrap(),
                    package_json: package.artifact_json().unwrap(),
                    intent: "rough terrain search".into(),
                    applicability: vec!["public simulation tasks; source only".into()],
                },
            )
            .await
            .unwrap();
        let found = RegistryClient::new(&registry_url, "receiver-token")
            .search_methods(&MethodSearch {
                query: "rough terrain".into(),
                limit: 1,
                runtime: None,
                protocol: None,
                exclude_publisher: Some(OrgId::new("org_b")),
            })
            .await
            .unwrap();
        assert_eq!(found.candidates.len(), 1);
        let mut cfg = OrgConfig::dev("org_b", "Receiving lab");
        cfg.registry_url = Some(registry_url);
        cfg.registry_token = Some("receiver-token".into());
        cfg.constitution.scopes = ["mission.coordinate", "method.develop"]
            .into_iter()
            .map(String::from)
            .collect();
        let app = build_org(&cfg, None).unwrap();
        let addr = spawn_org(app, "127.0.0.1:0").await.unwrap();
        let mut receiver = Self {
            url: format!("http://{addr}"),
            cfg,
            mission: String::new(),
            lead: String::new(),
            request: json!({"client_request_id":"selected-method-1",
                "need":NEED,"acceptance":ACCEPTANCE,"selection":found.candidates[0]}),
            http: reqwest::Client::new(),
        };
        let (status, mission) = receiver.post("/missions", &json!({
            "goal":"Improve capability development for physical-design tasks",
            "roles":[{"key":"lead","name":"Lab lead","charter":"Own the lab and evaluate reusable methods",
                "authority":{"scopes":["mission.coordinate","method.develop"],"budget_cents":10000}}],
            "responsibilities":[{"purpose":"Coordinate the lab","owner":"lead"}]
        })).await;
        assert_eq!(status, 201, "{mission}");
        receiver.mission = mission["mission_id"].as_str().unwrap().into();
        receiver.lead = receiver.get("/organization").await["roles"]
            .as_object()
            .unwrap()
            .values()
            .find(|r| r["name"] == "Lab lead")
            .unwrap()["role_id"]
            .as_str()
            .unwrap()
            .into();
        receiver
    }
    async fn post(&self, path: &str, body: &Value) -> (u16, Value) {
        let response = self
            .http
            .post(format!("{}/api/v1{path}", self.url))
            .bearer_auth(&self.cfg.tokens.operator)
            .json(body)
            .send()
            .await
            .unwrap();
        (response.status().as_u16(), response.json().await.unwrap())
    }
    async fn get(&self, path: &str) -> Value {
        self.http
            .get(format!("{}/api/v1{path}", self.url))
            .bearer_auth(&self.cfg.tokens.operator)
            .send()
            .await
            .unwrap()
            .error_for_status()
            .unwrap()
            .json()
            .await
            .unwrap()
    }
    fn runner(&self, model: Arc<ReplayClient>) -> Runner {
        Runner::new(
            Planner::new(KernelClient::new(&self.url, &self.cfg.tokens.agent), model),
            Duration::from_millis(1),
        )
    }
    async fn intake(&self) -> Value {
        let path = format!("/missions/{}/method-development", self.mission);
        let (status, receipt) = self.post(&path, &self.request).await;
        assert_eq!(status, 201, "{receipt}");
        assert_eq!(receipt["org_id"], "org_b");
        assert_eq!(receipt["mission_id"], self.mission);
        assert_eq!(receipt["request"], self.request);
        assert_eq!(receipt["request_sha256"].as_str().unwrap().len(), 64);
        assert_eq!(
            self.post(&path, &self.request).await,
            (200, receipt.clone())
        );
        receipt
    }
    async fn wait_committed(&self, id: &str) -> Value {
        for _ in 0..100 {
            let change = self.get(&format!("/org-changes/{id}")).await;
            if ["COMMITTED", "FAILED", "REJECTED"]
                .contains(&change["status"].as_str().unwrap_or(""))
            {
                assert_eq!(change["status"], "COMMITTED", "{change}");
                return change;
            }
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
        panic!("scripted kernel change did not finish");
    }
    async fn assert_replay(&self, receipt: &Value, model: Arc<ReplayClient>, runner: &mut Runner) {
        let path = format!("/missions/{}/method-development", self.mission);
        assert_eq!(
            self.post(&path, &self.request).await,
            (200, receipt.clone())
        );
        assert!(runner.tick().await.unwrap().is_empty());
        assert!(self.runner(model.clone()).tick().await.unwrap().is_empty());
        assert_eq!(model.requests().len(), 1);
        let events = self.get("/events/log?after=0&limit=5000").await;
        assert_eq!(
            events
                .as_array()
                .unwrap()
                .iter()
                .filter(|e| e["type"] == "planner_decision")
                .count(),
            1
        );
        let state = self.get("/organization").await;
        assert_eq!(state["observations"].as_object().unwrap().len(), 1);
        let requests = model.requests();
        let prompt = &requests[0].messages[0].content;
        for expected in [
            NEED,
            ACCEPTANCE,
            receipt["request_sha256"].as_str().unwrap(),
        ] {
            assert!(
                prompt.contains(expected),
                "missing selected-method context: {expected}"
            );
        }
        assert!(!prompt.contains(SOURCE));
    }
}
fn change_reply(lead: &str, specialist: bool) -> String {
    let owner = if specialist {
        json!({"kind":"new","key":"method-owner"})
    } else {
        json!({"kind":"existing","role_id":lead})
    };
    let mut operations = vec![];
    if specialist {
        operations.push(
            json!({"op":"CreateRole","key":"method-owner","name":"Method specialist",
            "charter":"Maintain and evaluate the search method for this requirement",
            "authority":{"scopes":["method.develop"],"budget_cents":1000},
            "reports_to":{"kind":"existing","role_id":lead},"provider":"elastic",
            "justification":"independent_ownership"}),
        );
    }
    operations.push(json!({"op":"AssignResponsibility",
        "target":{"kind":"from_requirement","key":"method-work","requirement_id":"$REQUIREMENT",
            "purpose":NEED,"trigger":"new method evidence","termination":ACCEPTANCE},"owner":owner}));
    json!({"assessment":if specialist {"The mission needs continuing independent method ownership."}
        else {"The current lab lead already has the authority and capacity for this work."},
        "verdict":"change","requirement":{"summary":NEED,"acceptance":ACCEPTANCE},
        "rationale":"Assign accountable development work; source selection grants no execution rights.",
        "budget_limit_cents":10000,"operations":operations}).to_string()
}
async fn ownership_case(specialist: bool) {
    let receiver = Receiver::new().await;
    let receipt = receiver.intake().await;
    let obs = ObservationId::new(receipt["observation_id"].as_str().unwrap());
    let model = Arc::new(ReplayClient::new([change_reply(
        &receiver.lead,
        specialist,
    )]));
    let mut runner = receiver.runner(model.clone());
    let outcomes = runner.tick().await.unwrap();
    assert_eq!(outcomes.len(), 1);
    assert_eq!(outcomes[0].0, obs);
    let PlanOutcome::Proposed {
        change,
        attempts: 1,
    } = &outcomes[0].1
    else {
        panic!("expected scripted proposal: {outcomes:?}");
    };
    let committed = receiver.wait_committed(change.change_id.as_str()).await;
    assert_eq!(
        committed["proposal"]["trigger_evidence_ids"],
        json!([obs.as_str()])
    );
    let ops = committed["proposal"]["operations"].as_array().unwrap();
    assert_eq!(
        ops.iter().filter(|o| o["op"] == "CreateRole").count(),
        usize::from(specialist)
    );
    receiver.assert_replay(&receipt, model, &mut runner).await;
    let state = receiver.get("/organization").await;
    assert_eq!(
        state["roles"].as_object().unwrap().len(),
        1 + usize::from(specialist)
    );
    let owner = if specialist {
        state["roles"]
            .as_object()
            .unwrap()
            .values()
            .find(|r| r["name"] == "Method specialist")
            .unwrap()["role_id"]
            .clone()
    } else {
        json!(receiver.lead)
    };
    let requirements = state["requirements"].as_object().unwrap();
    assert_eq!(requirements.len(), 1);
    let requirement = requirements.values().next().unwrap();
    assert_eq!(requirement["triggering_evidence"], json!([obs.as_str()]));
    assert_eq!(requirement["status"], "ASSIGNED");
    assert_eq!(requirement["owner"], json!({"kind":"role","id":owner}));
    let work: Vec<_> = state["responsibilities"]
        .as_object()
        .unwrap()
        .values()
        .filter(|r| r["requirement_id"] == requirement["requirement_id"])
        .collect();
    assert_eq!(work.len(), 1);
    assert_eq!(work[0]["owner"], requirement["owner"]);
}

#[tokio::test]
async fn selected_source_need_can_commit_a_specialist_and_owned_requirement() {
    ownership_case(true).await;
}

#[tokio::test]
async fn existing_owner_control_assigns_work_without_creating_a_role() {
    ownership_case(false).await;
}

#[tokio::test]
async fn appropriate_nonreuse_records_no_change_and_replay_stays_settled() {
    let receiver = Receiver::new().await;
    let receipt = receiver.intake().await;
    let before = receiver.get("/organization").await;
    let model = Arc::new(ReplayClient::new([json!({"assessment":"This method does not warrant new work for the mission.",
        "verdict":"no_change","requirement":null,"rationale":"Record non-reuse; no change is justified.",
        "operations":[]}).to_string()]));
    let mut runner = receiver.runner(model.clone());
    let outcomes = runner.tick().await.unwrap();
    assert_eq!(outcomes.len(), 1);
    assert_eq!(
        outcomes[0].0.as_str(),
        receipt["observation_id"].as_str().unwrap()
    );
    assert!(matches!(
        outcomes[0].1,
        PlanOutcome::NoChange { attempts: 1 }
    ));
    receiver.assert_replay(&receipt, model, &mut runner).await;
    let after = receiver.get("/organization").await;
    for field in ["revision", "roles", "responsibilities", "requirements"] {
        assert_eq!(before[field], after[field], "NoChange altered {field}");
    }
    let events = receiver.get("/events/log?after=0&limit=5000").await;
    assert!(!events
        .as_array()
        .unwrap()
        .iter()
        .any(|e| e["type"] == "change_proposed"));
}
