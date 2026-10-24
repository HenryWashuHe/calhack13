//! Private-journal receipt tests. InboxItem inputs model the authenticated
//! bridge boundary; these tests do not claim SDK signature or cloud evidence.
use chrono::{DateTime, Duration, Utc};
use genesis_fetch::{
    protocol::{Body, InboxItem, InboxPage, Message, SendReceipt, Task},
    runtime::{Disposition, RuntimeStore},
};
use genesis_verification::json_sha256;
use rusqlite::{params, Connection};
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
                input: json!({"facts":SECRET}),
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
fn complete(store: &RuntimeStore) {
    activate(store);
    store.prepare(PEER, task(5), at()).unwrap();
    // A result may legally arrive before TaskAccepted or a queue acknowledgment.
    assert_eq!(
        receive(store, result(6, &task(5), json!({"answer":SECRET}))),
        Disposition::Applied
    );
}
fn receipt(store: &RuntimeStore) -> genesis_fetch::TaskReceipt {
    store
        .task_receipt(PEER, &recruit(), &accept(), &task(5))
        .unwrap()
}
struct Fixture {
    _dir: tempfile::TempDir,
    path: std::path::PathBuf,
    store: RuntimeStore,
}
impl Fixture {
    fn new() -> Self {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("fetch.sqlite");
        let store = RuntimeStore::open(&path, "org-a", BRIDGE).unwrap();
        complete(&store);
        Self {
            _dir: dir,
            path,
            store,
        }
    }
    fn db(&self) -> Connection {
        Connection::open(&self.path).unwrap()
    }
    fn change_inbox(&self, n: u128, mutate: impl FnOnce(&mut Value), rehash: bool) {
        let db = self.db();
        let text: String = db
            .query_row("SELECT data FROM fetch_inbox WHERE id=?1", [id(n)], |r| {
                r.get(0)
            })
            .unwrap();
        let mut value: Value = serde_json::from_str(&text).unwrap();
        mutate(&mut value);
        if rehash {
            let digest = json_sha256(
                &json!({"sender_or_recipient":value["sender"],"message":value["message"]}),
            );
            db.execute(
                "UPDATE fetch_inbox SET data=?1,digest=?2 WHERE id=?3",
                params![value.to_string(), digest, id(n)],
            )
            .unwrap();
        } else {
            db.execute(
                "UPDATE fetch_inbox SET data=?1 WHERE id=?2",
                params![value.to_string(), id(n)],
            )
            .unwrap();
        }
    }
    fn change_cached(&self, mutate: impl FnOnce(&mut Value)) {
        let db = self.db();
        let text: String = db
            .query_row(
                "SELECT data FROM fetch_contracts WHERE id='contract-a'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        let mut value: Value = serde_json::from_str(&text).unwrap();
        mutate(&mut value);
        db.execute(
            "UPDATE fetch_contracts SET data=?1 WHERE id='contract-a'",
            [value.to_string()],
        )
        .unwrap();
    }
    fn replace_output(&self, output: Value) {
        self.change_inbox(6, |v| v["message"]["body"]["output"] = output.clone(), true);
        self.change_cached(|v| v["tasks"][id(5)]["evidence"]["body"]["output"] = output);
    }
}

#[test]
fn receipts_bind_the_complete_selected_chain_and_expose_no_authority_claim() {
    let f = Fixture::new();
    let r = receipt(&f.store);
    let recruitment = r.recruitment();
    assert_eq!(recruitment.org_id(), "org-a");
    assert_eq!(recruitment.bridge_address(), BRIDGE);
    assert_eq!(recruitment.peer_address(), PEER);
    assert_eq!(recruitment.accepted_message_id(), id(4));
    assert_eq!(recruitment.lease_expires_at(), at() + Duration::minutes(30));
    let expected = json_sha256(&json!({"domain":"genesis-fetch-recruitment-receipt-v1",
        "bridge_address":BRIDGE,"peer_address":PEER,"recruit":recruit(),"offer":offer(),
        "accept":accept(),"accepted":accepted()}));
    assert_eq!(recruitment.receipt_sha256(), expected);
    assert_eq!(
        r.receipt_sha256(),
        json_sha256(&json!({"domain":"genesis-fetch-task-receipt-v1",
        "recruitment_sha256":expected,"request":task(5),"result":result(6,&task(5),json!({"answer":SECRET})),"sender":PEER}))
    );
    assert_eq!(r.request_sha256(), task(5).canonical_sha256().unwrap());
    assert_eq!(r.output_sha256(), json_sha256(&json!({"answer":SECRET})));
    assert_eq!(r.output(), &json!({"answer":SECRET}));
    assert_eq!(r.assignment_id(), "assignment-a");
    assert_eq!(r.epoch(), 7);
    assert_eq!(r.deadline(), at() + Duration::minutes(5));
    let public = serde_json::to_value(&r).unwrap();
    for name in [
        "domain_verified",
        "authorized",
        "verified_at",
        "received_at",
    ] {
        assert!(public.get(name).is_none());
    }
    assert!(!format!("{r:?} {recruitment:?}").contains(SECRET));
    assert_eq!(
        f.store
            .recruitment_receipt(PEER, &recruit(), &accept())
            .unwrap(),
        *recruitment
    );
}

#[test]
fn receipts_survive_replay_later_work_release_expiry_and_reopen_without_writes() {
    let f = Fixture::new();
    let original = receipt(&f.store);
    let cursor = f.store.cursor().unwrap();
    let before = serde_json::to_value(f.store.get("contract-a").unwrap()).unwrap();
    for _ in 0..3 {
        assert_eq!(receipt(&f.store), original);
    }
    assert_eq!(f.store.cursor().unwrap(), cursor);
    assert_eq!(
        serde_json::to_value(f.store.get("contract-a").unwrap()).unwrap(),
        before
    );
    assert_eq!(receive(&f.store, accepted()), Disposition::Duplicate);
    let mut late = accepted();
    late.message_id = id(20);
    assert!(matches!(
        receive(&f.store, late),
        Disposition::Rejected { .. }
    ));
    for n in [1, 3, 5] {
        f.store
            .acknowledge(&SendReceipt {
                schema_version: 1,
                message_id: id(n),
                bridge_address: BRIDGE.into(),
                recipient: PEER.into(),
                state: "queued".into(),
            })
            .unwrap();
    }
    f.store.prepare(PEER, task(7), at()).unwrap();
    assert_eq!(
        receive(&f.store, result(8, &task(7), Value::Null)),
        Disposition::Applied
    );
    let null = f
        .store
        .task_receipt(PEER, &recruit(), &accept(), &task(7))
        .unwrap();
    assert_eq!(null.output(), &Value::Null);
    assert_eq!(null.output_sha256(), json_sha256(&Value::Null));
    f.store
        .prepare(
            PEER,
            message(
                9,
                Body::ReleaseContract {
                    reason: "Handed back".into(),
                },
            ),
            at(),
        )
        .unwrap();
    assert_eq!(
        receive(
            &f.store,
            message(
                10,
                Body::ContractReleased {
                    release_message_id: id(9)
                }
            )
        ),
        Disposition::Applied
    );
    assert_eq!(receipt(&f.store), original);
    drop(f.store);
    let reopened = RuntimeStore::open(&f.path, "org-a", BRIDGE).unwrap();
    // All leases/deadlines are in 2000: no current authorization is inferred.
    assert_eq!(receipt(&reopened), original);
    assert!(RuntimeStore::open(&f.path, "foreign-org", BRIDGE).is_err());
    assert!(RuntimeStore::open(&f.path, "org-a", OTHER).is_err());
}

#[test]
fn exact_backend_expectations_cannot_be_replaced_by_same_ids_or_other_context() {
    let f = Fixture::new();
    let value = serde_json::to_value(task(5)).unwrap();
    for (path, replacement) in [
        ("/org_id", json!("foreign-org")),
        ("/mission_id", json!("foreign-mission")),
        ("/contract_id", json!("foreign-contract")),
        ("/correlation_id", json!("foreign-correlation")),
        ("/message_id", json!(id(25))),
        ("/expires_at", json!(at() + Duration::minutes(9))),
        ("/body/task/job_id", json!("other-job")),
        ("/body/task/attempt_id", json!("other-attempt")),
        ("/body/task/role_id", json!("other-role")),
        ("/body/task/assignment_id", json!("other-assignment")),
        ("/body/task/epoch", json!(8)),
        ("/body/task/idempotency_key", json!("other-key")),
        ("/body/task/deadline", json!(at() + Duration::minutes(4))),
        ("/body/task/input", json!({"facts":"other"})),
    ] {
        let mut changed = value.clone();
        *changed.pointer_mut(path).unwrap() = replacement;
        let changed: Message = serde_json::from_value(changed).unwrap();
        assert!(
            f.store
                .task_receipt(PEER, &recruit(), &accept(), &changed)
                .is_err(),
            "{path}"
        );
    }
    let mut changed = recruit();
    if let Body::RecruitRequest { requirement, .. } = &mut changed.body {
        *requirement = "different".into();
    }
    assert!(f
        .store
        .task_receipt(PEER, &changed, &accept(), &task(5))
        .is_err());
    let mut changed = accept();
    if let Body::AcceptOffer { offer_id, .. } = &mut changed.body {
        *offer_id = "other-offer".into();
    }
    assert!(f
        .store
        .task_receipt(PEER, &recruit(), &changed, &task(5))
        .is_err());
    assert!(f
        .store
        .task_receipt(OTHER, &recruit(), &accept(), &task(5))
        .is_err());
    assert!(f
        .store
        .task_receipt(BRIDGE, &recruit(), &accept(), &task(5))
        .is_err());
}

#[test]
fn missing_pending_or_rejected_evidence_never_becomes_a_receipt() {
    let store = RuntimeStore::in_memory("org-a", BRIDGE).unwrap();
    assert!(store
        .recruitment_receipt(PEER, &recruit(), &accept())
        .is_err());
    assert!(store.pending(at()).unwrap().is_empty());
    store.prepare(PEER, recruit(), at()).unwrap();
    receive(&store, offer());
    store.prepare(PEER, accept(), at()).unwrap();
    assert!(store
        .recruitment_receipt(PEER, &recruit(), &accept())
        .is_err());
    receive(&store, accepted());
    store.prepare(PEER, task(5), at()).unwrap();
    let mut wrong = result(6, &task(5), Value::Null);
    if let Body::TaskResult { epoch, .. } = &mut wrong.body {
        *epoch = 99;
    }
    assert!(matches!(
        receive(&store, wrong),
        Disposition::Rejected { .. }
    ));
    assert!(store
        .task_receipt(PEER, &recruit(), &accept(), &task(5))
        .is_err());
    assert_eq!(store.get("contract-a").unwrap().tasks.len(), 1);
}

#[test]
fn outbox_identity_digest_and_payload_corruption_fail_closed() {
    for sql in [
        "UPDATE fetch_outbox SET contract_id='other' WHERE id=?1",
        "UPDATE fetch_outbox SET peer='invalid-peer' WHERE id=?1",
        "UPDATE fetch_outbox SET digest='bad-digest' WHERE id=?1",
        "UPDATE fetch_outbox SET queued=2 WHERE id=?1",
        "UPDATE fetch_outbox SET data=json_set(data,'$.body.task.role_id','other') WHERE id=?1",
        "DELETE FROM fetch_outbox WHERE id=?1",
    ] {
        let f = Fixture::new();
        f.db().execute(sql, [id(5)]).unwrap();
        assert!(
            f.store
                .task_receipt(PEER, &recruit(), &accept(), &task(5))
                .is_err(),
            "{sql}"
        );
    }
    for n in [1, 3] {
        let f = Fixture::new();
        f.db()
            .execute("DELETE FROM fetch_outbox WHERE id=?1", [id(n)])
            .unwrap();
        assert!(f
            .store
            .recruitment_receipt(PEER, &recruit(), &accept())
            .is_err());
    }
}

#[test]
fn native_rows_must_match_cached_messages_and_applied_authenticated_chain() {
    for n in [2, 4, 6] {
        for field in [
            "sender",
            "context",
            "id",
            "digest",
            "disposition",
            "sequence",
            "missing",
        ] {
            let f = Fixture::new();
            match field {
                "sender" => f.change_inbox(n, |v| v["sender"] = json!(OTHER), true),
                "context" => f.change_inbox(
                    n,
                    |v| v["message"]["mission_id"] = json!("other-mission"),
                    true,
                ),
                "id" => f.change_inbox(n, |v| v["message"]["message_id"] = json!(id(90)), true),
                "sequence" => f.change_inbox(n, |v| v["sequence"] = json!(0), false),
                "digest" => {
                    f.db()
                        .execute(
                            "UPDATE fetch_inbox SET digest=?1 WHERE id=?2",
                            params!["0".repeat(64), id(n)],
                        )
                        .unwrap();
                }
                "disposition" => {
                    f.db()
                        .execute(
                            "UPDATE fetch_inbox SET disposition=?1 WHERE id=?2",
                            params![
                                json!({"disposition":"rejected","reason":"fixture"}).to_string(),
                                id(n)
                            ],
                        )
                        .unwrap();
                }
                "missing" => {
                    f.db()
                        .execute("DELETE FROM fetch_inbox WHERE id=?1", [id(n)])
                        .unwrap();
                }
                _ => unreachable!(),
            }
            assert!(
                f.store
                    .task_receipt(PEER, &recruit(), &accept(), &task(5))
                    .is_err(),
                "message{n} {field}"
            );
        }
    }
    let f = Fixture::new();
    f.change_inbox(
        6,
        |v| v["message"]["body"]["output"] = json!({"tampered":true}),
        true,
    );
    assert!(f
        .store
        .task_receipt(PEER, &recruit(), &accept(), &task(5))
        .is_err());
}

#[test]
fn ambiguous_applied_acceptance_and_invalid_order_are_rejected() {
    let f = Fixture::new();
    let mut duplicate = accepted();
    duplicate.message_id = id(50);
    let item = InboxItem {
        sequence: 50,
        sender: PEER.into(),
        message: duplicate.clone(),
    };
    f.db()
        .execute(
            "INSERT INTO fetch_inbox VALUES (?1,?2,?3,?4)",
            params![
                id(50),
                json_sha256(&json!({"sender_or_recipient":PEER,"message":duplicate})),
                serde_json::to_string(&Disposition::Applied).unwrap(),
                serde_json::to_string(&item).unwrap()
            ],
        )
        .unwrap();
    assert!(f
        .store
        .recruitment_receipt(PEER, &recruit(), &accept())
        .is_err());
    for (n, sequence) in [(4, 1), (6, 2)] {
        let f = Fixture::new();
        f.change_inbox(n, |v| v["sequence"] = json!(sequence), false);
        assert!(f
            .store
            .task_receipt(PEER, &recruit(), &accept(), &task(5))
            .is_err());
    }
}

#[test]
fn oversized_output_and_changed_cached_phase_do_not_bypass_validation() {
    let f = Fixture::new();
    // Change both copies and recompute the inbox digest: equality alone cannot
    // reject this row, so the protocol output bound is actually exercised.
    f.replace_output(json!("x".repeat(33 * 1024)));
    assert!(f
        .store
        .task_receipt(PEER, &recruit(), &accept(), &task(5))
        .is_err());
    let f = Fixture::new();
    let db = f.db();
    let text: String = db
        .query_row(
            "SELECT data FROM fetch_contracts WHERE id='contract-a'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    let mut value: Value = serde_json::from_str(&text).unwrap();
    value["tasks"][id(5)]["phase"] = json!("accepted");
    db.execute(
        "UPDATE fetch_contracts SET data=?1 WHERE id='contract-a'",
        [value.to_string()],
    )
    .unwrap();
    assert!(f
        .store
        .task_receipt(PEER, &recruit(), &accept(), &task(5))
        .is_err());
}

#[test]
fn output_byte_and_depth_boundaries_apply_to_consistent_journal_copies() {
    let f = Fixture::new();
    // Quotes consume two bytes of the protocol's 32KiB JSON-value bound.
    f.replace_output(json!("x".repeat(32 * 1024 - 2)));
    assert_eq!(
        receipt(&f.store).output().as_str().unwrap().len(),
        32 * 1024 - 2
    );
    f.replace_output(json!("x".repeat(32 * 1024 - 1)));
    assert!(f
        .store
        .task_receipt(PEER, &recruit(), &accept(), &task(5))
        .is_err());
    let f = Fixture::new();
    let mut nested = Value::Null;
    for _ in 0..65 {
        nested = json!([nested]);
    }
    f.replace_output(nested);
    assert!(f
        .store
        .task_receipt(PEER, &recruit(), &accept(), &task(5))
        .is_err());
}

#[test]
fn task_acceptance_and_progress_are_not_result_receipts() {
    let store = RuntimeStore::in_memory("org-a", BRIDGE).unwrap();
    activate(&store);
    store.prepare(PEER, task(5), at()).unwrap();
    for reply in [
        message(
            20,
            Body::TaskAccepted {
                task_request_id: id(5),
                job_id: "job-5".into(),
                attempt_id: "attempt-5".into(),
            },
        ),
        message(
            21,
            Body::TaskProgress {
                task_request_id: id(5),
                job_id: "job-5".into(),
                attempt_id: "attempt-5".into(),
                detail: "Working".into(),
            },
        ),
    ] {
        assert_eq!(receive(&store, reply), Disposition::Applied);
        assert!(store
            .task_receipt(PEER, &recruit(), &accept(), &task(5))
            .is_err());
    }
    assert_eq!(
        receive(&store, result(6, &task(5), Value::Null)),
        Disposition::Applied
    );
    assert!(store
        .task_receipt(PEER, &recruit(), &accept(), &task(5))
        .is_ok());
}

#[test]
fn separate_organizations_with_identical_message_ids_have_distinct_receipts() {
    let a = RuntimeStore::in_memory("org-a", BRIDGE).unwrap();
    complete(&a);
    let b = RuntimeStore::in_memory("org-b", BRIDGE).unwrap();
    let other = |mut m: Message| {
        m.org_id = "org-b".into();
        m
    };
    b.prepare(PEER, other(recruit()), at()).unwrap();
    receive(&b, other(offer()));
    b.prepare(PEER, other(accept()), at()).unwrap();
    receive(&b, other(accepted()));
    b.prepare(PEER, other(task(5)), at()).unwrap();
    receive(&b, other(result(6, &task(5), json!({"answer":SECRET}))));
    let ar = receipt(&a);
    let br = b
        .task_receipt(PEER, &other(recruit()), &other(accept()), &other(task(5)))
        .unwrap();
    assert_eq!(ar.task_request_id(), br.task_request_id());
    assert_eq!(ar.output_sha256(), br.output_sha256());
    assert_ne!(
        ar.recruitment().receipt_sha256(),
        br.recruitment().receipt_sha256()
    );
    assert_ne!(ar.receipt_sha256(), br.receipt_sha256());
    assert!(a
        .task_receipt(PEER, &other(recruit()), &other(accept()), &other(task(5)))
        .is_err());
    assert!(b
        .task_receipt(PEER, &recruit(), &accept(), &task(5))
        .is_err());
}

#[test]
fn selected_receipts_cannot_be_ahead_of_the_transactional_inbox_cursor() {
    for cursor in [0, 1, 2] {
        let f = Fixture::new();
        f.db()
            .execute(
                "UPDATE fetch_meta SET value=?1 WHERE key='cursor'",
                [cursor.to_string()],
            )
            .unwrap();
        assert!(f
            .store
            .task_receipt(PEER, &recruit(), &accept(), &task(5))
            .is_err());
    }
}

#[test]
fn cached_offer_and_task_labels_cannot_replace_valid_native_links() {
    for (field, value) in [
        ("/offer/expires_at", json!(at() + Duration::hours(2))),
        (
            "/offer/body/lease_expires_at",
            json!(at() + Duration::hours(2)),
        ),
        ("/offer/body/recruit_message_id", json!(id(100))),
        ("/offer/body/capabilities", json!(["other.capability"])),
        ("/acceptance/body/offer_id", json!("other-offer")),
    ] {
        let f = Fixture::new();
        let db = f.db();
        let text: String = db
            .query_row(
                "SELECT data FROM fetch_contracts WHERE id='contract-a'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        let mut data: Value = serde_json::from_str(&text).unwrap();
        *data.pointer_mut(field).unwrap() = value;
        db.execute(
            "UPDATE fetch_contracts SET data=?1 WHERE id='contract-a'",
            [data.to_string()],
        )
        .unwrap();
        assert!(
            f.store
                .task_receipt(PEER, &recruit(), &accept(), &task(5))
                .is_err(),
            "{field}"
        );
    }
}
