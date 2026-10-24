//! Henry's Claude: the planner learns which capability names and public
//! sources the operator's trusted gap policy can act on, and nothing else.
use genesis_core::ids::CapabilityId;
use genesis_forge::PublicExample;
use genesis_server::{
    build_org,
    capability_gap_adoption::{GapAdoptionPolicy, GapAdoptionRule},
    capability_gap_policy::{GapAcquisitionPolicy, GapAcquisitionRule},
    OrgConfig,
};
use reqwest::StatusCode;
use serde_json::{json, Value};
use std::sync::Arc;

fn rule(capability: &str) -> GapAcquisitionRule {
    GapAcquisitionRule {
        capability_id: capability.into(),
        suite_id: "PRIVATE_SUITE_ID".into(),
        actor_name_prefix: "gap-reader".into(),
        allowed_source_urls: vec![
            format!("https://portal.example/{capability}/a.html"),
            format!("https://portal.example/{capability}/b.html"),
        ],
        examples: vec![PublicExample {
            url: format!("https://portal.example/{capability}/a.html"),
            html: "<html>PRIVATE_EXAMPLE</html>".into(),
        }],
        repair: false,
    }
}

async fn get(url: &str, token: &str) -> (StatusCode, Value) {
    let response = reqwest::Client::builder()
        .no_proxy()
        .build()
        .unwrap()
        .get(format!("{url}/api/v1/capability-gaps/requestable"))
        .bearer_auth(token)
        .send()
        .await
        .unwrap();
    let status = response.status();
    (status, response.json().await.unwrap_or(Value::Null))
}

async fn serve(app: genesis_server::app::AppState) -> String {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://{}", listener.local_addr().unwrap());
    let router = app.router();
    tokio::spawn(async move { axum::serve(listener, router).await.unwrap() });
    url
}

#[tokio::test]
async fn agent_and_operator_see_policy_names_only() {
    let cfg = OrgConfig::dev("org_names", "Names");
    let mut app = build_org(&cfg, None).unwrap();
    app.gap_acquisition_policy = Some(Arc::new(GapAcquisitionPolicy {
        rules: vec![rule("venue-reader"), rule("capacity-checker")],
    }));
    let url = serve(app).await;
    for token in [&cfg.tokens.agent, &cfg.tokens.operator] {
        let (status, body) = get(&url, token).await;
        assert_eq!(status, StatusCode::OK, "{body}");
        assert_eq!(
            body,
            json!({"capabilities":[
                {"capability_id":"capacity-checker","observed_sources":[
                    "https://portal.example/capacity-checker/a.html","https://portal.example/capacity-checker/b.html"]},
                {"capability_id":"venue-reader","observed_sources":[
                    "https://portal.example/venue-reader/a.html","https://portal.example/venue-reader/b.html"]}
            ], "required_scope":"capability.acquire"})
        );
        assert!(!body.to_string().contains("PRIVATE"));
    }
    let (status, _) = get(&url, &cfg.tokens.adapter).await;
    assert!(status.is_client_error());
}

#[tokio::test]
async fn without_a_policy_the_list_is_empty() {
    let cfg = OrgConfig::dev("org_none", "None");
    let url = serve(build_org(&cfg, None).unwrap()).await;
    let (status, body) = get(&url, &cfg.tokens.agent).await;
    assert_eq!(status, StatusCode::OK);
    // No acquisition policy: no scope to hold.
    assert_eq!(body, json!({"capabilities":[]}));
}

fn adopt(capability: &str) -> GapAdoptionRule {
    GapAdoptionRule {
        capability_id: CapabilityId::new(capability),
        suite_id: "PRIVATE_ADOPTION_SUITE".into(),
        synthesis_fallback: false,
    }
}

#[tokio::test]
async fn adoption_only_names_are_listed_without_sources_or_scope() {
    // Org B: adopt-first only. Without these names B's planner had no list,
    // and a request under any other name is never adopted.
    let cfg = OrgConfig::dev("org_adopt", "Adopt");
    let mut app = build_org(&cfg, None).unwrap();
    app.gap_adoption_policy = Some(Arc::new(GapAdoptionPolicy {
        rules: vec![adopt("venue-lookup"), adopt("capacity-check")],
    }));
    let url = serve(app).await;
    let (status, body) = get(&url, &cfg.tokens.agent).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(
        body,
        json!({"capabilities":[
            {"capability_id":"capacity-check","observed_sources":[],"adoption_only":true},
            {"capability_id":"venue-lookup","observed_sources":[],"adoption_only":true}
        ]})
    );
    assert!(!body.to_string().contains("PRIVATE"));
}

#[tokio::test]
async fn a_name_with_both_rules_is_listed_once_as_acquirable() {
    // Synthesis fallback needs the acquisition rule's sources and scope, so
    // the acquisition entry wins; adoption-only names keep their flag.
    let cfg = OrgConfig::dev("org_both", "Both");
    let mut app = build_org(&cfg, None).unwrap();
    app.gap_acquisition_policy = Some(Arc::new(GapAcquisitionPolicy {
        rules: vec![rule("venue-reader")],
    }));
    app.gap_adoption_policy = Some(Arc::new(GapAdoptionPolicy {
        rules: vec![adopt("venue-reader"), adopt("capacity-check")],
    }));
    let url = serve(app).await;
    let (status, body) = get(&url, &cfg.tokens.agent).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(
        body,
        json!({"capabilities":[
            {"capability_id":"capacity-check","observed_sources":[],"adoption_only":true},
            {"capability_id":"venue-reader","observed_sources":[
                "https://portal.example/venue-reader/a.html","https://portal.example/venue-reader/b.html"]}
        ], "required_scope":"capability.acquire"})
    );
}
