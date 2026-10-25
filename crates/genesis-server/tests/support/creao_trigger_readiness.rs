//! Only synthetic loopback Trigger dispatch and gateway callbacks.
use super::*;
const TRIGGER_TOKEN: &str = "capi_readiness_loopback_only";

fn trigger_runtime(provider: &ProviderFixture) -> (Runtime, RuntimeStore) {
    let store = RuntimeStore::in_memory(ORG).unwrap();
    let client = CreaoClient::for_local_testing_trigger(
        TRIGGER_TOKEN,
        AGENT,
        Limits::default(),
        AccountLimiter::in_memory(ACCOUNT, QuotaLimits::default()).unwrap(),
        &format!(
            "{}/apps/{AGENT}/runs",
            provider.endpoint.replace("/v1", "/api/v1")
        ),
    )
    .unwrap();
    (
        Runtime::new(client, store.clone(), INTERVAL).unwrap(),
        store,
    )
}

#[tokio::test]
async fn trigger_native_completion_still_requires_exact_gateway_callback() {
    let provider = ProviderFixture::with_token(
        vec![
            Reply {
                status: 202,
                body: json!({"id":RUN,"status":"pending","appId":AGENT}),
                hold: None,
            },
            Reply {
                status: 200,
                body: json!({"id":RUN,"status":"completed","result":{"ready":true}}),
                hold: None,
            },
        ],
        TRIGGER_TOKEN,
    )
    .await;
    let (runtime, store) = trigger_runtime(&provider);
    let dir = tempfile::tempdir().unwrap();
    let gateway = GatewayFixture::new(&dir.path().join("org.db")).await;
    let verifier = gateway.verifier(runtime.clone());
    assert!(matches!(
        verifier.verify(&role(), &binding()).await.unwrap(),
        Readiness::Waiting(_)
    ));
    let seen = provider.seen();
    assert_eq!(seen.len(), 1);
    assert_eq!(seen[0].path, format!("/api/v1/apps/{AGENT}/runs"));
    assert_eq!(seen[0].body.as_object().unwrap().len(), 1);
    let input = &seen[0].body["inputs"];
    assert_eq!(input["task"], "genesis_gateway_readiness");
    assert_eq!(input["context"]["org_id"], ORG);
    let receipt = input["receipt"].clone();
    let operation = store.list().unwrap().remove(0);
    assert_eq!(
        operation.spec.api_version,
        genesis_creao::ApiVersion::Trigger
    );
    let delay = (operation.next_poll_at.unwrap() - Utc::now())
        .to_std()
        .unwrap_or_default();
    tokio::time::sleep(delay + INTERVAL).await;
    assert_eq!(
        runtime
            .step(&operation.spec.operation_id, false)
            .await
            .unwrap()
            .phase,
        OperationPhase::NativeCompleted
    );
    assert!(
        matches!(
            verifier.verify(&role(), &binding()).await.unwrap(),
            Readiness::Waiting(_)
        ),
        "a published app's ready:true result cannot replace the challenge-bound receipt"
    );
    let mut wrong = receipt.clone();
    wrong["provider_agent_id"] = json!(OTHER_AGENT);
    assert_eq!(gateway.receipt(&wrong).await.0, 403);
    wrong = receipt.clone();
    wrong["challenge_id"] = json!("foreign-role-challenge");
    assert_eq!(gateway.receipt(&wrong).await.0, 403);
    assert_eq!(gateway.receipt(&receipt).await.0, 200);
    assert!(matches!(
        verifier.verify(&role(), &binding()).await.unwrap(),
        Readiness::Ready { .. }
    ));
    assert_eq!(
        provider.seen().len(),
        2,
        "readiness does not repeat the native run"
    );
    assert!(
        gateway.app.current_state().unwrap().roles.is_empty(),
        "receipt itself does not activate a role"
    );
}

#[tokio::test]
async fn trigger_readiness_wrong_app_is_refused_without_network_or_saved_intent() {
    let provider = ProviderFixture::with_token(vec![], TRIGGER_TOKEN).await;
    let (runtime, store) = trigger_runtime(&provider);
    let mut request = request(&format!("http://127.0.0.1:19001{RECEIPT_PATH}"));
    request.provider_agent_id = OTHER_AGENT.into();
    let probe = CreaoReadinessProbe::new(runtime, request.receipt_url.clone()).unwrap();
    assert!(probe.dispatch(&request).await.is_err());
    assert!(store.list().unwrap().is_empty());
    assert!(provider.seen().is_empty());
}
