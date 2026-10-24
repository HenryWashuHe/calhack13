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
        .contains("different"));
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
        // Henry's Claude: act on the first challenge only. A correct verifier
        // retires the mismatched receipt and issues a fresh challenge; that
        // later dispatch reports the trusted run and posts nothing.
        {
            let mut seen = self.seen.lock().unwrap();
            if seen.is_some() {
                return Ok(ProbeDispatch {
                    probe_ref: Some(TRUSTED_RUN.into()),
                });
            }
            *seen = Some(request.clone());
        }
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
    // Henry's Claude: trusted and receipt runs are stored separately now.
    assert_eq!(persisted.probe_ref.as_deref(), Some(TRUSTED_RUN));
    assert_eq!(persisted.receipt_probe_ref.as_deref(), Some(WRONG_RUN));
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
    // Henry's Claude: trusted and receipt runs are stored separately now.
    assert_eq!(persisted.probe_ref.as_deref(), Some(TRUSTED_RUN));
    assert_eq!(persisted.receipt_probe_ref.as_deref(), Some(WRONG_RUN));
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

// Henry's Claude, pressure test of #26 `51dd431`: the trusted-run write is
// lost (write failure here; a crash between dispatch returning and the write
// is the same state). An early receipt naming a different run must not become
// Ready just because the trusted run was never recorded.
struct TrustedWriteFailureProbe {
    db: std::path::PathBuf,
    seen: Mutex<Option<ProbeRequest>>,
}

#[async_trait]
impl ReadinessProbe for TrustedWriteFailureProbe {
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
                "CREATE TRIGGER fail_trusted_run_write
             BEFORE UPDATE ON readiness_challenges
             WHEN OLD.probe_ref IS NULL AND NEW.probe_ref IS NOT NULL
             BEGIN SELECT RAISE(ABORT, 'injected trusted-run write failure'); END;",
            )
            .unwrap();
        Ok(ProbeDispatch {
            probe_ref: Some(TRUSTED_RUN.into()),
        })
    }
}

