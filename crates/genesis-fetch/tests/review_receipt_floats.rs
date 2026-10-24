//! Henry's Claude: review regressions for #81 Fetch receipts. Both message
//! comparison and receipt hashing depend on JSON floats that serde_json (built
//! without `float_roundtrip`) does not round-trip exactly, and `PartialEq`
//! treats 0.0 and -0.0 as equal while they serialize differently. These tests
//! FAIL on main `0c689d5`; they describe the expected behavior. Test-only.
#![allow(dead_code)]
use chrono::{DateTime, Duration, Utc};
use genesis_fetch::{
    protocol::{Body, InboxItem, InboxPage, Message, Task},
    runtime::{Disposition, RuntimeStore},
};
use serde_json::{json, Value};

const BRIDGE: &str = "agent1qgllj55vfr6ueyktacwgsv9fysg0qtexqrv4l738eq006km8e2hxu8pg5ev";
const PEER: &str = "agent1q2hsltlcv7afefqgreazmw8fce28fql0dnlas6jezt9nzqceru7au62r39n";
const OTHER: &str = "agent1qwggt4n8xns6fk2e7wxwrsp8a5khlw5h4dewnrm595vqd5ucuhfss02kw06";
const SECRET: &str = "PRIVATE_RECEIPT_INPUT_OUTPUT_TERMS";

fn at() -> DateTime<Utc> {
    "2000-01-01T00:00:00Z".parse().unwrap()
}
fn id(n: u128) -> String {
    uuid::Uuid::from_u128(n).to_string()
}
fn message(n: u128, body: Body) -> Message {
    Message {
        schema_version: 1,
        message_id: id(n),
        org_id: "org-a".into(),
        mission_id: "mission-a".into(),
        contract_id: "contract-a".into(),
        correlation_id: "correlation-a".into(),
        expires_at: at() + Duration::minutes(10),
        body,
    }
}
fn recruit() -> Message {
    message(
        1,
        Body::RecruitRequest {
            requirement: SECRET.into(),
            capabilities: vec!["venue.select".into()],
            lease_expires_at: at() + Duration::hours(1),
        },
    )
}
fn offer() -> Message {
    message(
        2,
        Body::Offer {
            recruit_message_id: id(1),
            offer_id: "offer-a".into(),
            capabilities: vec!["venue.select".into(), "venue.explain".into()],
            terms: json!({"note":SECRET}),
            lease_expires_at: at() + Duration::minutes(30),
        },
    )
}
fn accept() -> Message {
    message(
        3,
        Body::AcceptOffer {
            offer_message_id: id(2),
            offer_id: "offer-a".into(),
        },
    )
}
fn accepted() -> Message {
    message(
        4,
        Body::ContractAccepted {
            accept_message_id: id(3),
            offer_id: "offer-a".into(),
        },
    )
}
fn task(n: u128) -> Message {
    task_with(n, json!({"facts":SECRET}))
}
fn task_with(n: u128, input: Value) -> Message {
    message(
        n,
        Body::TaskRequest {
            task: Task {
                job_id: format!("job-{n}"),
                attempt_id: format!("attempt-{n}"),
                role_id: "role-a".into(),
                assignment_id: "assignment-a".into(),
                idempotency_key: format!("key-{n}"),
                epoch: 7,
                deadline: at() + Duration::minutes(5),
                input,
            },
        },
    )
}
fn result(n: u128, request: &Message, output: Value) -> Message {
    let Body::TaskRequest { task } = &request.body else {
        panic!()
    };
    message(
        n,
        Body::TaskResult {
            task_request_id: request.message_id.clone(),
            job_id: task.job_id.clone(),
            attempt_id: task.attempt_id.clone(),
            epoch: task.epoch,
            output,
        },
    )
}
fn receive(store: &RuntimeStore, m: Message) -> Disposition {
    let sequence = store.cursor().unwrap() + 1;
    store
        .ingest(
            &InboxPage {
                schema_version: 1,
                address: BRIDGE.into(),
                next_cursor: sequence,
                items: vec![InboxItem {
                    sequence,
                    sender: PEER.into(),
                    message: m,
                }],
            },
            at(),
        )
        .unwrap()
        .remove(0)
        .disposition
}
fn activate(store: &RuntimeStore) {
    store.prepare(PEER, recruit(), at()).unwrap();
    assert_eq!(receive(store, offer()), Disposition::Applied);
    store.prepare(PEER, accept(), at()).unwrap();
    assert_eq!(receive(store, accepted()), Disposition::Applied);
}

fn store() -> (tempfile::TempDir, RuntimeStore) {
    let dir = tempfile::tempdir().unwrap();
    let store = RuntimeStore::open(&dir.path().join("fetch.sqlite"), "org-a", BRIDGE).unwrap();
    activate(&store);
    (dir, store)
}

/// A float that changes when written and read back without float_roundtrip.
const LOSSY: &str = "1.0715660391465826e-75";

#[test]
fn review_one_interaction_has_one_receipt_hash_whatever_the_caller_encodes() {
    let (_dir, store) = store();
    let prepared = task_with(5, json!({"x": 0.0}));
    store.prepare(PEER, prepared.clone(), at()).unwrap();
    assert_eq!(
        receive(&store, result(6, &prepared, json!({"ok": true}))),
        Disposition::Applied
    );
    let exact = store
        .task_receipt(PEER, &recruit(), &accept(), &prepared)
        .unwrap();
    // prepare() refuses this as a different message under the same ID...
    let variant = task_with(5, json!({"x": -0.0}));
    assert!(store.prepare(PEER, variant.clone(), at()).is_err());
    // ...so the receipt must refuse it too, or at least return the same hash.
    match store.task_receipt(PEER, &recruit(), &accept(), &variant) {
        Err(_) => {}
        Ok(other) => assert_eq!(
            other.receipt_sha256(),
            exact.receipt_sha256(),
            "same stored interaction, two receipt hashes"
        ),
    }
}

#[test]
fn review_a_prepared_task_with_a_valid_float_input_is_sent_and_receipted_exactly() {
    let (_dir, store) = store();
    let input: Value = serde_json::from_str(&format!("{{\"x\":{LOSSY}}}")).unwrap();
    let prepared = task_with(5, input);
    store.prepare(PEER, prepared.clone(), at()).unwrap();
    // The peer must be sent exactly what was prepared.
    let sent = store.pending(at()).unwrap();
    let sent = sent
        .iter()
        .find(|s| s.message.message_id == prepared.message_id)
        .unwrap();
    assert_eq!(
        serde_json::to_string(&sent.message).unwrap(),
        serde_json::to_string(&prepared).unwrap(),
        "outbox sends a different input than was prepared"
    );
    assert_eq!(
        receive(&store, result(6, &prepared, json!({"ok": true}))),
        Disposition::Applied
    );
    store
        .task_receipt(PEER, &recruit(), &accept(), &prepared)
        .expect("an Applied result for an exactly prepared task has a receipt");
}

#[test]
fn review_an_applied_wire_result_with_a_valid_float_output_has_a_receipt() {
    let (_dir, store) = store();
    store.prepare(PEER, task(5), at()).unwrap();
    // Decoded from bytes, as the bridge page is.
    let output: Value = serde_json::from_str(&format!("{{\"score\":{LOSSY}}}")).unwrap();
    assert_eq!(
        receive(&store, result(6, &task(5), output)),
        Disposition::Applied
    );
    store
        .task_receipt(PEER, &recruit(), &accept(), &task(5))
        .expect("an Applied result has a receipt");
}
