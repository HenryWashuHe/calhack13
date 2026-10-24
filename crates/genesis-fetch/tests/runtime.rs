use chrono::{DateTime, Duration, Utc};
use genesis_fetch::protocol::{Body, InboxItem, InboxPage, Message, SendReceipt, Task};
use genesis_fetch::runtime::{ContractPhase, Disposition, RuntimeError, RuntimeStore, TaskPhase};
use serde_json::json;

// Addresses derived with uagents 0.26.1 Identity.from_seed(seed, 0), not mock syntax.
const ORG_A: &str = "agent1qgllj55vfr6ueyktacwgsv9fysg0qtexqrv4l738eq006km8e2hxu8pg5ev";
const ORG_B: &str = "agent1qwggt4n8xns6fk2e7wxwrsp8a5khlw5h4dewnrm595vqd5ucuhfss02kw06";
const PEER: &str = "agent1q2hsltlcv7afefqgreazmw8fce28fql0dnlas6jezt9nzqceru7au62r39n";
const INTRUDER: &str = "agent1q2snsedc580w0tmjt48t92f9va3ys3zlte7zsyfetff6pnhzmwga2dh4n9r";

fn now() -> DateTime<Utc> {
    "2030-01-01T00:00:00Z".parse().unwrap()
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
        expires_at: now() + Duration::minutes(10),
        body,
    }
}
fn recruit() -> Message {
    message(
        1,
        Body::RecruitRequest {
            requirement: "Find public supplier notices".into(),
            capabilities: vec!["supplier-notices".into()],
            lease_expires_at: now() + Duration::hours(1),
        },
    )
}
fn offer() -> Message {
    message(
        2,
        Body::Offer {
            recruit_message_id: id(1),
            offer_id: "offer-a".into(),
            capabilities: vec!["supplier-notices".into()],
            terms: json!({"advisory_only": true}),
            lease_expires_at: now() + Duration::minutes(30),
        },
    )
}
fn acceptance() -> Message {
    message(
        3,
        Body::AcceptOffer {
            offer_message_id: id(2),
            offer_id: "offer-a".into(),
        },
    )
}
fn accepted_contract() -> Message {
    message(
        4,
        Body::ContractAccepted {
            accept_message_id: id(3),
            offer_id: "offer-a".into(),
        },
    )
}
fn task_request() -> Message {
    message(
        5,
        Body::TaskRequest {
            task: Task {
                job_id: "job-a".into(),
                attempt_id: "attempt-a".into(),
                role_id: "role-a".into(),
                assignment_id: "assignment-a".into(),
                idempotency_key: "job-a:attempt-a".into(),
                epoch: 7,
                deadline: now() + Duration::minutes(5),
                input: json!({"city":"Berkeley"}),
            },
        },
    )
}
fn accepted_task() -> Message {
    message(
        6,
        Body::TaskAccepted {
            task_request_id: id(5),
            job_id: "job-a".into(),
            attempt_id: "attempt-a".into(),
        },
    )
}
fn result() -> Message {
    message(
        8,
        Body::TaskResult {
            task_request_id: id(5),
            job_id: "job-a".into(),
            attempt_id: "attempt-a".into(),
            epoch: 7,
            output: json!({"candidate":"example.test/notice"}),
        },
    )
}
fn release() -> Message {
    message(
        9,
        Body::ReleaseContract {
            reason: "Responsibility reassigned".into(),
        },
    )
}
fn page(address: &str, items: Vec<(u64, &str, Message)>, after: u64) -> InboxPage {
    let next_cursor = items.last().map_or(after, |i| i.0);
    InboxPage {
        schema_version: 1,
        address: address.into(),
        next_cursor,
        items: items
            .into_iter()
            .map(|(sequence, sender, message)| InboxItem {
                sequence,
                sender: sender.into(),
                message,
            })
            .collect(),
    }
}
fn receive(store: &RuntimeStore, m: Message) -> Disposition {
    let cursor = store.cursor().unwrap();
    store
        .ingest(
            &page(store.bridge_address(), vec![(cursor + 1, PEER, m)], cursor),
            now(),
        )
        .unwrap()
        .remove(0)
        .disposition
}
fn activate(store: &RuntimeStore) {
    store.prepare(PEER, recruit(), now()).unwrap();
    assert_eq!(receive(store, offer()), Disposition::Applied);
    store.prepare(PEER, acceptance(), now()).unwrap();
    assert_eq!(receive(store, accepted_contract()), Disposition::Applied);
}
fn running() -> RuntimeStore {
    let store = RuntimeStore::in_memory("org-a", ORG_A).unwrap();
    activate(&store);
    store.prepare(PEER, task_request(), now()).unwrap();
    store
}
fn assert_rejected(disposition: &Disposition) {
    assert!(
        matches!(disposition, Disposition::Rejected { .. }),
        "{disposition:?}"
    );
}

