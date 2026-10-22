use super::*;
use crate::capability_acquisition_store::{AcquisitionPolicy, AcquisitionQuotas};
use crate::{build_org, AcquisitionConfig, OrgConfig};
use genesis_apify::{ApifyClient, RunLimits};
use genesis_core::ids::OrgId;
use genesis_forge::{AnthropicGenerator, ModelConfig};
use serde_json::json;
use std::sync::Arc;

const PRIVATE_RECORD: &str = "PRIVATE_ORACLE_SOURCE_PROVIDER_TOKEN_NEVER_IN_AUTOMATION_STATUS";

fn now() -> DateTime<Utc> {
    "2026-10-21T12:00:00Z".parse().unwrap()
}

fn policy() -> AcquisitionPolicy {
    AcquisitionPolicy {
        model: ModelConfig {
            model: "scheduler-fixture-model".into(),
            max_tokens: 1024,
        },
        quotas: AcquisitionQuotas::default(),
        max_duration_seconds: 3600,
    }
}

fn init(store: &Store) {
    store.init_acquisitions(&policy()).unwrap();
    store.init_acquisition_driver().unwrap();
}

fn store() -> Store {
    let store = Store::open_in_memory(OrgId::new("scheduler-tests")).unwrap();
    init(&store);
    store
}

// Metadata-only scheduling fixtures, deliberately NOT valid acquisition
// checkpoints or proofs. No runtime step may deserialize or execute these rows.
fn seed(store: &Store, id: &str, version: u64, active: bool) {
    store.lock().execute(
        "INSERT INTO capability_acquisitions(id,client_request_id,requirement_id,assignment_id,epoch,suite_id,version,active,record_json)
         VALUES (?1,?1,?1,?1,1,'metadata-only-suite',?2,?3,?4)",
        params![id, version as i64, i64::from(active), json!({"private":PRIVATE_RECORD}).to_string()],
    ).unwrap();
}

fn change_version(store: &Store, id: &str, version: u64) {
    assert_eq!(
        store
            .lock()
            .execute(
                "UPDATE capability_acquisitions SET version=?2 WHERE id=?1",
                params![id, version as i64],
            )
            .unwrap(),
        1
    );
}

fn ids(store: &Store, time: DateTime<Utc>) -> Vec<String> {
    candidates(store, time)
        .unwrap()
        .into_iter()
        .map(|candidate| candidate.id)
        .collect()
}

#[test]
fn selection_is_bounded_and_services_unseen_then_oldest_acquisitions() {
    let store = store();
    seed(&store, "00-inactive", 1, false);
    for index in 0..9 {
        seed(&store, &format!("intent-{index}"), 1, true);
    }
    let first = candidates(&store, now()).unwrap();
    assert_eq!(first.len(), ACQUISITION_BATCH_LIMIT);
    assert_eq!(
        ids(&store, now()),
        ["intent-0", "intent-1", "intent-2", "intent-3"]
    );
    for candidate in first {
        assert!(save_schedule(&store, &candidate.id, candidate.version, None, now()).unwrap());
    }
    let second_at = now() + ChronoDuration::seconds(1);
    let second = candidates(&store, second_at).unwrap();
    assert_eq!(
        ids(&store, second_at),
        ["intent-4", "intent-5", "intent-6", "intent-7"]
    );
    for candidate in second {
        assert!(save_schedule(&store, &candidate.id, candidate.version, None, second_at).unwrap());
    }
    // Even though the first four are due again, every unseen intent gets its
    // turn; afterwards the least recently serviced rows lead the next batch.
    assert_eq!(
        ids(&store, now() + ChronoDuration::seconds(2)),
        ["intent-8", "intent-0", "intent-1", "intent-2"]
    );
}

#[test]
fn ready_and_paused_schedules_have_exact_one_and_five_second_due_boundaries() {
    let store = store();
    for (id, error) in [
        ("ready", None),
        ("retry-read", Some(DriverErrorCode::RetryRead)),
        ("quota", Some(DriverErrorCode::Quota)),
    ] {
        seed(&store, id, 1, true);
        assert!(save_schedule(&store, id, 1, error, now()).unwrap());
        let saved = schedule(&store.lock(), id).unwrap().unwrap();
        assert_eq!(saved.error, error);
        assert_eq!(
            saved.next_check_at,
            Some(now() + ChronoDuration::seconds(if error.is_some() { 5 } else { 1 }))
        );
    }
    assert!(ids(&store, now() + ChronoDuration::milliseconds(999)).is_empty());
    assert_eq!(ids(&store, now() + ChronoDuration::seconds(1)), ["ready"]);
    assert_eq!(
        ids(&store, now() + ChronoDuration::milliseconds(4999)),
        ["ready"]
    );
    assert_eq!(
        ids(&store, now() + ChronoDuration::seconds(5)),
        ["quota", "ready", "retry-read"]
    );
}

