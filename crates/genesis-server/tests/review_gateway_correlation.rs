//! Negative review regression for PR26's early receipt/run correlation race.
//! All provider behavior is scripted; the receipt uses the real HTTP route.

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

struct Probe {
    /// `None`: no early receipt. `Some(r)`: answer before dispatch returns,
    /// with receipt `probe_ref` `r`.
    early: Option<Option<&'static str>>,
    seen: Mutex<Option<ProbeRequest>>,
    challenges: Mutex<Vec<String>>,
    receipt_status: Mutex<Option<u16>>,
}

#[async_trait]
impl ReadinessProbe for Probe {
    async fn dispatch(&self, request: &ProbeRequest) -> Result<ProbeDispatch, ProviderError> {
        *self.seen.lock().unwrap() = Some(request.clone());
        self.challenges
            .lock()
            .unwrap()
            .push(request.challenge_id.clone());
        if let Some(early_ref) = self.early {
            let mut receipt = request.expected_receipt();
            match early_ref {
                Some(r) => receipt["probe_ref"] = json!(r),
                None => {
                    receipt.as_object_mut().unwrap().remove("probe_ref");
                }
            }
            let response = reqwest::Client::new()
                .post(&request.receipt_url)
                .json(&receipt)
                .send()
                .await
                .unwrap();
            *self.receipt_status.lock().unwrap() = Some(response.status().as_u16());
        }
        Ok(ProbeDispatch {
            probe_ref: Some(TRUSTED_RUN.into()),
        })
    }
}

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

async fn probe_case(early_receipt: bool) -> (u16, bool, Option<String>) {
    let c = run_case(early_receipt.then_some(Some(WRONG_RUN))).await;
    (c.status, c.ready, c.stored_run)
}

struct Case {
    status: u16,
    ready: bool,
    stored_run: Option<String>,
    first: genesis_server::gateway_readiness::Challenge,
    challenges: Vec<String>,
}

async fn run_case(early: Option<Option<&'static str>>) -> Case {
    let early_receipt = early.is_some();
    let app = build_org(&OrgConfig::dev("org_a", "Org A"), None).unwrap();
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let base = format!("http://{}", listener.local_addr().unwrap());
    let router = app.clone().router();
    let server = tokio::spawn(async move { axum::serve(listener, router).await.unwrap() });
    let probe = Arc::new(Probe {
        early,
        seen: Mutex::new(None),
        challenges: Mutex::new(Vec::new()),
        receipt_status: Mutex::new(None),
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
    verifier.verify(&role, &binding).await.unwrap();
    let request = probe.seen.lock().unwrap().clone().unwrap();
    let first_id = request.challenge_id.clone();
    let status = if early_receipt {
        probe.receipt_status.lock().unwrap().unwrap()
    } else {
        let mut receipt = request.expected_receipt();
        receipt["probe_ref"] = json!(WRONG_RUN);
        reqwest::Client::new()
            .post(&request.receipt_url)
            .json(&receipt)
            .send()
            .await
            .unwrap()
            .status()
            .as_u16()
    };
    let ready = matches!(
        verifier.verify(&role, &binding).await.unwrap(),
        Readiness::Ready { .. }
    );
    let challenge = get_challenge(&app.store, &first_id).unwrap().unwrap();
    let challenges = probe.challenges.lock().unwrap().clone();
    server.abort();
    Case {
        status,
        ready,
        stored_run: challenge.probe_ref.clone(),
        first: challenge,
        challenges,
    }
}

#[tokio::test]
async fn early_receipt_must_not_override_trusted_dispatch_run_identity() {
    let (status, ready, stored_run) = probe_case(true).await;
    eprintln!("early wrong-run receipt: HTTP={status}, ready={ready}, stored_run={stored_run:?}");
    assert!(
        !ready,
        "a receipt naming {WRONG_RUN} must not remain Ready after dispatch identifies {TRUSTED_RUN}; HTTP={status}, stored_run={stored_run:?}"
    );
}

#[tokio::test]
async fn post_dispatch_wrong_run_is_rejected_control() {
    let (status, ready, stored_run) = probe_case(false).await;
    assert_eq!(status, 403);
    assert!(!ready);
    assert_eq!(stored_run.as_deref(), Some(TRUSTED_RUN));
}

// Henry's Claude follow-ups: early receipts that agree with (or omit) the
// trusted run are kept; a mismatch retires only that challenge.

#[tokio::test]
async fn early_receipt_matching_the_trusted_run_stays_ready() {
    let c = run_case(Some(Some(TRUSTED_RUN))).await;
    assert_eq!(c.status, 200);
    assert!(c.ready);
    assert_eq!(c.stored_run.as_deref(), Some(TRUSTED_RUN));
    assert_eq!(
        c.challenges.len(),
        1,
        "no new challenge for a valid early receipt"
    );
}

#[tokio::test]
async fn early_receipt_without_a_run_stays_ready_and_records_the_trusted_run() {
    let c = run_case(Some(None)).await;
    assert_eq!(c.status, 200);
    assert!(c.ready);
    assert_eq!(c.stored_run.as_deref(), Some(TRUSTED_RUN));
    assert_eq!(c.challenges.len(), 1);
}

#[tokio::test]
async fn early_wrong_run_retires_the_challenge_and_issues_a_new_one() {
    let c = run_case(Some(Some(WRONG_RUN))).await;
    assert!(!c.ready);
    assert_eq!(
        c.first.status,
        genesis_server::gateway_readiness::ChallengeStatus::Expired
    );
    assert_eq!(
        c.stored_run.as_deref(),
        Some(TRUSTED_RUN),
        "trusted run is the record"
    );
    assert!(c
        .first
        .detail
        .as_deref()
        .unwrap_or("")
        .contains("different run"));
    assert_eq!(
        c.challenges.len(),
        2,
        "the next pass issues a fresh challenge"
    );
    assert_ne!(c.challenges[0], c.challenges[1]);
}

struct RetirementSaveFailureProbe {
    db: std::path::PathBuf,
    seen: Mutex<Option<ProbeRequest>>,
}

#[async_trait]
impl ReadinessProbe for RetirementSaveFailureProbe {
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
                "CREATE TRIGGER fail_early_receipt_retirement
             BEFORE UPDATE ON readiness_challenges WHEN NEW.status = 'expired'
             BEGIN SELECT RAISE(ABORT, 'injected retirement failure'); END;",
            )
            .unwrap();
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
    assert_eq!(persisted.probe_ref.as_deref(), Some(WRONG_RUN));
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

// The original retry regression above is unchanged; this covers a real database reopen.
#[tokio::test]
async fn failed_retirement_must_not_promote_the_mismatched_receipt_after_restart() {
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
    assert_eq!(persisted.probe_ref.as_deref(), Some(WRONG_RUN));
    rusqlite::Connection::open(&db)
        .unwrap()
        .execute_batch("DROP TRIGGER fail_early_receipt_retirement")
        .unwrap();
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
    let retried = restored.verify(&role, &binding).await.unwrap();
    assert!(
        !matches!(retried, Readiness::Ready { .. }),
        "a failed mismatch-retirement save must remain fail-closed, got {retried:?}"
    );
}
