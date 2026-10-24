use super::*;
use chrono::Duration;
use genesis_core::ids::OrgId;
use serde_json::{json, Value};

fn instant() -> DateTime<Utc> {
    DateTime::parse_from_rfc3339("2026-10-21T12:00:00Z")
        .unwrap()
        .with_timezone(&Utc)
}

fn snapshot(id: &str) -> Snapshot {
    Snapshot {
        id: id.into(),
        org_id: "org_journal".into(),
        mission_id: "mission_one".into(),
        requirement_id: "requirement_one".into(),
        role_id: "role_one".into(),
        responsibility_id: "responsibility_one".into(),
        assignment_id: "assignment_one".into(),
        epoch: 1,
        version: 1,
        created_at: instant(),
        updated_at: instant(),
        phase: AcquisitionPhase::Generating,
        generation_phase: SynthesisPhase::Prepared,
        forge_phase: None,
        attempt: 0,
        pending_action: None,
        completed_case_count: 0,
        fence_reason: None,
        active: true,
    }
}

fn store() -> Store {
    let store = Store::open_in_memory(OrgId::new("org_journal")).unwrap();
    // Minimal indexed table: tests deliberately do not need or store source.
    store
        .lock()
        .execute_batch(
            "CREATE TABLE capability_acquisitions(id TEXT PRIMARY KEY,version INTEGER NOT NULL);",
        )
        .unwrap();
    store.init_acquisition_history().unwrap();
    store
}

fn write(store: &Store, snapshot: &Snapshot, kind: CheckpointKind) {
    let mut conn = store.lock();
    let tx = conn.transaction().unwrap();
    tx.execute(
        "INSERT INTO capability_acquisitions(id,version) VALUES(?1,?2)
         ON CONFLICT(id) DO UPDATE SET version=excluded.version",
        params![snapshot.id, snapshot.version as i64],
    )
    .unwrap();
    append_snapshot(&tx, snapshot.clone(), 4, kind, None, snapshot.updated_at).unwrap();
    tx.commit().unwrap();
}

fn attention(store: &Store, id: &str, version: u64, error: Option<DriverErrorCode>) {
    let mut conn = store.lock();
    let tx = conn.transaction().unwrap();
    append_attention(&tx, id, version, error, instant() + Duration::seconds(10)).unwrap();
    tx.commit().unwrap();
}

#[test]
fn readiness_blocks_effect_journaling_until_an_honest_baseline_is_observed() {
    let store = store();
    let mut old = snapshot("acq_legacy");
    old.version = 17;
    old.updated_at += Duration::seconds(60);
    store
        .lock()
        .execute(
            "INSERT INTO capability_acquisitions(id,version) VALUES(?1,?2)",
            params![old.id, old.version as i64],
        )
        .unwrap();
    store.init_acquisition_history().unwrap();
    assert!(matches!(
        store.list_acquisitions(&ListQuery::default()),
        Err(HistoryError::Unavailable)
    ));
    let mut conn = store.lock();
    let tx = conn.transaction().unwrap();
    assert!(matches!(
        append_snapshot(
            &tx,
            old.clone(),
            4,
            CheckpointKind::Transition,
            None,
            old.updated_at
        ),
        Err(HistoryError::Unavailable)
    ));
    // The real observation time can precede a stored update after clock rollback.
    append_snapshot(
        &tx,
        old.clone(),
        4,
        CheckpointKind::BaselineObserved,
        None,
        instant(),
    )
    .unwrap();
    append_snapshot(
        &tx,
        old.clone(),
        4,
        CheckpointKind::BaselineObserved,
        None,
        instant(),
    )
    .unwrap();
    mark_ready(&tx).unwrap();
    tx.commit().unwrap();
    drop(conn);
    let history = store
        .acquisition_history(&old.id, &PageQuery::default())
        .unwrap();
    assert_eq!(history.items.len(), 1);
    let entry = &history.items[0];
    assert_eq!(entry.kind, HistoryKind::BaselineObserved);
    assert_eq!(entry.recorded_at, instant());
    assert_eq!(entry.snapshot.updated_at, old.updated_at);
    assert_eq!(entry.snapshot.version, 17);
    assert_eq!(entry.checkpoint_org_revision, 4);
    assert!(entry.debit.is_none());
}