#[test]
fn permanent_attention_survives_reopen_but_a_new_version_is_eligible() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("scheduler.sqlite");
    let store = Store::open(&path, OrgId::new("scheduler-tests")).unwrap();
    init(&store);
    for (id, error) in [
        ("configuration", DriverErrorCode::Configuration),
        ("invalid-record", DriverErrorCode::InvalidRecord),
        ("conflict", DriverErrorCode::Conflict),
        ("internal", DriverErrorCode::Internal),
    ] {
        seed(&store, id, 1, true);
        save_schedule(&store, id, 1, Some(error), now()).unwrap();
    }
    assert!(ids(&store, now() + ChronoDuration::days(365)).is_empty());
    drop(store);
    let reopened = Store::open(&path, OrgId::new("scheduler-tests")).unwrap();
    init(&reopened);
    assert!(ids(&reopened, now() + ChronoDuration::days(365)).is_empty());
    assert_eq!(
        schedule(&reopened.lock(), "configuration")
            .unwrap()
            .unwrap()
            .error,
        Some(DriverErrorCode::Configuration)
    );
    change_version(&reopened, "configuration", 2);
    let selected = candidates(&reopened, now()).unwrap();
    assert_eq!(selected.len(), 1);
    assert_eq!(selected[0].id, "configuration");
    assert_eq!(selected[0].version, 2);
    assert_eq!(
        selected[0].error, None,
        "old attention must not follow a new manual version"
    );
    assert!(save_schedule(&reopened, "configuration", 2, None, now()).unwrap());
    let saved = schedule(&reopened.lock(), "configuration")
        .unwrap()
        .unwrap();
    assert_eq!((saved.version, saved.error), (2, None));
}

#[test]
fn stale_schedule_cannot_attach_an_error_to_a_newer_manual_version() {
    let store = store();
    seed(&store, "with-schedule", 1, true);
    save_schedule(&store, "with-schedule", 1, None, now()).unwrap();
    change_version(&store, "with-schedule", 2);
    assert!(!save_schedule(
        &store,
        "with-schedule",
        1,
        Some(DriverErrorCode::InvalidRecord),
        now() + ChronoDuration::seconds(10)
    )
    .unwrap());
    let saved = schedule(&store.lock(), "with-schedule").unwrap().unwrap();
    assert_eq!(
        (saved.version, saved.error, saved.serviced_at),
        (1, None, now())
    );
    let selected = candidates(&store, now()).unwrap();
    assert_eq!((selected[0].version, selected[0].error), (2, None));

    seed(&store, "without-schedule", 2, true);
    assert!(!save_schedule(
        &store,
        "without-schedule",
        1,
        Some(DriverErrorCode::Quota),
        now()
    )
    .unwrap());
    assert!(schedule(&store.lock(), "without-schedule")
        .unwrap()
        .is_none());
}

#[test]
fn failed_schedule_transaction_preserves_the_previous_schedule() {
    let store = store();
    seed(&store, "rollback", 1, true);
    save_schedule(
        &store,
        "rollback",
        1,
        Some(DriverErrorCode::RetryRead),
        now(),
    )
    .unwrap();
    let before = schedule(&store.lock(), "rollback").unwrap().unwrap();
    store
        .lock()
        .execute_batch(
            "CREATE TRIGGER reject_schedule_update AFTER UPDATE ON acquisition_automation
         BEGIN SELECT RAISE(ABORT,'synthetic scheduler storage fault'); END;",
        )
        .unwrap();
    assert!(save_schedule(
        &store,
        "rollback",
        1,
        Some(DriverErrorCode::InvalidRecord),
        now() + ChronoDuration::seconds(10)
    )
    .is_err());
    let after = schedule(&store.lock(), "rollback").unwrap().unwrap();
    assert_eq!(
        (
            after.version,
            after.serviced_at,
            after.next_check_at,
            after.error
        ),
        (
            before.version,
            before.serviced_at,
            before.next_check_at,
            before.error
        )
    );
    store
        .lock()
        .execute_batch("DROP TRIGGER reject_schedule_update")
        .unwrap();
    assert!(save_schedule(&store, "rollback", 1, None, now()).unwrap());
}

