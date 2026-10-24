use genesis_core::development_method::ResourceGrant;
use genesis_forge::method_accounting::CostObservation;
use genesis_forge::method_roster::GenerationRoster;
use genesis_forge::method_synthesis::{MethodSynthesisPhase, MethodSynthesisSession};
#[path = "common/method_revision_fixture.rs"]
mod fixtures;

fn prepared(id: &str) -> MethodSynthesisSession {
    let (mut spec, bytes) = fixtures::fixture();
    spec.generation.operation_id = id.into();
    spec.prepare(&bytes).unwrap()
}
fn grant() -> ResourceGrant {
    ResourceGrant {
        model_calls: 64,
        input_tokens: 100,
        output_tokens: 64 * 16_384,
        wall_time_ms: 100,
        simulation_steps: 0,
    }
}
fn roster(sessions: &[MethodSynthesisSession]) -> GenerationRoster {
    GenerationRoster::freeze(
        "experiment".into(),
        sessions[0].plan().settings.clone(),
        grant(),
        sessions,
    )
    .unwrap()
}

#[test]
fn missing_unused_rejected_and_uncertain_attempts_stay_in_the_report() {
    let sessions: Vec<_> = ["missing", "unused", "rejected", "uncertain"]
        .into_iter()
        .map(prepared)
        .collect();
    let roster = roster(&sessions);
    let original = serde_json::to_vec(&roster).unwrap();
    let mut rejected = sessions[2].clone();
    fixtures::complete(&mut rejected, "");
    assert_eq!(rejected.phase(), MethodSynthesisPhase::RejectedSource);
    let mut uncertain = sessions[3].clone();
    uncertain.begin().unwrap();
    uncertain.recover_interrupted().unwrap();
    let report = roster
        .assess(&[sessions[1].clone(), rejected, uncertain])
        .unwrap();
    assert_eq!(report.costs.rows.len(), 4);
    assert!(matches!(
        report.costs.rows[0].observation,
        CostObservation::Missing
    ));
    assert!(matches!(
        report.costs.rows[1].observation,
        CostObservation::Generation {
            phase: MethodSynthesisPhase::Prepared,
            ..
        }
    ));
    assert_eq!(report.costs.combined[0].known_lower_bound, 1);
    assert!(report.costs.combined[0].unknown);
    assert_eq!(report.roster_sha256, roster.sha256().unwrap());
    assert_eq!(serde_json::to_vec(&roster).unwrap(), original);
    assert_eq!(roster.assess(&[]).unwrap().costs.rows.len(), 4);
}

#[test]
fn declaration_rejects_every_started_or_completed_phase() {
    let session = prepared("one");
    let mut requesting = session.clone();
    requesting.begin().unwrap();
    let mut unknown = requesting.clone();
    unknown.recover_interrupted().unwrap();
    let mut generated = session.clone();
    fixtures::complete(&mut generated, "print(1)");
    let mut rejected = session.clone();
    fixtures::complete(&mut rejected, "");
    let mut failed = session.clone();
    let request = failed.begin().unwrap();
    let mut completion =
        genesis_forge::text_generation::TextCompletion::interrupted(&request).unwrap();
    completion.outcome = genesis_forge::text_generation::TextOutcome::Failed {
        reason: genesis_forge::text_generation::TextFailure::Http,
    };
    failed.complete(completion).unwrap();
    for past in [requesting, unknown, generated, rejected, failed] {
        assert!(GenerationRoster::freeze(
            "late".into(),
            session.plan().settings.clone(),
            grant(),
            &[past]
        )
        .is_err());
    }
}

#[test]
fn extra_duplicate_and_substituted_snapshots_cannot_enter_the_roster() {
    let first = prepared("first");
    let second = prepared("second");
    let declaration = roster(&[first.clone(), second]);
    assert!(declaration.assess(&[prepared("foreign")]).is_err());
    assert!(declaration.assess(&[first.clone(), first.clone()]).is_err());
    for field in ["instruction", "org", "model", "budget", "parent"] {
        let mut plan = first.plan().clone();
        match field {
            "instruction" => plan.instruction.push_str(" changed"),
            "org" => plan.org_id = "other".into(),
            "model" => plan.settings.model = "different-model".into(),
            "budget" => plan.settings.max_tokens += 1,
            _ => {
                plan.public_inputs.pop();
            }
        }
        let mut substituted = MethodSynthesisSession::new(plan).unwrap();
        fixtures::complete(&mut substituted, "print(1)");
        assert!(declaration.assess(&[substituted]).is_err(), "{field}");
    }
}

