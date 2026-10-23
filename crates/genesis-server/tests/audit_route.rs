//! `GET /api/v1/organization/audit` serves the kernel's history audit.
use genesis_core::model::OrgState;
use genesis_server::store::insert_snapshot;
use genesis_server::{build_org, spawn_org, OrgConfig};
use serde_json::{json, Value};

#[tokio::test]
async fn the_audit_route_reports_the_stored_history_and_names_a_bad_snapshot() {
    let dir = tempfile::tempdir().unwrap();
    let db = dir.path().join("org.db");
    let mut config = OrgConfig::dev("org_audit", "Audit");
    config.db = Some(db.clone());
    let app = build_org(&config, None).unwrap();
    let addr = spawn_org(app, "127.0.0.1:0").await.unwrap();
    let http = reqwest::Client::new();
    let base = format!("http://{addr}/api/v1");
    let get = |token: &'static str| {
        let http = http.clone();
        let url = format!("{base}/organization/audit");
        async move {
            let r = http.get(url).bearer_auth(token).send().await.unwrap();
            (r.status(), r.json::<Value>().await.unwrap_or(Value::Null))
        }
    };

    let mission = json!({
        "goal": "Run the event",
        "roles": [{"key": "lead", "name": "Lead", "charter": "Coordinate",
                   "authority": {"scopes": ["mission.coordinate"], "budget_cents": 1000}}],
        "responsibilities": [{"purpose": "Coordinate", "owner": "lead"}]
    });
    let r = http
        .post(format!("{base}/missions"))
        .bearer_auth("org_audit-operator")
        .json(&mission)
        .send()
        .await
        .unwrap();
    assert!(r.status().is_success(), "{}", r.text().await.unwrap());

    let (status, body) = get("org_audit-operator").await;
    assert!(status.is_success(), "{body}");
    assert_eq!(body["violation_count"], 0, "{body}");
    assert_eq!(body["snapshots_checked"], 2);
    assert_eq!(body["last_revision"], 1);
    assert!(body["invariants"].as_array().unwrap().len() >= 16);

    let (status, _) = get("org_audit-adapter").await;
    assert_eq!(status.as_u16(), 403, "adapters cannot read the audit");

    // Tamper with storage the way only a bug or an attacker could: a new
    // revision whose role exceeds the constitution's budget.
    let mut bad: OrgState = serde_json::from_value(
        http.get(format!("{base}/organization"))
            .bearer_auth("org_audit-operator")
            .send()
            .await
            .unwrap()
            .json()
            .await
            .unwrap(),
    )
    .unwrap();
    bad.revision += 1;
    for r in bad.roles.values_mut() {
        r.authority.budget_cents = bad.constitution.budget_cents + 1;
    }
    {
        let mut conn = rusqlite::Connection::open(&db).unwrap();
        let tx = conn.transaction().unwrap();
        insert_snapshot(&tx, &bad).unwrap();
        tx.commit().unwrap();
    }
    let (status, body) = get("org_audit-agent").await;
    assert!(status.is_success());
    assert!(body["violation_count"].as_u64().unwrap() > 0, "{body}");
    let named: Vec<&str> = body["violations"]
        .as_array()
        .unwrap()
        .iter()
        .map(|v| v["invariant"].as_str().unwrap())
        .collect();
    assert!(named.contains(&"budget_limit"), "{named:?}");
    assert!(body["violations"][0]["revision"].as_u64().unwrap() == 2);
}