#[test]
fn attention_is_version_bound_deduplicated_and_explicitly_cleared() {
    let store = store();
    let initial = snapshot("acq_attention");
    write(&store, &initial, CheckpointKind::Created);
    attention(&store, &initial.id, 1, None);
    attention(&store, &initial.id, 1, Some(DriverErrorCode::Quota));
    attention(&store, &initial.id, 1, Some(DriverErrorCode::Quota));
    let list = store.list_acquisitions(&ListQuery::default()).unwrap();
    assert_eq!(list.items[0].attention, Some(DriverErrorCode::Quota));
    let mut conn = store.lock();
    let tx = conn.transaction().unwrap();
    assert!(matches!(
        append_attention(
            &tx,
            &initial.id,
            2,
            Some(DriverErrorCode::Internal),
            instant() + Duration::seconds(10)
        ),
        Err(HistoryError::InvalidRecord)
    ));
    assert!(matches!(
        append_attention(&tx, "acq_missing", 1, None, instant()),
        Err(HistoryError::InvalidRecord)
    ));
    tx.rollback().unwrap();
    drop(conn);
    attention(&store, &initial.id, 1, Some(DriverErrorCode::RetryRead));
    attention(&store, &initial.id, 1, None);
    attention(&store, &initial.id, 1, None);
    let history = store
        .acquisition_history(&initial.id, &PageQuery::default())
        .unwrap();
    assert_eq!(
        history
            .items
            .iter()
            .map(|entry| entry.kind)
            .collect::<Vec<_>>(),
        vec![
            HistoryKind::Created,
            HistoryKind::Attention,
            HistoryKind::Attention,
            HistoryKind::AttentionCleared
        ]
    );
    assert_eq!(history.items[1].attention, Some(DriverErrorCode::Quota));
    assert_eq!(history.items[2].attention, Some(DriverErrorCode::RetryRead));
    assert_eq!(history.items[3].attention, None);
    assert!(history
        .items
        .iter()
        .all(|entry| entry.snapshot.version == 1 && entry.checkpoint_org_revision == 4));
    assert_eq!(
        store
            .list_acquisitions(&ListQuery::default())
            .unwrap()
            .items[0]
            .attention,
        None
    );
}

#[test]
fn listing_uses_immutable_ordinals_and_history_uses_global_exclusive_sequences() {
    let store = store();
    let mut first = snapshot("acq_z_first");
    let mut second = snapshot("acq_a_second");
    second.requirement_id = "requirement_two".into();
    write(&store, &first, CheckpointKind::Created);
    write(&store, &second, CheckpointKind::Created);
    let first_page = store
        .list_acquisitions(&ListQuery {
            limit: 1,
            ..Default::default()
        })
        .unwrap();
    assert_eq!(first_page.items[0].snapshot.id, first.id);
    let cursor = first_page.next_after.unwrap();
    first.version = 2;
    first.updated_at += Duration::seconds(1);
    write(&store, &first, CheckpointKind::Transition);
    let second_page = store
        .list_acquisitions(&ListQuery {
            after: cursor,
            limit: 1,
            ..Default::default()
        })
        .unwrap();
    assert_eq!(second_page.items[0].snapshot.id, second.id);
    assert!(second_page.next_after.is_none());
    let all = store.list_acquisitions(&ListQuery::default()).unwrap();
    assert_eq!(all.items[0].ordinal, cursor);
    assert_eq!(all.items[0].snapshot.version, 2);
    let filtered = store
        .list_acquisitions(&ListQuery {
            mission_id: Some("mission_one".into()),
            requirement_id: Some("requirement_two".into()),
            ..Default::default()
        })
        .unwrap();
    assert_eq!(filtered.items.len(), 1);
    assert_eq!(filtered.items[0].snapshot.id, second.id);
    assert!(store
        .list_acquisitions(&ListQuery {
            mission_id: Some("mission_other".into()),
            requirement_id: Some("requirement_two".into()),
            ..Default::default()
        })
        .unwrap()
        .items
        .is_empty());
    let history = store
        .acquisition_history(&first.id, &PageQuery { after: 0, limit: 1 })
        .unwrap();
    let after = history.next_after.unwrap();
    assert_eq!(after, history.items[0].seq);
    let continued = store
        .acquisition_history(&first.id, &PageQuery { after, limit: 1 })
        .unwrap();
    assert_eq!(continued.items.len(), 1);
    assert_eq!(continued.items[0].snapshot.version, 2);
    assert!(
        continued.items[0].seq > after + 1,
        "another acquisition used the intervening global sequence"
    );
    assert!(continued.next_after.is_none());
}