#[test]
fn recruitment_task_progress_and_collection_preserve_native_evidence_only() {
    let store = RuntimeStore::in_memory("org-a", ORG_A).unwrap();
    assert_eq!(
        store.prepare(PEER, recruit(), now()).unwrap().phase,
        ContractPhase::Requested
    );
    assert_eq!(store.pending(now()).unwrap()[0].message, recruit());
    assert_eq!(receive(&store, offer()), Disposition::Applied);
    assert_eq!(
        store.get("contract-a").unwrap().phase,
        ContractPhase::Offered
    );
    assert!(store.pending(now()).unwrap().is_empty());
    assert_eq!(
        store.prepare(PEER, acceptance(), now()).unwrap().phase,
        ContractPhase::Accepting
    );
    assert_eq!(receive(&store, accepted_contract()), Disposition::Applied);
    assert_eq!(
        store.get("contract-a").unwrap().phase,
        ContractPhase::Active
    );
    store.prepare(PEER, task_request(), now()).unwrap();
    assert_eq!(receive(&store, accepted_task()), Disposition::Applied);
    let progress = message(
        7,
        Body::TaskProgress {
            task_request_id: id(5),
            job_id: "job-a".into(),
            attempt_id: "attempt-a".into(),
            detail: "Two public notices found".into(),
        },
    );
    assert_eq!(receive(&store, progress), Disposition::Applied);
    let mut native_result = result();
    if let Body::TaskResult { output, .. } = &mut native_result.body {
        *output = json!({"authorized":true,"assignment_id":"attacker-assignment","epoch":999,"domain_verified":true});
    }
    assert_eq!(receive(&store, native_result.clone()), Disposition::Applied);
    let contract = store.get("contract-a").unwrap();
    let task = &contract.tasks[&id(5)];
    assert_eq!(task.phase, TaskPhase::Collected);
    assert_eq!(task.request, task_request());
    assert_eq!(task.evidence.as_ref(), Some(&native_result));
    // Untrusted output claims are retained as evidence, never promoted to state.
    let saved = serde_json::to_value(task).unwrap();
    assert!(saved.get("domain_verified").is_none());
    assert!(saved.get("authorized").is_none());
    assert!(store.pending(now()).unwrap().is_empty());
}

#[test]
fn wrong_sender_context_epoch_job_and_expired_replies_are_quarantined() {
    let store = running();
    let saved = serde_json::to_value(store.get("contract-a").unwrap()).unwrap();
    let mut cases = Vec::new();
    cases.push((INTRUDER, result(), now()));
    for field in [
        "org",
        "mission",
        "correlation",
        "contract",
        "epoch",
        "job",
        "attempt",
        "request",
    ] {
        let mut m = result();
        match field {
            "org" => m.org_id = "org-b".into(),
            "mission" => m.mission_id = "mission-b".into(),
            "correlation" => m.correlation_id = "correlation-b".into(),
            "contract" => m.contract_id = "contract-b".into(),
            _ => {
                if let Body::TaskResult {
                    epoch,
                    job_id,
                    attempt_id,
                    task_request_id,
                    ..
                } = &mut m.body
                {
                    match field {
                        "epoch" => *epoch += 1,
                        "job" => *job_id = "job-b".into(),
                        "attempt" => *attempt_id = "attempt-b".into(),
                        "request" => *task_request_id = id(999),
                        _ => unreachable!(),
                    }
                }
            }
        }
        cases.push((PEER, m, now()));
    }
    let mut expired = result();
    expired.expires_at = now();
    cases.push((PEER, expired, now()));
    cases.push((PEER, result(), now() + Duration::minutes(6))); // task deadline
    for (index, (sender, mut m, time)) in cases.into_iter().enumerate() {
        m.message_id = id(100 + index as u128);
        let sequence = store.cursor().unwrap() + 1;
        let outcomes = store
            .ingest(
                &page(ORG_A, vec![(sequence, sender, m)], sequence - 1),
                time,
            )
            .unwrap();
        assert_rejected(&outcomes[0].disposition);
        assert_eq!(store.cursor().unwrap(), sequence);
        assert_eq!(
            serde_json::to_value(store.get("contract-a").unwrap()).unwrap(),
            saved
        );
    }
    assert_eq!(receive(&store, result()), Disposition::Applied);
}

