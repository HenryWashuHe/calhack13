use genesis_core::development_method::ResourceGrant;
use genesis_forge::{
    method_admission::GenerationAdmission,
    method_roster::GenerationRoster,
    method_synthesis::MethodSynthesisSession,
    text_generation::{TextCompletion, TextFailure, TextOutcome},
};
use serde_json::json;
#[path = "common/method_revision_fixture.rs"]
mod fixture;

fn sessions(count: usize) -> Vec<MethodSynthesisSession> {
    let (spec, parent) = fixture::fixture();
    (0..count)
        .map(|i| {
            let mut spec = spec.clone();
            spec.generation.operation_id = format!("attempt-{i}");
            spec.prepare(&parent).unwrap()
        })
        .collect()
}
fn admission(sessions: &[MethodSynthesisSession]) -> GenerationAdmission {
    let settings = sessions[0].plan().settings.clone();
    let grant = ResourceGrant {
        model_calls: sessions.len() as u64,
        input_tokens: 1,
        output_tokens: sessions.len() as u64 * u64::from(settings.max_tokens),
        wall_time_ms: 1,
        simulation_steps: 0,
    };
    GenerationAdmission::new(
        GenerationRoster::freeze("declared".into(), settings, grant, sessions).unwrap(),
    )
    .unwrap()
}

#[test]
fn exact_attempts_are_permanently_reserved_across_restore_and_outcomes() {
    let sessions = sessions(2);
    let mut journal = admission(&sessions);
    let before = serde_json::to_vec(&sessions).unwrap();
    let initial = journal.summary().unwrap();
    assert_eq!(initial.reserved_attempts, 0);
    journal.reserve(&sessions[0]).unwrap();
    assert!(journal.is_reserved(&sessions[0]).unwrap());
    assert!(!journal.is_reserved(&sessions[1]).unwrap());
    assert_ne!(
        initial.journal_sha256,
        journal.summary().unwrap().journal_sha256
    );
    assert_eq!(
        initial.roster_sha256,
        journal.summary().unwrap().roster_sha256
    );
    let mut restored: GenerationAdmission =
        serde_json::from_slice(&serde_json::to_vec(&journal).unwrap()).unwrap();
    let reserved = serde_json::to_vec(&restored).unwrap();
    assert!(restored.reserve(&sessions[0].clone()).is_err());
    let mut generated = sessions[0].clone();
    fixture::complete(&mut generated, fixture::SOURCE);
    let mut rejected = sessions[0].clone();
    fixture::complete(&mut rejected, "");
    let mut pending = sessions[0].clone();
    let request = pending.begin().unwrap();
    let mut failed = pending.clone();
    let mut completion = TextCompletion::interrupted(&request).unwrap();
    completion.outcome = TextOutcome::Failed {
        reason: TextFailure::Http,
    };
    failed.complete(completion).unwrap();
    let mut uncertain = pending.clone();
    uncertain.recover_interrupted().unwrap();
    for snapshot in [generated, rejected, pending, failed, uncertain] {
        assert!(restored.is_reserved(&snapshot).unwrap());
        assert!(restored.reserve(&snapshot).is_err());
        assert_eq!(serde_json::to_vec(&restored).unwrap(), reserved);
    }
    restored.reserve(&sessions[1]).unwrap();
    let summary = restored.summary().unwrap();
    assert_eq!(summary.remaining_attempts, 0);
    assert_eq!(summary.reserved_attempts, 2);
    assert_eq!(
        summary.reserved_output_token_caps,
        2 * u64::from(sessions[0].plan().settings.max_tokens)
    );
    assert_eq!(serde_json::to_vec(&sessions).unwrap(), before);
}

#[test]
fn foreign_substituted_and_already_started_attempts_do_not_consume_capacity() {
    let sessions = sessions(1);
    let mut journal = admission(&sessions);
    let before = serde_json::to_vec(&journal).unwrap();
    for field in ["instruction", "model", "operation", "org"] {
        let mut plan = sessions[0].plan().clone();
        match field {
            "instruction" => plan.instruction.push_str(" changed"),
            "model" => plan.settings.model = "different-model".into(),
            "operation" => plan.operation_id = "different-operation".into(),
            _ => plan.org_id = "different-org".into(),
        }
        let substituted = MethodSynthesisSession::new(plan).unwrap();
        assert!(journal.reserve(&substituted).is_err());
        assert!(journal.is_reserved(&substituted).is_err());
        assert_eq!(serde_json::to_vec(&journal).unwrap(), before);
    }
    let mut pending = sessions[0].clone();
    pending.begin().unwrap();
    assert!(!journal.is_reserved(&pending).unwrap());
    assert!(journal.reserve(&pending).is_err());
    assert_eq!(serde_json::to_vec(&journal).unwrap(), before);
}

#[test]
fn restore_refuses_unbound_duplicate_foreign_and_unsupported_reservations() {
    let sessions = sessions(2);
    let mut journal = admission(&sessions);
    journal.reserve(&sessions[0]).unwrap();
    let valid = serde_json::to_value(journal).unwrap();
    for (field, value) in [
        ("schema_version", json!(2)),
        ("extra", json!(true)),
        ("reserved_prepared_checkpoints", json!(["f".repeat(64)])),
        (
            "reserved_prepared_checkpoints",
            json!([
                sessions[0].checkpoint_sha256(),
                sessions[0].checkpoint_sha256()
            ]),
        ),
    ] {
        let mut invalid = valid.clone();
        invalid[field] = value;
        assert!(serde_json::from_value::<GenerationAdmission>(invalid).is_err());
    }
    let mut substituted_roster = valid;
    substituted_roster["roster"]["attempts"][0]["checkpoint_sha256"] = json!("e".repeat(64));
    assert!(serde_json::from_value::<GenerationAdmission>(substituted_roster).is_err());
}

#[test]
fn full_roster_capacity_is_bounded_and_source_free() {
    let sessions = sessions(64);
    let mut journal = admission(&sessions);
    for session in sessions.iter().rev() {
        journal.reserve(session).unwrap();
    }
    let summary = journal.summary().unwrap();
    assert_eq!(summary.reserved_attempts, 64);
    assert_eq!(summary.remaining_attempts, 0);
    assert_eq!(
        summary.reserved_output_token_caps,
        64 * u64::from(sessions[0].plan().settings.max_tokens)
    );
    let saved = serde_json::to_string(&journal).unwrap();
    assert!(!saved.contains("MUST_NOT_EXECUTE"));
    assert!(!saved.contains("instruction"));
    let restored: GenerationAdmission = serde_json::from_str(&saved).unwrap();
    assert_eq!(
        summary.journal_sha256,
        restored.summary().unwrap().journal_sha256
    );
}