#[test]
fn mutations_and_journal_rows_rollback_together() {
    let store = store();
    let initial = snapshot("acq_rollback");
    let mut conn = store.lock();
    let tx = conn.transaction().unwrap();
    tx.execute(
        "INSERT INTO capability_acquisitions(id,version) VALUES(?1,1)",
        params![initial.id],
    )
    .unwrap();
    append_snapshot(
        &tx,
        initial.clone(),
        4,
        CheckpointKind::Created,
        None,
        instant(),
    )
    .unwrap();
    tx.rollback().unwrap();
    assert_eq!(
        conn.query_row("SELECT COUNT(*) FROM capability_acquisitions", [], |row| {
            row.get::<_, u64>(0)
        })
        .unwrap(),
        0
    );
    assert_eq!(
        conn.query_row("SELECT COUNT(*) FROM acquisition_catalog", [], |row| row
            .get::<_, u64>(0))
            .unwrap(),
        0
    );
    assert_eq!(
        conn.query_row("SELECT COUNT(*) FROM acquisition_history", [], |row| row
            .get::<_, u64>(0))
            .unwrap(),
        0
    );
    drop(conn);
    write(&store, &initial, CheckpointKind::Created);
    let mut conn = store.lock();
    let tx = conn.transaction().unwrap();
    assert!(matches!(
        append_snapshot(
            &tx,
            initial.clone(),
            4,
            CheckpointKind::Created,
            None,
            instant()
        ),
        Err(HistoryError::InvalidRecord)
    ));
    assert!(matches!(
        append_snapshot(
            &tx,
            initial.clone(),
            4,
            CheckpointKind::Transition,
            None,
            instant()
        ),
        Err(HistoryError::InvalidRecord)
    ));
    let mut changed = initial.clone();
    changed.role_id = "different_role".into();
    assert!(matches!(
        append_snapshot(
            &tx,
            changed,
            4,
            CheckpointKind::BaselineObserved,
            None,
            instant()
        ),
        Err(HistoryError::InvalidRecord)
    ));
}