#[test]
fn an_offer_must_answer_the_request_cover_capabilities_and_not_extend_the_lease() {
    let store = RuntimeStore::in_memory("org-a", ORG_A).unwrap();
    store.prepare(PEER, recruit(), now()).unwrap();
    for (index, failure) in ["request", "capabilities", "lease"].into_iter().enumerate() {
        let mut m = offer();
        m.message_id = id(30 + index as u128);
        if let Body::Offer {
            recruit_message_id,
            capabilities,
            lease_expires_at,
            ..
        } = &mut m.body
        {
            match failure {
                "request" => *recruit_message_id = id(999),
                "capabilities" => *capabilities = vec!["unrelated".into()],
                "lease" => *lease_expires_at = now() + Duration::hours(2),
                _ => unreachable!(),
            }
        }
        assert_rejected(&receive(&store, m));
        assert_eq!(
            store.get("contract-a").unwrap().phase,
            ContractPhase::Requested
        );
    }
    assert_eq!(receive(&store, offer()), Disposition::Applied);
}

#[test]
fn exact_receipts_deduplicate_even_after_expiry_but_reused_identity_cannot_replace_evidence() {
    let store = running();
    assert_eq!(receive(&store, result()), Disposition::Applied);
    let before = serde_json::to_value(store.get("contract-a").unwrap()).unwrap();
    let cursor = store.cursor().unwrap();
    let same = store
        .ingest(
            &page(ORG_A, vec![(cursor + 1, PEER, result())], cursor),
            now() + Duration::days(1),
        )
        .unwrap();
    assert_eq!(same[0].disposition, Disposition::Duplicate);
    let mut altered = result();
    if let Body::TaskResult { output, .. } = &mut altered.body {
        *output = json!({"replacement":true});
    }
    assert_rejected(&receive(&store, altered));
    let cursor = store.cursor().unwrap();
    let other_sender = store
        .ingest(
            &page(ORG_A, vec![(cursor + 1, INTRUDER, result())], cursor),
            now(),
        )
        .unwrap();
    assert_rejected(&other_sender[0].disposition);
    assert_eq!(
        serde_json::to_value(store.get("contract-a").unwrap()).unwrap(),
        before
    );
}

#[test]
fn outgoing_idempotency_binds_full_message_peer_and_task_identity() {
    let store = running();
    let revision = store.get("contract-a").unwrap().revision;
    assert_eq!(
        store
            .prepare(PEER, task_request(), now() + Duration::days(1))
            .unwrap()
            .revision,
        revision
    );
    let mut changed = task_request();
    if let Body::TaskRequest { task } = &mut changed.body {
        task.input = json!({"city":"Oakland"});
    }
    assert!(matches!(
        store.prepare(PEER, changed, now()),
        Err(RuntimeError::Conflict)
    ));
    assert!(matches!(
        store.prepare(INTRUDER, task_request(), now()),
        Err(RuntimeError::Conflict)
    ));
    let mut same_task_new_message = task_request();
    same_task_new_message.message_id = id(50);
    assert!(matches!(
        store.prepare(PEER, same_task_new_message, now()),
        Err(RuntimeError::Conflict)
    ));
    let mut reused_key = task_request();
    reused_key.message_id = id(51);
    if let Body::TaskRequest { task } = &mut reused_key.body {
        task.job_id = "job-b".into();
        task.attempt_id = "attempt-b".into();
    }
    assert!(matches!(
        store.prepare(PEER, reused_key, now()),
        Err(RuntimeError::Conflict)
    ));
    assert_eq!(store.get("contract-a").unwrap().tasks.len(), 1);
}