#[test]
fn operational_status_is_redacted_and_does_not_turn_metadata_into_a_proof() {
    let mut config = OrgConfig::dev("scheduler-status", "Scheduler status");
    config.acquisition = Some(AcquisitionConfig::offline(policy()));
    let mut app = build_org(&config, None).unwrap();
    seed(&app.store, "intent", 1, true);
    assert_eq!(
        status(&app, "intent").unwrap().state,
        AutomationState::Disabled
    );
    app.automatic_acquisition = true;
    assert_eq!(
        status(&app, "intent").unwrap().state,
        AutomationState::Ready
    );
    save_schedule(
        &app.store,
        "intent",
        1,
        None,
        Utc::now() + ChronoDuration::days(1),
    )
    .unwrap();
    let waiting = status(&app, "intent").unwrap();
    assert_eq!(waiting.state, AutomationState::Waiting);
    assert!(waiting.next_attempt_at.is_some());
    assert!(waiting.next_reconcile_at.is_none());
    save_schedule(&app.store, "intent", 1, Some(DriverErrorCode::Quota), now()).unwrap();
    let paused = status(&app, "intent").unwrap();
    assert_eq!(paused.state, AutomationState::Attention);
    assert_eq!(paused.error_code, Some(DriverErrorCode::Quota));
    assert!(paused.next_attempt_at.is_none());
    assert_eq!(
        paused.next_reconcile_at,
        Some(now() + ChronoDuration::seconds(5))
    );
    let value = serde_json::to_value(paused).unwrap();
    assert_eq!(value.as_object().unwrap().len(), 9);
    for field in [
        "proof_ref",
        "verified",
        "phase",
        "source",
        "parser",
        "oracle",
        "record_json",
        "output",
    ] {
        assert!(value.get(field).is_none());
    }
    assert!(!value.to_string().contains(PRIVATE_RECORD));
    // The seed cannot authenticate any backend record, even while the purely
    // operational endpoint successfully describes its version and schedule.
    assert!(app.acquisition_runtime.status("intent").is_err());
    change_version(&app.store, "intent", 2);
    let reset = status(&app, "intent").unwrap();
    assert_eq!(reset.state, AutomationState::Ready);
    assert_eq!(reset.observed_version, Some(1));
    assert_eq!(reset.error_code, None);
    assert!(reset.next_reconcile_at.is_none());
    app.store
        .lock()
        .execute(
            "UPDATE capability_acquisitions SET active=0 WHERE id='intent'",
            [],
        )
        .unwrap();
    assert_eq!(
        status(&app, "intent").unwrap().state,
        AutomationState::Inactive
    );
}

#[tokio::test]
async fn default_off_drain_does_not_even_require_acquisition_tables_or_clients() {
    let config = OrgConfig::dev("scheduler-disabled", "Disabled");
    assert!(config.acquisition.is_none());
    let app = build_org(&config, None).unwrap();
    assert!(!app.automatic_acquisition);
    let has_acquisition_table: bool = app
        .store
        .lock()
        .query_row(
            "SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE name='capability_acquisitions')",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert!(!has_acquisition_table);
    assert_eq!(drain_once(&app).await.unwrap(), DrainSummary::default());
    assert!(app.store.commitments().unwrap().is_empty());
}

#[test]
fn automatic_configuration_requires_both_clients_before_creating_a_database() {
    let directory = tempfile::tempdir().unwrap();
    for (index, has_model, has_apify) in [(0, false, false), (1, true, false), (2, false, true)] {
        let path = directory
            .path()
            .join(format!("must-not-exist-{index}.sqlite"));
        let mut config = OrgConfig::dev("scheduler-missing-client", "Missing client");
        config.db = Some(path.clone());
        let mut acquisition = AcquisitionConfig::offline(policy());
        assert!(!acquisition.automatic);
        acquisition.automatic = true;
        if has_model {
            acquisition.model = Some(Arc::new(
                AnthropicGenerator::new("synthetic-private-model-key").unwrap(),
            ));
        }
        if has_apify {
            acquisition.apify = Some(Arc::new(
                ApifyClient::new("synthetic-private-apify-key", RunLimits::default()).unwrap(),
            ));
        }
        config.acquisition = Some(acquisition);
        let error = build_org(&config, None)
            .err()
            .expect("automatic acquisition requires both clients");
        assert!(error
            .to_string()
            .contains("requires configured model and Apify clients"));
        assert!(!error.to_string().contains("synthetic-private"));
        assert!(
            !path.exists(),
            "invalid automatic configuration created its database"
        );
    }
}
