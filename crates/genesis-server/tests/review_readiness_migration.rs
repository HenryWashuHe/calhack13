//! PR62 upgrade regression: a legacy early receipt is not trusted dispatch evidence.
//! Uses the exact persisted shape from the pre-fix failed-retirement regression.
//! No provider calls or receipt HTTP are needed to reproduce migration promotion.
use async_trait::async_trait;
use genesis_core::engine::RoleToProvision;
use genesis_core::ids::RoleId;
use genesis_core::model::{AgentBinding, Provider};
use genesis_providers::ProviderError;
use genesis_server::gateway_readiness::{
    get_challenge, GatewayConfig, GatewayReadiness, ProbeDispatch, ProbeRequest, ReadinessProbe,
};
use genesis_server::specialist::{Readiness, ReadinessVerifier};
use genesis_server::{build_org, OrgConfig};
use genesis_verification::json_sha256;
use rusqlite::params;
use serde_json::json;
use std::sync::Arc;

struct NoReceiptProbe;
#[async_trait]
impl ReadinessProbe for NoReceiptProbe {
    async fn dispatch(&self, _: &ProbeRequest) -> Result<ProbeDispatch, ProviderError> {
        Ok(ProbeDispatch {
            probe_ref: Some("fresh-trusted-run".into()),
        })
    }
}

#[tokio::test]
async fn upgrading_a_legacy_unreconciled_receipt_must_not_promote_it_to_ready() {
    let directory = tempfile::tempdir().unwrap();
    let db = directory.path().join("legacy-unreconciled.sqlite");
    let mut config = OrgConfig::dev("org_a", "Org A");
    config.db = Some(db.clone());
    let role = RoleToProvision {
        key: "monitor".into(),
        role_id: RoleId::new("role-monitor"),
        name: "Monitor".into(),
        charter: "Read venue updates".into(),
        authority: serde_json::from_value(
            json!({"scopes":["mission.coordinate"],"budget_cents":0}),
        )
        .unwrap(),
        provider: Provider::Creao,
    };
    let binding = AgentBinding {
        provider: Provider::Creao,
        provider_agent_id: "created-agent".into(),
        config_digest: "sha256:config".into(),
        conversation_ref: None,
    };
    let subject = json_sha256(&json!({
        "org_id": "org_a", "provider": "creao", "role_id": role.role_id,
        "provider_agent_id": binding.provider_agent_id, "config_digest": binding.config_digest,
    }));
    {
        let conn = rusqlite::Connection::open(&db).unwrap();
        conn.execute_batch(
            "CREATE TABLE readiness_challenges (
                challenge_id TEXT PRIMARY KEY, subject TEXT NOT NULL, attempt INTEGER NOT NULL,
                nonce_sha256 TEXT NOT NULL, provider TEXT NOT NULL, role_id TEXT NOT NULL,
                role_key TEXT NOT NULL, provider_agent_id TEXT NOT NULL, config_digest TEXT NOT NULL,
                status TEXT NOT NULL, probe_ref TEXT, detail TEXT, issued_at TEXT NOT NULL,
                expires_at TEXT NOT NULL, received_at TEXT, UNIQUE(subject, attempt));"
        ).unwrap();
        // Before PR62, accept_receipt stores the receipt's provisional run in
        // probe_ref. If mismatch retirement aborts, this is precisely what remains.
        conn.execute(
            "INSERT INTO readiness_challenges VALUES (
                'rc-unreconciled', ?1, 1, 'hash', 'creao', 'role-monitor', 'monitor',
                'created-agent', 'sha256:config', 'received', 'unrelated-native-probe-run',
                NULL, '2026-10-22T00:00:00Z', '2099-01-01T00:00:00Z', '2026-10-22T00:00:01Z')",
            params![subject],
        )
        .unwrap();
    }
    let app = build_org(&config, None).unwrap();
    let verifier = GatewayReadiness::new(
        app.store.clone(),
        app.org_id.clone(),
        Provider::Creao,
        Arc::new(NoReceiptProbe),
        GatewayConfig::from_public_url("http://127.0.0.1:1").unwrap(),
    )
    .unwrap();
    let migrated = get_challenge(&app.store, "rc-unreconciled")
        .unwrap()
        .unwrap();
    eprintln!("migrated legacy receipt: {migrated:?}");
    let readiness = verifier.verify(&role, &binding).await.unwrap();
    assert!(
        !matches!(readiness, Readiness::Ready { .. }),
        "legacy Received does not prove dispatch correlation; migration promoted {readiness:?}"
    );
}
