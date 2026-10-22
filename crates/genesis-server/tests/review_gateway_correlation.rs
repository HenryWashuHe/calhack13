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
    early_receipt: bool,
    seen: Mutex<Option<ProbeRequest>>,
    receipt_status: Mutex<Option<u16>>,
}

#[async_trait]
impl ReadinessProbe for Probe {
    async fn dispatch(&self, request: &ProbeRequest) -> Result<ProbeDispatch, ProviderError> {
        *self.seen.lock().unwrap() = Some(request.clone());
        if self.early_receipt {
            let mut receipt = request.expected_receipt();
            receipt["probe_ref"] = json!(WRONG_RUN);
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
    let app = build_org(&OrgConfig::dev("org_a", "Org A"), None).unwrap();
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let base = format!("http://{}", listener.local_addr().unwrap());
    let router = app.clone().router();
    let server = tokio::spawn(async move { axum::serve(listener, router).await.unwrap() });
    let probe = Arc::new(Probe {
        early_receipt,
        seen: Mutex::new(None),
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
    let challenge = get_challenge(&app.store, &request.challenge_id)
        .unwrap()
        .unwrap();
    server.abort();
    (status, ready, challenge.probe_ref)
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
