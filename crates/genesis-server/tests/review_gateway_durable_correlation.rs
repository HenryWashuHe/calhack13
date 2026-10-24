//! Test-only review: real SQLite and HTTP receipts; provider dispatch is scripted.

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
use serde_json::json;
use std::sync::{Arc, Mutex};

const TRUSTED_RUN: &str = "trusted-native-probe-run";
const WRONG_RUN: &str = "unrelated-native-probe-run";

fn subject() -> (RoleToProvision, AgentBinding) {
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
    (role, binding)
}

struct RetirementSaveFailureProbe {
    db: std::path::PathBuf,
    seen: Mutex<Option<ProbeRequest>>,
}

#[async_trait]
impl ReadinessProbe for RetirementSaveFailureProbe {
    async fn dispatch(&self, request: &ProbeRequest) -> Result<ProbeDispatch, ProviderError> {
        let first_dispatch = {
            let mut seen = self.seen.lock().unwrap();
            let first = seen.is_none();
            *seen = Some(request.clone());
            first
        };
        let mut receipt = request.expected_receipt();
        receipt["probe_ref"] = json!(WRONG_RUN);
        let response = reqwest::Client::new()
            .post(&request.receipt_url)
            .json(&receipt)
            .send()
            .await
            .unwrap();
        assert_eq!(response.status().as_u16(), 200);
        if first_dispatch {
            rusqlite::Connection::open(&self.db)
                .unwrap()
                .execute_batch(
                    "CREATE TRIGGER fail_early_receipt_retirement
             BEFORE UPDATE ON readiness_challenges WHEN NEW.status = 'expired'
             BEGIN SELECT RAISE(ABORT, 'injected retirement failure'); END;",
                )
                .unwrap();
        }
        Ok(ProbeDispatch {
            probe_ref: Some(TRUSTED_RUN.into()),
        })
    }
}

#[tokio::test]
async fn failed_retirement_must_not_promote_the_mismatched_receipt_on_retry() {
    let directory = tempfile::tempdir().unwrap();
    let db = directory.path().join("retirement.sqlite");
    let mut config = OrgConfig::dev("org_a", "Org A");
    config.db = Some(db.clone());
    let app = build_org(&config, None).unwrap();
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let base = format!("http://{}", listener.local_addr().unwrap());
    let router = app.clone().router();
    let server = tokio::spawn(async move { axum::serve(listener, router).await.unwrap() });
    let probe = Arc::new(RetirementSaveFailureProbe {
        db: db.clone(),
        seen: Mutex::new(None),
    });
    let verifier = GatewayReadiness::new(
        app.store.clone(),
        app.org_id.clone(),
        Provider::Creao,
        probe.clone(),
        GatewayConfig::from_public_url(&base).unwrap(),
    )
    .unwrap();
    let (role, binding) = subject();
    assert!(verifier.verify(&role, &binding).await.is_err());
    let request = probe.seen.lock().unwrap().clone().unwrap();
    let persisted = get_challenge(&app.store, &request.challenge_id)
        .unwrap()
        .unwrap();
    assert_eq!(
        persisted.status,
        genesis_server::gateway_readiness::ChallengeStatus::Received
    );
    assert_eq!(persisted.receipt_probe_ref.as_deref(), Some(WRONG_RUN));
    assert_eq!(persisted.probe_ref.as_deref(), Some(TRUSTED_RUN));
    rusqlite::Connection::open(&db)
        .unwrap()
        .execute_batch("DROP TRIGGER fail_early_receipt_retirement")
        .unwrap();
    let retried = verifier.verify(&role, &binding).await.unwrap();
    server.abort();
    assert!(
        !matches!(retried, Readiness::Ready { .. }),
        "a failed mismatch-retirement save must remain fail-closed, got {retried:?}"
    );
}

// The previous review injected failure while retiring an already correlated
// mismatch. This variant aborts the earlier write of the trusted dispatch ID.
struct TrustedRunSaveFailureProbe {
    db: std::path::PathBuf,
    seen: Mutex<Option<ProbeRequest>>,
}