#[tokio::test]
async fn lost_trusted_run_must_not_promote_a_mismatched_early_receipt() {
    let directory = tempfile::tempdir().unwrap();
    let db = directory.path().join("trusted.sqlite");
    let mut config = OrgConfig::dev("org_a", "Org A");
    config.db = Some(db.clone());
    let app = build_org(&config, None).unwrap();
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let base = format!("http://{}", listener.local_addr().unwrap());
    let router = app.clone().router();
    let server = tokio::spawn(async move { axum::serve(listener, router).await.unwrap() });
    let probe = Arc::new(TrustedWriteFailureProbe {
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
    let first = verifier.verify(&role, &binding).await;
    assert!(first.is_err(), "trusted-run write failed, got {first:?}");
    let request = probe.seen.lock().unwrap().clone().unwrap();
    let persisted = get_challenge(&app.store, &request.challenge_id)
        .unwrap()
        .unwrap();
    assert_eq!(persisted.probe_ref, None, "trusted run was not recorded");
    assert_eq!(persisted.receipt_probe_ref.as_deref(), Some(WRONG_RUN));
    rusqlite::Connection::open(&db)
        .unwrap()
        .execute_batch("DROP TRIGGER fail_trusted_run_write")
        .unwrap();
    let retried = verifier.verify(&role, &binding).await.unwrap();
    server.abort();
    assert!(
        !matches!(retried, Readiness::Ready { .. }),
        "dispatch outcome is unknown, so the early receipt must stay provisional, got {retried:?}"
    );
}

// Henry's Claude: tables from before this fix gain the new columns; a row
// that had already left `dispatching` keeps its readiness.
#[tokio::test]
async fn legacy_challenge_tables_migrate_failing_closed_on_unproven_receipts() {
    let directory = tempfile::tempdir().unwrap();
    let db = directory.path().join("legacy.sqlite");
    let mut config = OrgConfig::dev("org_a", "Org A");
    config.db = Some(db.clone());
    {
        let conn = rusqlite::Connection::open(&db).unwrap();
        conn.execute_batch(
            "CREATE TABLE readiness_challenges (
                challenge_id TEXT PRIMARY KEY, subject TEXT NOT NULL, attempt INTEGER NOT NULL,
                nonce_sha256 TEXT NOT NULL, provider TEXT NOT NULL, role_id TEXT NOT NULL,
                role_key TEXT NOT NULL, provider_agent_id TEXT NOT NULL, config_digest TEXT NOT NULL,
                status TEXT NOT NULL, probe_ref TEXT, detail TEXT, issued_at TEXT NOT NULL,
                expires_at TEXT NOT NULL, received_at TEXT, UNIQUE(subject, attempt));
             INSERT INTO readiness_challenges VALUES ('rc-old', 's', 1, 'h', 'creao', 'r', 'k', 'a', 'd',
                'received', 'run-1', NULL, '2026-10-22T00:00:00Z', '2099-01-01T00:00:00Z', '2026-10-22T00:00:01Z');
             INSERT INTO readiness_challenges VALUES ('rc-sent', 's3', 1, 'h', 'creao', 'r', 'k', 'a', 'd',
                'dispatched', 'run-2', NULL, '2026-10-22T00:00:00Z', '2099-01-01T00:00:00Z', NULL);
             INSERT INTO readiness_challenges VALUES ('rc-inflight', 's2', 1, 'h', 'creao', 'r', 'k', 'a', 'd',
                'dispatching', NULL, NULL, '2026-10-22T00:00:00Z', '2099-01-01T00:00:00Z', NULL);",
        )
        .unwrap();
    }
    let app = build_org(&config, None).unwrap();
    genesis_server::gateway_readiness::ensure_schema(&app.store).unwrap();
    // A legacy receipt cannot prove which run the dispatch trusted: its run is
    // kept as the receipt's claim only, and the challenge stays unresolved.
    let old = get_challenge(&app.store, "rc-old").unwrap().unwrap();
    assert!(
        old.dispatch_resolved_at.is_none(),
        "a legacy receipt is not a recorded dispatch outcome"
    );
    assert_eq!(old.probe_ref, None);
    assert_eq!(old.receipt_probe_ref.as_deref(), Some("run-1"));
    let sent = get_challenge(&app.store, "rc-sent").unwrap().unwrap();
    assert!(
        sent.dispatch_resolved_at.is_some(),
        "a dispatched challenge recorded its outcome under the old code"
    );
    assert_eq!(sent.probe_ref.as_deref(), Some("run-2"));
    let inflight = get_challenge(&app.store, "rc-inflight").unwrap().unwrap();
    assert!(
        inflight.dispatch_resolved_at.is_none(),
        "in flight stays unresolved"
    );
    // Idempotent.
    genesis_server::gateway_readiness::ensure_schema(&app.store).unwrap();
}

// Henry's Claude: a receipt whose dispatch outcome was lost never becomes
// readiness; once the challenge expires it is retired and a fresh one issued.
struct LostOutcomeOnce {
    db: std::path::PathBuf,
    calls: Mutex<Vec<ProbeRequest>>,
}

#[async_trait]
impl ReadinessProbe for LostOutcomeOnce {
    async fn dispatch(&self, request: &ProbeRequest) -> Result<ProbeDispatch, ProviderError> {
        let first = {
            let mut calls = self.calls.lock().unwrap();
            calls.push(request.clone());
            calls.len() == 1
        };
        if first {
            let mut receipt = request.expected_receipt();
            receipt["probe_ref"] = json!(WRONG_RUN);
            let r = reqwest::Client::new()
                .post(&request.receipt_url)
                .json(&receipt)
                .send()
                .await
                .unwrap();
            assert_eq!(r.status().as_u16(), 200);
            rusqlite::Connection::open(&self.db)
                .unwrap()
                .execute_batch(
                    "CREATE TRIGGER lose_outcome BEFORE UPDATE ON readiness_challenges
                     WHEN OLD.dispatch_resolved_at IS NULL AND NEW.dispatch_resolved_at IS NOT NULL
                     BEGIN SELECT RAISE(ABORT, 'injected lost dispatch outcome'); END;",
                )
                .unwrap();
        }
        Ok(ProbeDispatch {
            probe_ref: Some(TRUSTED_RUN.into()),
        })
    }
}

#[tokio::test]
async fn an_expired_receipt_with_a_lost_dispatch_outcome_is_retired_and_reissued() {
    let directory = tempfile::tempdir().unwrap();
    let db = directory.path().join("lost.sqlite");
    let mut config = OrgConfig::dev("org_a", "Org A");
    config.db = Some(db.clone());
    let app = build_org(&config, None).unwrap();
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let base = format!("http://{}", listener.local_addr().unwrap());
    let router = app.clone().router();
    let server = tokio::spawn(async move { axum::serve(listener, router).await.unwrap() });
    let probe = Arc::new(LostOutcomeOnce {
        db: db.clone(),
        calls: Mutex::new(vec![]),
    });
    let mut cfg = GatewayConfig::from_public_url(&base).unwrap();
    cfg.ttl = std::time::Duration::from_millis(300);
    let verifier = GatewayReadiness::new(
        app.store.clone(),
        app.org_id.clone(),
        Provider::Creao,
        probe.clone(),
        cfg,
    )
    .unwrap();
    let (role, binding) = subject();
    assert!(
        verifier.verify(&role, &binding).await.is_err(),
        "outcome write failed"
    );
    rusqlite::Connection::open(&db)
        .unwrap()
        .execute_batch("DROP TRIGGER lose_outcome")
        .unwrap();
    let first = probe.calls.lock().unwrap()[0].challenge_id.clone();
    // Live: still waiting, never Ready.
    let waiting = verifier.verify(&role, &binding).await.unwrap();
    assert!(matches!(waiting, Readiness::Waiting(_)), "{waiting:?}");
    tokio::time::sleep(std::time::Duration::from_millis(400)).await;
    // Expired: retired fail-closed, and a fresh challenge goes out.
    let next = verifier.verify(&role, &binding).await.unwrap();
    server.abort();
    assert!(!matches!(next, Readiness::Ready { .. }), "{next:?}");
    let old = get_challenge(&app.store, &first).unwrap().unwrap();
    assert_eq!(
        old.status,
        genesis_server::gateway_readiness::ChallengeStatus::Expired
    );
    assert!(old.detail.unwrap_or_default().contains("never recorded"));
    assert_eq!(
        probe.calls.lock().unwrap().len(),
        2,
        "a fresh challenge was issued"
    );
}
