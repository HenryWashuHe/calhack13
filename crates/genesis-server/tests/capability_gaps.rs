//! The acquisition-driver boundary for committed capability gaps.

use genesis_core::ids::{CapabilityGapId, CapabilityId};
use genesis_core::model::{CapabilityGapStatus, CapabilityRef};
use genesis_server::capability_gaps::{open_gaps, transition};
use genesis_server::{build_org, spawn_org, OrgConfig};
use serde_json::{json, Value};
use std::time::Duration;

async fn post(base: &str, token: &str, path: &str, body: Value) -> (u16, Value) {
    let r = reqwest::Client::new()
        .post(format!("{base}/api/v1{path}"))
        .bearer_auth(token)
        .json(&body)
        .send()
        .await
        .unwrap();
    let s = r.status().as_u16();
    (s, r.json().await.unwrap_or(Value::Null))
}
async fn get(base: &str, token: &str, path: &str) -> Value {
    reqwest::Client::new()
        .get(format!("{base}/api/v1{path}"))
        .bearer_auth(token)
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap()
}

#[tokio::test]
async fn a_committed_gap_is_listed_and_moves_forward_once() {
    let cfg = OrgConfig::dev("org_a", "Org A");
    let app = build_org(&cfg, None).unwrap();
    let addr = spawn_org(app.clone(), "127.0.0.1:0").await.unwrap();
    let base = format!("http://{addr}");
    let op = cfg.tokens.operator.clone();
    let (_, m) = post(&base, &op, "/missions", json!({"goal": "Deliver", "roles": [{"key": "lead", "name": "Lead", "charter": "Owns it",
        "authority": {"scopes": ["mission.coordinate"], "budget_cents": 0}}], "responsibilities": [{"purpose": "Coordinate", "owner": "lead"}]})).await;
    let mission = m["mission_id"].as_str().unwrap().to_owned();
    let (_, o) = post(&base, &op, &format!("/missions/{mission}/observations"),
        json!({"source": "portal", "summary": "unreadable listing", "evidence_ref": "https://portal.example/x/index.html"})).await;
    let obs = o["observation_id"].as_str().unwrap().to_owned();
    let (_, r) = post(
        &base,
        &op,
        &format!("/missions/{mission}/requirements"),
        json!({"summary": "Read listings", "evidence": [obs], "acceptance": "extracted"}),
    )
    .await;
    let state = get(&base, &op, "/organization").await;
    let lead = state["roles"]
        .as_object()
        .unwrap()
        .keys()
        .next()
        .unwrap()
        .clone();
    let (s, rec) = post(&base, &op, "/org-changes", json!({"client_request_id": "gap-1", "proposal": {
        "expected_org_revision": state["revision"], "trigger_evidence_ids": [obs], "rationale": "need a reader", "budget_limit_cents": 0,
        "operations": [
            {"op": "AssignResponsibility", "target": {"kind": "from_requirement", "key": "read", "requirement_id": r["requirement_id"],
              "purpose": "Keep listings current", "trigger": "change", "termination": "done"}, "owner": {"kind": "existing", "role_id": lead}},
            {"op": "RequestCapability", "key": "reader", "requirement_id": r["requirement_id"], "role": {"kind": "existing", "role_id": lead},
             "responsibility": {"kind": "new", "key": "read"}, "capability_id": "listing-reader", "purpose": "Read listings",
             "observed_sources": ["https://portal.example/x/table.html"]}]}})).await;
    assert_eq!(s, 202, "{rec}");
    let id = rec["change_id"].as_str().unwrap().to_owned();
    for _ in 0..100 {
        if get(&base, &op, &format!("/org-changes/{id}")).await["status"] == "COMMITTED" {
            break;
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    let open = open_gaps(&app).unwrap();
    assert_eq!(open.len(), 1);
    let gap = open[0].gap_id.clone();

    let acquiring = CapabilityGapStatus::Acquiring {
        acquisition_id: "acq-1".into(),
    };
    let g = transition(&app, &gap, acquiring.clone()).await.unwrap();
    assert_eq!(g.status, acquiring);
    // Repeating the same transition (lost response, concurrent driver) is a no-op.
    let rev = app.current_state().unwrap().revision;
    transition(&app, &gap, acquiring.clone()).await.unwrap();
    assert_eq!(app.current_state().unwrap().revision, rev);
    assert!(
        open_gaps(&app).unwrap().is_empty(),
        "claimed gaps are no longer open"
    );

    let cap = || CapabilityRef {
        capability_id: CapabilityId::new("listing-reader"),
        version: "1".into(),
    };
    let wrong = CapabilityGapStatus::Acquired {
        acquisition_id: "acq-1".into(),
        capability: CapabilityRef {
            capability_id: CapabilityId::new("other"),
            version: "1".into(),
        },
        publication_ref: None,
    };
    assert!(transition(&app, &gap, wrong).await.is_err());
    let acquired = CapabilityGapStatus::Acquired {
        acquisition_id: "acq-1".into(),
        capability: cap(),
        publication_ref: Some("registry:listing-reader@1".into()),
    };
    transition(&app, &gap, acquired).await.unwrap();
    // Verified and published is not installed.
    let installed = CapabilityGapStatus::Installed {
        acquisition_id: "acq-1".into(),
        capability: cap(),
        installation_id: genesis_core::ids::InstallationId::new("inst_none"),
    };
    assert!(transition(&app, &gap, installed).await.is_err());
    let refused = |reason: &str| CapabilityGapStatus::Refused {
        reason: reason.into(),
        acquisition_id: Some("acq-1".into()),
    };
    transition(&app, &gap, refused("not installable here"))
        .await
        .unwrap();
    assert!(
        transition(&app, &gap, refused("again")).await.is_err(),
        "terminal"
    );
    assert!(
        transition(&app, &CapabilityGapId::new("gap_missing"), acquiring)
            .await
            .is_err()
    );

    let events = get(&base, &op, "/events/log?after=0&limit=5000").await;
    let kinds: Vec<_> = events
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|e| e["type"].as_str())
        .filter(|t| t.starts_with("capability_gap"))
        .collect();
    assert_eq!(
        kinds,
        [
            "capability_gap_opened",
            "capability_gap_status_changed",
            "capability_gap_status_changed",
            "capability_gap_status_changed"
        ]
    );
    let state = get(&base, &op, "/organization").await;
    let saved = &state["capability_gaps"][gap.as_str()];
    assert_eq!(saved["status"]["state"], "refused");
    assert_eq!(
        saved["status"]["acquisition_id"], "acq-1",
        "the failed attempt stays recorded"
    );
}