#[test]
fn restart_preserves_contract_cursor_collected_evidence_and_queued_acknowledgment() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("runtime.sqlite");
    {
        let store = RuntimeStore::open(&path, "org-a", ORG_A).unwrap();
        activate(&store);
        store.prepare(PEER, task_request(), now()).unwrap();
        let pending = store.pending(now()).unwrap();
        assert_eq!(pending.len(), 1);
        store
            .acknowledge(&SendReceipt {
                schema_version: 1,
                message_id: id(5),
                bridge_address: ORG_A.into(),
                recipient: PEER.into(),
                state: "queued".into(),
            })
            .unwrap();
    }
    {
        let store = RuntimeStore::open(&path, "org-a", ORG_A).unwrap();
        assert_eq!(store.cursor().unwrap(), 2);
        assert_eq!(
            store.get("contract-a").unwrap().phase,
            ContractPhase::Active
        );
        assert_eq!(
            store.get("contract-a").unwrap().tasks[&id(5)].phase,
            TaskPhase::Sent
        );
        assert!(store.pending(now()).unwrap().is_empty());
        assert_eq!(receive(&store, result()), Disposition::Applied);
    }
    let store = RuntimeStore::open(&path, "org-a", ORG_A).unwrap();
    assert_eq!(store.cursor().unwrap(), 3);
    assert_eq!(
        store.get("contract-a").unwrap().tasks[&id(5)]
            .evidence
            .as_ref(),
        Some(&result())
    );
    let db = rusqlite::Connection::open(&path).unwrap();
    let evidence: String = db
        .query_row("SELECT data FROM fetch_inbox WHERE id=?1", [id(8)], |row| {
            row.get(0)
        })
        .unwrap();
    let receipt: InboxItem = serde_json::from_str(&evidence).unwrap();
    assert_eq!(receipt.sender, PEER);
    assert_eq!(receipt.sequence, 3);
    assert_eq!(receipt.message, result());
    assert!(matches!(
        RuntimeStore::open(&path, "org-b", ORG_A),
        Err(RuntimeError::Conflict)
    ));
    assert!(matches!(
        RuntimeStore::open(&path, "org-a", ORG_B),
        Err(RuntimeError::Conflict)
    ));
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        assert_eq!(
            std::fs::metadata(path).unwrap().permissions().mode() & 0o777,
            0o600
        );
    }
}

#[test]
fn release_suppresses_prepared_sends_and_does_not_retire_another_organizations_contract() {
    let a = running();
    let b = RuntimeStore::in_memory("org-b", ORG_B).unwrap();
    let for_b = |mut m: Message| {
        m.org_id = "org-b".into();
        m
    };
    b.prepare(PEER, for_b(recruit()), now()).unwrap();
    assert_eq!(receive(&b, for_b(offer())), Disposition::Applied);
    b.prepare(PEER, for_b(acceptance()), now()).unwrap();
    assert_eq!(
        receive(&b, for_b(accepted_contract())),
        Disposition::Applied
    );
    b.prepare(PEER, for_b(task_request()), now()).unwrap();
    a.prepare(PEER, release(), now()).unwrap();
    let pending = a.pending(now()).unwrap();
    assert_eq!(pending.len(), 1);
    assert_eq!(pending[0].message, release());
    assert_rejected(&receive(&a, result()));
    assert_eq!(b.pending(now()).unwrap().len(), 1);
    assert_eq!(receive(&b, for_b(result())), Disposition::Applied);
    assert_eq!(b.get("contract-a").unwrap().phase, ContractPhase::Active);
    let mut replacement = recruit();
    replacement.message_id = id(100);
    assert!(matches!(
        a.prepare(PEER, replacement, now()),
        Err(RuntimeError::Conflict)
    ));
    assert_eq!(
        a.get("contract-a").unwrap().tasks[&id(5)].phase,
        TaskPhase::Sent
    );
}

#[test]
fn result_before_task_acknowledgment_is_collected_once_and_late_ack_cannot_regress_it() {
    let store = running();
    assert_eq!(receive(&store, result()), Disposition::Applied);
    assert_rejected(&receive(&store, accepted_task()));
    let mut another_result = result();
    another_result.message_id = id(42);
    assert_rejected(&receive(&store, another_result));
    let task = &store.get("contract-a").unwrap().tasks[&id(5)];
    assert_eq!(task.phase, TaskPhase::Collected);
    assert_eq!(task.evidence.as_ref(), Some(&result()));
}