#[test]
fn query_bounds_and_unknown_fields_are_rejected_without_reading_records() {
    let store = Store::open_in_memory(OrgId::new("org_journal")).unwrap();
    for query in [
        PageQuery {
            after: u64::MAX,
            limit: 1,
        },
        PageQuery { after: 0, limit: 0 },
        PageQuery {
            after: 0,
            limit: 101,
        },
    ] {
        assert!(matches!(
            store.acquisition_history("acq_one", &query),
            Err(HistoryError::InvalidQuery)
        ));
    }
    assert!(matches!(
        store.list_acquisitions(&ListQuery {
            mission_id: Some("bad/id".into()),
            ..Default::default()
        }),
        Err(HistoryError::InvalidQuery)
    ));
    assert!(matches!(
        store.acquisition_history("../private", &PageQuery::default()),
        Err(HistoryError::InvalidQuery)
    ));
    for value in [
        json!({"after":0,"unknown":true}),
        json!({"after":-1}),
        json!({"limit":"25"}),
    ] {
        assert!(serde_json::from_value::<PageQuery>(value).is_err());
    }
    assert!(matches!(
        store.list_acquisitions(&ListQuery::default()),
        Err(HistoryError::Unavailable)
    ));
    store.init_acquisition_history().unwrap();
    assert!(store
        .list_acquisitions(&ListQuery::default())
        .unwrap()
        .items
        .is_empty());
    assert!(matches!(
        store.acquisition_history("acq_unknown", &PageQuery::default()),
        Err(HistoryError::NotFound)
    ));
}

#[test]
fn corrupt_oversized_or_misbound_catalog_rows_fail_closed() {
    for mutation in [
        "UPDATE acquisition_catalog SET snapshot_json=printf('%.*c',1000000,'x')",
        "UPDATE acquisition_catalog SET snapshot_json=json_set(snapshot_json,'$.phase','provider_canary')",
        "UPDATE acquisition_catalog SET snapshot_json=json_set(snapshot_json,'$.source','private_canary')",
        "UPDATE acquisition_catalog SET last_seq=99999",
        "UPDATE acquisition_catalog SET mission_id='wrong_mission'",
        "UPDATE capability_acquisitions SET version=2",
        "UPDATE acquisition_catalog SET attention_version=1,attention_code='\"unknown_canary\"'",
    ] {
        let store=store();
        write(&store,&snapshot("acq_corrupt"),CheckpointKind::Created);
        store.lock().execute(mutation,[]).unwrap();
        let error=store.list_acquisitions(&ListQuery::default()).unwrap_err();
        assert!(matches!(error,HistoryError::InvalidRecord),"{mutation}: {error}");
        assert!(!error.to_string().contains("canary"));
    }
}

#[test]
fn corrupt_oversized_or_wrongly_indexed_history_rows_fail_closed() {
    for mutation in [
        "UPDATE acquisition_history SET payload_json=printf('%.*c',1000000,'x')",
        "UPDATE acquisition_history SET payload_json=json_set(payload_json,'$.snapshot.role_id','another_role')",
        "UPDATE acquisition_history SET payload_json=json_set(payload_json,'$.snapshot.version',2)",
        "UPDATE acquisition_history SET payload_json=json_set(payload_json,'$.provider_body','private_canary')",
        "UPDATE acquisition_history SET event_kind='attention'",
        "DELETE FROM acquisition_history",
    ] {
        let store=store();
        write(&store,&snapshot("acq_corrupt"),CheckpointKind::Created);
        store.lock().execute(mutation,[]).unwrap();
        let error=store.acquisition_history("acq_corrupt",&PageQuery::default()).unwrap_err();
        assert!(matches!(error,HistoryError::InvalidRecord),"{mutation}: {error}");
        assert!(!error.to_string().contains("canary"));
    }
}

#[test]
fn projection_validation_does_not_accept_unbounded_or_impossible_metadata() {
    let initial = snapshot("acq_small");
    let encoded = serde_json::to_value(&initial).unwrap();
    for (key, value) in [
        ("id", json!("x".repeat(257))),
        ("version", json!(0)),
        ("version", json!(u64::MAX)),
        ("attempt", json!(2)),
        ("completed_case_count", json!(1)),
        ("pending_action", json!("create_actor")),
        ("fence_reason", json!("lease_expired")),
    ] {
        let mut changed: Value = encoded.clone();
        changed[key] = value;
        let snapshot: Snapshot = serde_json::from_value(changed).unwrap();
        assert!(
            matches!(snapshot.validate(), Err(HistoryError::InvalidRecord)),
            "{key}"
        );
    }
}
