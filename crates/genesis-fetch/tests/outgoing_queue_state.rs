use chrono::{DateTime, Duration, Utc};
use genesis_fetch::protocol::{Body, InboxItem, InboxPage, Message, SendReceipt, Task};
use genesis_fetch::runtime::{RuntimeError, RuntimeStore};

const BRIDGE: &str = "agent1qgllj55vfr6ueyktacwgsv9fysg0qtexqrv4l738eq006km8e2hxu8pg5ev";
const PEER: &str = "agent1q2hsltlcv7afefqgreazmw8fce28fql0dnlas6jezt9nzqceru7au62r39n";
const OTHER: &str = "agent1qwggt4n8xns6fk2e7wxwrsp8a5khlw5h4dewnrm595vqd5ucuhfss02kw06";

fn now() -> DateTime<Utc> {
    "2030-01-01T00:00:00Z".parse().unwrap()
}

fn recruit() -> Message {
    Message {
        schema_version: 1,
        message_id: uuid::Uuid::from_u128(1).to_string(),
        org_id: "org-a".into(),
        mission_id: "mission-a".into(),
        contract_id: "contract-a".into(),
        correlation_id: "correlation-a".into(),
        expires_at: now() + Duration::minutes(1),
        body: Body::RecruitRequest {
            requirement: "Find a venue".into(),
            capabilities: vec!["venue.select".into()],
            lease_expires_at: now() + Duration::minutes(1),
        },
    }
}

fn release() -> Message {
    let mut message = recruit();
    message.message_id = uuid::Uuid::from_u128(2).to_string();
    message.expires_at = now() + Duration::minutes(32);
    message.body = Body::ReleaseContract {
        reason: "Retire expired grant".into(),
    };
    message
}

fn prepare_release(store: &RuntimeStore) {
    store.prepare(PEER, recruit(), now()).unwrap();
    // The grant is already expired; only the cleanup message has a new window.
    store
        .prepare(PEER, release(), now() + Duration::minutes(2))
        .unwrap();
}

#[test]
fn queue_evidence_survives_restart_and_expiry_without_asserting_peer_release() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("fetch.db");
    let store = RuntimeStore::open(&path, "org-a", BRIDGE).unwrap();
    assert_eq!(store.outgoing_queued(PEER, &release()).unwrap(), None);
    prepare_release(&store);
    assert_eq!(
        store.outgoing_queued(PEER, &release()).unwrap(),
        Some(false)
    );
    store
        .acknowledge(&SendReceipt {
            schema_version: 1,
            message_id: release().message_id,
            bridge_address: BRIDGE.into(),
            recipient: PEER.into(),
            state: "queued".into(),
        })
        .unwrap();
    drop(store);

    let store = RuntimeStore::open(&path, "org-a", BRIDGE).unwrap();
    assert!(store
        .pending(now() + Duration::hours(1))
        .unwrap()
        .is_empty());
    assert_eq!(store.outgoing_queued(PEER, &release()).unwrap(), Some(true));
    assert!(store.get("contract-a").unwrap().release_ack.is_none());
}

#[test]
fn queue_lookup_requires_the_full_expected_message_and_recipient() {
    let store = RuntimeStore::in_memory("org-a", BRIDGE).unwrap();
    prepare_release(&store);
    let mut changed = release();
    changed.body = Body::ReleaseContract {
        reason: "Different intent with the same UUID".into(),
    };
    assert!(matches!(
        store.outgoing_queued(PEER, &changed),
        Err(RuntimeError::Conflict)
    ));
    assert!(matches!(
        store.outgoing_queued(OTHER, &release()),
        Err(RuntimeError::Conflict)
    ));
    changed = release();
    changed.expires_at += Duration::seconds(1);
    assert!(matches!(
        store.outgoing_queued(PEER, &changed),
        Err(RuntimeError::Conflict)
    ));
}

#[test]
fn queue_lookup_distinguishes_signed_zero_in_the_frozen_payload() {
    let store = RuntimeStore::in_memory("org-a", BRIDGE).unwrap();
    store.prepare(PEER, recruit(), now()).unwrap();
    let mut offer = recruit();
    offer.message_id = uuid::Uuid::from_u128(3).to_string();
    offer.body = Body::Offer {
        recruit_message_id: recruit().message_id,
        offer_id: "offer-a".into(),
        capabilities: vec!["venue.select".into()],
        terms: serde_json::json!({}),
        lease_expires_at: recruit().expires_at,
    };
    let mut accept = recruit();
    accept.message_id = uuid::Uuid::from_u128(4).to_string();
    accept.body = Body::AcceptOffer {
        offer_message_id: offer.message_id.clone(),
        offer_id: "offer-a".into(),
    };
    let mut accepted = recruit();
    accepted.message_id = uuid::Uuid::from_u128(5).to_string();
    accepted.body = Body::ContractAccepted {
        accept_message_id: accept.message_id.clone(),
        offer_id: "offer-a".into(),
    };
    for (sequence, message) in [(1, offer), (2, accepted)] {
        if sequence == 2 {
            store.prepare(PEER, accept.clone(), now()).unwrap();
        }
        store
            .ingest(
                &InboxPage {
                    schema_version: 1,
                    address: BRIDGE.into(),
                    next_cursor: sequence,
                    items: vec![InboxItem {
                        sequence,
                        sender: PEER.into(),
                        message,
                    }],
                },
                now(),
            )
            .unwrap();
    }
    let mut task = recruit();
    task.message_id = uuid::Uuid::from_u128(6).to_string();
    task.body = Body::TaskRequest {
        task: Task {
            job_id: "job-a".into(),
            attempt_id: "attempt-a".into(),
            role_id: "role-a".into(),
            assignment_id: "assignment-a".into(),
            idempotency_key: "job-a:attempt-a".into(),
            epoch: 1,
            deadline: recruit().expires_at,
            input: serde_json::json!({"budget": -0.0}),
        },
    };
    store.prepare(PEER, task.clone(), now()).unwrap();
    assert_eq!(store.outgoing_queued(PEER, &task).unwrap(), Some(false));
    let mut changed = task.clone();
    let Body::TaskRequest { task: payload } = &mut changed.body else {
        unreachable!()
    };
    payload.input = serde_json::json!({"budget": 0.0});
    assert!(matches!(
        store.outgoing_queued(PEER, &changed),
        Err(RuntimeError::Conflict)
    ));
}

#[test]
fn queue_lookup_rejects_corrupt_indexes_payload_digest_and_queue_flag() {
    for mutation in [
        "UPDATE fetch_outbox SET contract_id='contract-other' WHERE id=?1",
        "UPDATE fetch_outbox SET peer='wrong-peer' WHERE id=?1",
        "UPDATE fetch_outbox SET data=replace(data,'Retire expired grant','Changed release') WHERE id=?1",
        "UPDATE fetch_outbox SET digest='wrong-digest' WHERE id=?1",
        "UPDATE fetch_outbox SET queued=2 WHERE id=?1",
    ] {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("fetch.db");
        let store = RuntimeStore::open(&path, "org-a", BRIDGE).unwrap();
        prepare_release(&store);
        rusqlite::Connection::open(&path).unwrap().execute(mutation, [&release().message_id]).unwrap();
        assert!(matches!(store.outgoing_queued(PEER, &release()), Err(RuntimeError::Conflict)), "{mutation}");
    }
}