#[test]
fn task_deadline_and_contract_release_ack_are_enforced_before_send_or_state_change() {
    let store = running();
    assert!(store
        .pending(now() + Duration::minutes(5))
        .unwrap()
        .is_empty());
    store.prepare(PEER, release(), now()).unwrap();
    let bad = message(
        10,
        Body::ContractReleased {
            release_message_id: id(999),
        },
    );
    assert_rejected(&receive(&store, bad));
    let good = message(
        11,
        Body::ContractReleased {
            release_message_id: id(9),
        },
    );
    assert_eq!(receive(&store, good.clone()), Disposition::Applied);
    assert_eq!(
        store.get("contract-a").unwrap().release_ack.as_ref(),
        Some(&good)
    );
    let mut fresh = task_request();
    fresh.message_id = id(12);
    assert!(matches!(
        store.prepare(PEER, fresh, now()),
        Err(RuntimeError::Conflict)
    ));
}

#[test]
fn forged_queue_receipts_cannot_hide_a_pending_task() {
    let store = running();
    let good = SendReceipt {
        schema_version: 1,
        message_id: id(5),
        bridge_address: ORG_A.into(),
        recipient: PEER.into(),
        state: "queued".into(),
    };
    for variant in ["bridge", "recipient", "state", "message", "schema"] {
        let mut receipt = good.clone();
        match variant {
            "bridge" => receipt.bridge_address = ORG_B.into(),
            "recipient" => receipt.recipient = INTRUDER.into(),
            "state" => receipt.state = "completed".into(),
            "message" => receipt.message_id = id(999),
            "schema" => receipt.schema_version = 2,
            _ => unreachable!(),
        }
        assert!(store.acknowledge(&receipt).is_err());
        assert_eq!(store.pending(now()).unwrap().len(), 1);
    }
    store.acknowledge(&good).unwrap();
    store.acknowledge(&good).unwrap();
    assert!(store.pending(now()).unwrap().is_empty());
    assert_eq!(
        store.get("contract-a").unwrap().tasks[&id(5)].phase,
        TaskPhase::Sent
    );
}

#[test]
fn failed_outbox_insert_rolls_back_recruitment_and_can_retry_cleanly() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("runtime.sqlite");
    let store = RuntimeStore::open(&path, "org-a", ORG_A).unwrap();
    let db = rusqlite::Connection::open(&path).unwrap();
    db.execute_batch("CREATE TRIGGER fail_outbox BEFORE INSERT ON fetch_outbox BEGIN SELECT RAISE(ABORT,'injected failure'); END;").unwrap();
    assert!(matches!(
        store.prepare(PEER, recruit(), now()),
        Err(RuntimeError::Storage)
    ));
    assert!(matches!(
        store.get("contract-a"),
        Err(RuntimeError::NotFound)
    ));
    assert!(store.pending(now()).unwrap().is_empty());
    db.execute_batch("DROP TRIGGER fail_outbox;").unwrap();
    assert_eq!(
        store.prepare(PEER, recruit(), now()).unwrap().phase,
        ContractPhase::Requested
    );
    assert_eq!(store.pending(now()).unwrap().len(), 1);
}

#[test]
fn failed_inbox_write_rolls_back_prior_replies_in_page_and_cursor() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("runtime.sqlite");
    let store = RuntimeStore::open(&path, "org-a", ORG_A).unwrap();
    store.prepare(PEER, recruit(), now()).unwrap();
    let db = rusqlite::Connection::open(&path).unwrap();
    db.execute_batch(&format!("CREATE TRIGGER fail_second BEFORE INSERT ON fetch_inbox WHEN NEW.id='{}' BEGIN SELECT RAISE(ABORT,'injected failure'); END;",id(40))).unwrap();
    let mut unknown = result();
    unknown.message_id = id(40);
    unknown.contract_id = "missing".into();
    let input = page(ORG_A, vec![(1, PEER, offer()), (2, PEER, unknown)], 0);
    assert!(matches!(
        store.ingest(&input, now()),
        Err(RuntimeError::Storage)
    ));
    assert_eq!(
        store.get("contract-a").unwrap().phase,
        ContractPhase::Requested
    );
    assert_eq!(store.cursor().unwrap(), 0);
    db.execute_batch("DROP TRIGGER fail_second;").unwrap();
    let results = store.ingest(&input, now()).unwrap();
    assert_eq!(results[0].disposition, Disposition::Applied);
    assert_rejected(&results[1].disposition);
    assert_eq!(store.cursor().unwrap(), 2);
    assert_eq!(
        store.get("contract-a").unwrap().phase,
        ContractPhase::Offered
    );
}