#[test]
fn identity_settings_and_declared_capacity_are_checked_before_freezing() {
    let one = prepared("one");
    let settings = one.plan().settings.clone();
    assert!(GenerationRoster::freeze("zero".into(), settings.clone(), grant(), &[]).is_err());
    assert!(GenerationRoster::freeze(
        "duplicates".into(),
        settings.clone(),
        grant(),
        &[one.clone(), one.clone()]
    )
    .is_err());
    let mut other_plan = prepared("two").plan().clone();
    other_plan.settings.timeout_ms += 1;
    let other = MethodSynthesisSession::new(other_plan).unwrap();
    assert!(GenerationRoster::freeze(
        "settings".into(),
        settings.clone(),
        grant(),
        &[one.clone(), other]
    )
    .is_err());
    for budget in [
        ResourceGrant {
            model_calls: 0,
            ..grant()
        },
        ResourceGrant {
            output_tokens: u64::from(settings.max_tokens) - 1,
            ..grant()
        },
    ] {
        assert!(GenerationRoster::freeze(
            "budget".into(),
            settings.clone(),
            budget,
            std::slice::from_ref(&one)
        )
        .is_err());
    }
    let many: Vec<_> = (0..65).map(|i| prepared(&format!("op{i}"))).collect();
    assert!(GenerationRoster::freeze("too-many".into(), settings, grant(), &many).is_err());
}

#[test]
fn roundtrip_hash_binds_order_budget_and_settings_without_source_disclosure() {
    let a = prepared("a");
    let b = prepared("b");
    let original = roster(&[a.clone(), b.clone()]);
    let bytes = serde_json::to_vec(&original).unwrap();
    assert!(!String::from_utf8(bytes.clone())
        .unwrap()
        .contains("MUST_NOT_EXECUTE"));
    let restored: GenerationRoster = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(original.sha256().unwrap(), restored.sha256().unwrap());
    assert_ne!(
        original.sha256().unwrap(),
        roster(&[b, a]).sha256().unwrap()
    );
    let mut value = serde_json::to_value(&original).unwrap();
    value["total_grant"]["input_tokens"] = 101.into();
    let changed: GenerationRoster = serde_json::from_value(value.clone()).unwrap();
    assert_ne!(original.sha256().unwrap(), changed.sha256().unwrap());
    value["schema_version"] = 2.into();
    assert!(serde_json::from_value::<GenerationRoster>(value).is_err());
    let mut value = serde_json::to_value(&original).unwrap();
    value["attempts"][0]["checkpoint_sha256"] = "not-a-digest".into();
    assert!(serde_json::from_value::<GenerationRoster>(value).is_err());
}

#[test]
fn advanced_snapshot_preserves_identity_and_measured_overruns() {
    let session = prepared("one");
    let mut declared = grant();
    declared.input_tokens = 0;
    let roster = GenerationRoster::freeze(
        "tight".into(),
        session.plan().settings.clone(),
        declared,
        std::slice::from_ref(&session),
    )
    .unwrap();
    let mut advanced = session.clone();
    let request = advanced.begin().unwrap();
    let mut completion =
        genesis_forge::text_generation::TextCompletion::interrupted(&request).unwrap();
    completion.usage.input_tokens =
        genesis_core::development_method::ResourceUsage::Measured { value: 12 };
    advanced.complete(completion).unwrap();
    let report = roster.assess(&[advanced.clone()]).unwrap();
    assert!(report.costs.combined[1].limit_exceeded);
    assert_eq!(report.costs.combined[1].known_lower_bound, 12);
    assert_eq!(
        report.costs.rows[0].evidence_sha256.as_deref(),
        Some(advanced.checkpoint_sha256())
    );
    assert_ne!(session.checkpoint_sha256(), advanced.checkpoint_sha256());
}

#[test]
fn pending_and_failed_snapshots_preserve_declared_order_and_initial_identity() {
    use genesis_forge::text_generation::{TextCompletion, TextFailure, TextOutcome};
    let first = prepared("first");
    let second = prepared("second");
    let roster = roster(&[first.clone(), second.clone()]);
    let mut pending = first.clone();
    pending.begin().unwrap();
    let mut failed = second.clone();
    let request = failed.begin().unwrap();
    let mut completion = TextCompletion::interrupted(&request).unwrap();
    completion.outcome = TextOutcome::Failed {
        reason: TextFailure::Http,
    };
    completion.usage.input_tokens =
        genesis_core::development_method::ResourceUsage::Measured { value: 8 };
    failed.complete(completion).unwrap();
    let before = serde_json::to_vec(&failed).unwrap();
    assert_eq!(
        failed.prepared_checkpoint_sha256().unwrap(),
        second.checkpoint_sha256()
    );
    assert_eq!(serde_json::to_vec(&failed).unwrap(), before);
    let forward = roster.assess(&[pending.clone(), failed.clone()]).unwrap();
    let reversed = roster.assess(&[failed.clone(), pending]).unwrap();
    assert_eq!(
        serde_json::to_value(forward).unwrap(),
        serde_json::to_value(reversed).unwrap()
    );
    let mut substituted = serde_json::to_value(&roster).unwrap();
    substituted["attempts"][1]["checkpoint_sha256"] = "a".repeat(64).into();
    let substituted: GenerationRoster = serde_json::from_value(substituted).unwrap();
    assert!(substituted.assess(&[failed]).is_err());
}
