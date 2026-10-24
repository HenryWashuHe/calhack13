//! Henry's Claude: the planner learns which capability names the operator's
//! trusted gap policy can act on, and nothing else about that policy.
use genesis_forge::PublicExample;
use genesis_server::{
    build_org,
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
        allowed_source_urls: vec!["https://portal.example/PRIVATE_SOURCE.html".into()],
        examples: vec![PublicExample {
            url: "https://portal.example/PRIVATE_SOURCE.html".into(),
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
            json!({"capability_ids":["capacity-checker","venue-reader"]})
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
    assert_eq!(body, json!({"capability_ids":[]}));
}