#[async_trait]
impl ReadinessProbe for TrustedRunSaveFailureProbe {
    async fn dispatch(&self, request: &ProbeRequest) -> Result<ProbeDispatch, ProviderError> {
        *self.seen.lock().unwrap() = Some(request.clone());
        let mut receipt = request.expected_receipt();
        receipt["probe_ref"] = json!(WRONG_RUN);
        let response = reqwest::Client::new()
            .post(&request.receipt_url)
            .json(&receipt)
            .send()
            .await
            .unwrap();
        assert_eq!(response.status().as_u16(), 200);
        rusqlite::Connection::open(&self.db)
            .unwrap()
            .execute_batch(
                "CREATE TRIGGER fail_trusted_probe_ref_save
             BEFORE UPDATE OF probe_ref ON readiness_challenges
             WHEN NEW.probe_ref = 'trusted-native-probe-run'
             BEGIN SELECT RAISE(ABORT, 'injected trusted reference save failure'); END;",
            )
            .unwrap();
        Ok(ProbeDispatch {
            probe_ref: Some(TRUSTED_RUN.into()),
        })
    }
}

async fn failed_trusted_run_save_case(restart: bool) {
    let directory = tempfile::tempdir().unwrap();
    let db = directory.path().join("trusted-ref.sqlite");
    let mut config = OrgConfig::dev("org_a", "Org A");
    config.db = Some(db.clone());
    let app = build_org(&config, None).unwrap();
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let base = format!("http://{}", listener.local_addr().unwrap());
    let router = app.clone().router();
    let server = tokio::spawn(async move { axum::serve(listener, router).await.unwrap() });
    let probe = Arc::new(TrustedRunSaveFailureProbe {
        db: db.clone(),
        seen: Mutex::new(None),
    });
    let verifier = GatewayReadiness::new(
        app.store.clone(),
        app.org_id.clone(),
        Provider::Creao,
        probe.clone(),
        GatewayConfig::from_public_url(&base).unwrap(),
    )
    .unwrap();
    let (role, binding) = subject();
    assert!(verifier.verify(&role, &binding).await.is_err());
    let request = probe.seen.lock().unwrap().clone().unwrap();
    let persisted = get_challenge(&app.store, &request.challenge_id)
        .unwrap()
        .unwrap();
    assert_eq!(
        persisted.status,
        genesis_server::gateway_readiness::ChallengeStatus::Received
    );
    assert_eq!(persisted.probe_ref, None);
    assert_eq!(persisted.receipt_probe_ref.as_deref(), Some(WRONG_RUN));
    rusqlite::Connection::open(&db)
        .unwrap()
        .execute_batch("DROP TRIGGER fail_trusted_probe_ref_save")
        .unwrap();
    let observed = if restart {
        server.abort();
        let _ = server.await;
        drop(verifier);
        drop(app);
        let reopened = build_org(&config, None).unwrap();
        let restored = GatewayReadiness::new(
            reopened.store.clone(),
            reopened.org_id.clone(),
            Provider::Creao,
            probe,
            GatewayConfig::from_public_url(&base).unwrap(),
        )
        .unwrap();
        restored.verify(&role, &binding).await.unwrap()
    } else {
        let result = verifier.verify(&role, &binding).await.unwrap();
        server.abort();
        result
    };
    assert!(
        !matches!(observed, Readiness::Ready { .. }),
        "trusted dispatch identified a different run, but a failed correlation save became Ready (restart={restart}): {observed:?}"
    );
}

#[tokio::test]
async fn failed_trusted_run_save_must_not_promote_early_receipt_on_retry() {
    failed_trusted_run_save_case(false).await;
}

#[tokio::test]
async fn failed_trusted_run_save_must_not_promote_early_receipt_after_restart() {
    failed_trusted_run_save_case(true).await;
}