#[test]
fn failed_cursor_commit_rolls_back_evidence_and_can_replay_the_page() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("runtime.sqlite");
    let store = RuntimeStore::open(&path, "org-a", ORG_A).unwrap();
    activate(&store);
    store.prepare(PEER, task_request(), now()).unwrap();
    let db = rusqlite::Connection::open(&path).unwrap();
    db.execute_batch("CREATE TRIGGER fail_cursor BEFORE UPDATE ON fetch_meta WHEN NEW.key='cursor' BEGIN SELECT RAISE(ABORT,'injected failure'); END;").unwrap();
    let input = page(ORG_A, vec![(3, PEER, result())], 2);
    assert!(matches!(
        store.ingest(&input, now()),
        Err(RuntimeError::Storage)
    ));
    assert_eq!(store.cursor().unwrap(), 2);
    assert_eq!(
        store.get("contract-a").unwrap().tasks[&id(5)].phase,
        TaskPhase::Sent
    );
    db.execute_batch("DROP TRIGGER fail_cursor;").unwrap();
    assert_eq!(
        store.ingest(&input, now()).unwrap()[0].disposition,
        Disposition::Applied
    );
}

#[test]
fn malformed_transport_pages_are_rejected_atomically_without_skipping_the_cursor() {
    let store = RuntimeStore::in_memory("org-a", ORG_A).unwrap();
    store.prepare(PEER, recruit(), now()).unwrap();
    let valid = page(ORG_A, vec![(1, PEER, offer())], 0);
    let mut wrong_address = valid.clone();
    wrong_address.address = ORG_B.into();
    let mut wrong_cursor = valid.clone();
    wrong_cursor.next_cursor = 2;
    let mut bad_sender = valid.clone();
    bad_sender.items[0].sender = "agent1invalid".into();
    let mut bad_order = valid.clone();
    bad_order.items.push(bad_order.items[0].clone());
    let mut bad_message = valid.clone();
    bad_message.items[0].message.schema_version = 2;
    for bad in [
        wrong_address,
        wrong_cursor,
        bad_sender,
        bad_order,
        bad_message,
    ] {
        assert!(store.ingest(&bad, now()).is_err());
        assert_eq!(store.cursor().unwrap(), 0);
        assert_eq!(
            store.get("contract-a").unwrap().phase,
            ContractPhase::Requested
        );
    }
    assert_eq!(
        store.ingest(&valid, now()).unwrap()[0].disposition,
        Disposition::Applied
    );
    assert!(store.ingest(&valid, now()).is_err());
    assert_eq!(store.cursor().unwrap(), 1);
}

#[test]
fn byte_capacity_rejects_unicode_backlog_and_rolls_back_new_intent() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("runtime.sqlite");
    let store = RuntimeStore::open(&path, "org-a", ORG_A).unwrap();
    let db = rusqlite::Connection::open(&path).unwrap();
    // Simulate an already full evidence backlog without manufacturing thousands
    // of contracts. TEXT length is only 24 Mi characters; stored UTF-8 is 72 MiB.
    db.execute_batch("INSERT INTO fetch_inbox(id,digest,disposition,data) VALUES ('backlog','digest','{}',replace(hex(zeroblob(12582912)),'0','€'));").unwrap();
    assert!(matches!(
        store.prepare(PEER, recruit(), now()),
        Err(RuntimeError::Capacity)
    ));
    assert!(matches!(
        store.get("contract-a"),
        Err(RuntimeError::NotFound)
    ));
    assert!(store.pending(now()).unwrap().is_empty());
    db.execute("DELETE FROM fetch_inbox WHERE id='backlog'", [])
        .unwrap();
    assert_eq!(
        store.prepare(PEER, recruit(), now()).unwrap().phase,
        ContractPhase::Requested
    );
}
