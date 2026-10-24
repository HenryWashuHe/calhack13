//! Focused review regressions for PR #11 exact head 4d4949c; expected to fail.
use axum::{
    extract::{Path, State},
    routing::{get, post},
    Json, Router,
};
use genesis_core::{
    engine::RoleToProvision,
    ids::{ChangeId, OrgId, RoleId},
    model::{AuthorityGrant, Provider},
};
use genesis_elastic::{fake, AgentBuilder, AgentBuilderConfig, DocKind, EvidenceDoc};
use genesis_providers::{ProvisionRequest, RoleProvisioner};
use serde_json::{json, Value};
use std::{
    collections::BTreeMap,
    sync::{Arc, Mutex},
    time::Duration,
};

fn builder(url: &str, timeout: Duration) -> AgentBuilder {
    AgentBuilder::new(AgentBuilderConfig {
        kibana_url: url.into(),
        api_key: "review-only-key".into(),
        space: None,
        index_prefix: "genesis".into(),
        timeout,
    })
    .unwrap()
}
fn request(org: &str) -> ProvisionRequest {
    ProvisionRequest {
        org_id: OrgId::new(org),
        change_id: ChangeId::new("change-review"),
        role: RoleToProvision {
            key: "specialist".into(),
            role_id: RoleId::new("role-review"),
            name: "Review specialist".into(),
            charter: "Read only this organization's evidence".into(),
            authority: AuthorityGrant::default(),
            provider: Provider::Elastic,
        },
    }
}
fn doc(org: &str) -> EvidenceDoc {
    EvidenceDoc {
        doc_id: format!("{org}:secret"),
        org_id: org.into(),
        kind: DocKind::Observation,
        title: "private venue evidence".into(),
        body: format!("Only {org} should see this"),
        mission_id: None,
        status: None,
        revision: 1,
        refs: BTreeMap::new(),
    }
}

#[tokio::test]
async fn native_tool_must_not_mix_distinct_orgs_whose_slugs_collide() {
    let (es, state) = fake::start().await;
    let (url, kibana) = fake::start_kibana().await;
    let ab = builder(&url, Duration::from_secs(2));
    for org in ["Org_A", "org_a"] {
        es.ensure_indices(org).await.unwrap();
        es.index_evidence(org, &[doc(org)]).await.unwrap();
        ab.provision(&request(org)).await.unwrap();
    }
    let tool = kibana.lock().unwrap().tools[&ab.evidence_tool_id("org_a")].clone();
    let pattern = tool["configuration"]["pattern"].as_str().unwrap();
    let stored = state.lock().unwrap();
    let docs = &stored.docs[pattern];
    assert!(
        docs.values().all(|doc| doc["org_id"] == "org_a"),
        "native index_search sees both raw org IDs in one pattern: {:?}",
        docs.values()
            .map(|doc| doc["org_id"].clone())
            .collect::<Vec<_>>()
    );
}

#[tokio::test]
async fn long_organization_id_must_use_same_index_for_projection_and_tool() {
    let org = "a".repeat(81);
    let (es, _) = fake::start().await;
    let (url, kibana) = fake::start_kibana().await;
    let ab = builder(&url, Duration::from_secs(2));
    let tool = ab.ensure_evidence_tool(&org).await.unwrap();
    assert_eq!(
        kibana.lock().unwrap().tools[&tool]["configuration"]["pattern"],
        es.evidence_index(&org)
    );
}

#[tokio::test]
async fn reconciling_an_agent_must_reject_added_broad_tools() {
    let (url, kibana) = fake::start_kibana().await;
    let ab = builder(&url, Duration::from_secs(2));
    let req = request("org_a");
    let binding = ab.provision(&req).await.unwrap();
    kibana
        .lock()
        .unwrap()
        .agents
        .get_mut(&binding.provider_agent_id)
        .unwrap()["configuration"]["tools"][0]["tool_ids"]
        .as_array_mut()
        .unwrap()
        .push(json!("platform.core.search"));
    assert!(
        ab.provision(&req).await.is_err(),
        "extra broad tool silently adopted under the original config digest"
    );
}

#[tokio::test]
async fn readiness_must_reject_wrong_org_tool_and_modified_instructions() {
    let (url, kibana) = fake::start_kibana().await;
    let ab = builder(&url, Duration::from_secs(2));
    let binding = ab.provision(&request("org_a")).await.unwrap();
    {
        let mut state = kibana.lock().unwrap();
        let agent = state.agents.get_mut(&binding.provider_agent_id).unwrap();
        agent["configuration"]["instructions"] = json!("Changed after provisioning");
        agent["configuration"]["tools"] = json!([{"tool_ids":["genesis-org_b-evidence-search"]}]);
        state.tools.clear();
    }
    assert!(
        !ab.check_ready(&binding).await.unwrap(),
        "missing wrong-tenant tool is ready despite changed binding config"
    );
}

#[derive(Default)]
struct LostReply {
    objects: BTreeMap<String, Value>,
    agent_posts: usize,
    agent_gets: usize,
}
type Shared = Arc<Mutex<LostReply>>;
async fn read(
    State(state): State<Shared>,
    Path((collection, id)): Path<(String, String)>,
) -> (axum::http::StatusCode, Json<Value>) {
    let mut state = state.lock().unwrap();
    if collection == "agents" {
        state.agent_gets += 1;
    }
    match state.objects.get(&format!("{collection}/{id}")) {
        Some(value) => (axum::http::StatusCode::OK, Json(value.clone())),
        None => (
            axum::http::StatusCode::NOT_FOUND,
            Json(json!({"error":"Not Found"})),
        ),
    }
}
async fn create(
    State(state): State<Shared>,
    Path(collection): Path<String>,
    Json(body): Json<Value>,
) -> Json<Value> {
    {
        let mut state = state.lock().unwrap();
        state.objects.insert(
            format!("{collection}/{}", body["id"].as_str().unwrap()),
            body.clone(),
        );
        if collection == "agents" {
            state.agent_posts += 1;
        }
    }
    if collection == "agents" {
        tokio::time::sleep(Duration::from_secs(2)).await;
    }
    Json(body)
}
#[tokio::test]
async fn lost_agent_post_response_must_reconcile_by_get_without_another_post() {
    let state: Shared = Arc::default();
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://{}", listener.local_addr().unwrap());
    let app = Router::new()
        .route("/api/agent_builder/{collection}/{id}", get(read))
        .route("/api/agent_builder/{collection}", post(create))
        .with_state(state.clone());
    let server = tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    let result = builder(&url, Duration::from_millis(100))
        .provision(&request("org_a"))
        .await;
    let state = state.lock().unwrap();
    assert_eq!(state.agent_posts, 1);
    let gets = state.agent_gets;
    server.abort();
    assert!(result.is_ok(), "remote agent exists, but result is {result:?}; agent GET count={gets} (only preflight, no reconciliation)");
}
