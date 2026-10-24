use genesis_forge::{
    AcquisitionError, AcquisitionPhase, AcquisitionPlan, AcquisitionSession, CandidateIdentity,
    ModelConfig, PublicExample, RepairFeedback, RepairIdentity, SynthesisError, SynthesisPhase,
    SynthesisRepairContext, SynthesisRequest, SynthesisSession,
};
use genesis_verification::{decode_json, json_sha256, OracleSuite};
use serde_json::json;
use std::collections::BTreeSet;

fn suite() -> OracleSuite {
    decode_json(include_bytes!(
        "../../genesis-verification/examples/fixtures/trusted/suite.json"
    ))
    .unwrap()
}
fn plan() -> AcquisitionPlan {
    let suite = suite();
    let sources: Vec<String> = suite
        .cases
        .iter()
        .map(|c| c.source.url.clone())
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect();
    AcquisitionPlan {
        request: SynthesisRequest {
            operation_id: "acquisition-original".into(),
            candidate: CandidateIdentity {
                capability_id: "venue-parser".into(),
                capability_version: "1".into(),
                actor_name: "venue-original".into(),
                source_version: "1.0".into(),
            },
            requirement: "Extract public venue capacity and availability".into(),
            examples: vec![PublicExample {
                url: sources[0].clone(),
                html: "<h1>Public venue example</h1>".into(),
            }],
            allowed_source_urls: sources,
        },
        model: ModelConfig {
            model: "claude-sonnet-4-6".into(),
            max_tokens: 1024,
        },
        repair: Some(RepairIdentity {
            operation_id: "acquisition-repair".into(),
            candidate: CandidateIdentity {
                capability_id: "venue-parser".into(),
                capability_version: "2".into(),
                actor_name: "venue-repair".into(),
                source_version: "1.0".into(),
            },
        }),
    }
}
fn session() -> AcquisitionSession {
    AcquisitionSession::new(plan(), &suite()).unwrap()
}
fn serialized(session: &AcquisitionSession) -> Vec<u8> {
    serde_json::to_vec(session).unwrap()
}

#[test]
fn validates_repair_identity_and_oracle_source_contract_before_generation() {
    let edits: Vec<fn(&mut AcquisitionPlan)> = vec![
        |p| p.request.operation_id.clear(),
        |p| p.repair.as_mut().unwrap().operation_id = p.request.operation_id.clone(),
        |p| {
            p.repair.as_mut().unwrap().candidate.actor_name = p.request.candidate.actor_name.clone()
        },
        |p| p.repair.as_mut().unwrap().candidate.capability_id = "foreign-capability".into(),
        |p| {
            p.repair.as_mut().unwrap().candidate.capability_version =
                p.request.candidate.capability_version.clone()
        },
        |p| p.repair.as_mut().unwrap().candidate.actor_name = "invalid/name".into(),
        |p| p.repair.as_mut().unwrap().candidate.source_version = "invalid".into(),
        |p| p.model.max_tokens = 0,
        |p| p.request.allowed_source_urls.truncate(1),
    ];
    for (index, edit) in edits.into_iter().enumerate() {
        let mut plan = plan();
        edit(&mut plan);
        assert!(
            matches!(
                AcquisitionSession::new(plan, &suite()),
                Err(AcquisitionError::InvalidPlan)
            ),
            "case {index}"
        );
    }
    let mut bad_input = suite();
    bad_input.cases[0].input["source_url"] = json!("https://unexpected.example.org/");
    assert!(matches!(
        AcquisitionSession::new(plan(), &bad_input),
        Err(AcquisitionError::InvalidPlan)
    ));
    let mut missing_input = suite();
    missing_input.cases[0]
        .input
        .as_object_mut()
        .unwrap()
        .remove("source_url");
    assert!(matches!(
        AcquisitionSession::new(plan(), &missing_input),
        Err(AcquisitionError::InvalidPlan)
    ));
    let mut bad_suite = suite();
    bad_suite.cases.clear();
    assert!(matches!(
        AcquisitionSession::new(plan(), &bad_suite),
        Err(AcquisitionError::InvalidOracle)
    ));
    let mut no_repair = plan();
    no_repair.repair = None;
    assert!(AcquisitionSession::new(no_repair, &suite()).is_ok());
    // A new Actor can start at source version 1.0 while the capability identity
    // and operation are fresh. Repair's source version need not increment.
    assert_eq!(
        plan().request.candidate.source_version,
        plan().repair.unwrap().candidate.source_version
    );
}

#[test]
fn state_transitions_are_atomic_on_wrong_phase_changed_suite_and_invalid_completion() {
    let suite = suite();
    let mut session = session();
    assert_eq!(session.phase(), AcquisitionPhase::Generating);
    assert_eq!(session.attempt(), 0);
    assert!(session.forge().is_none());
    assert!(!session.advance_local(&suite).unwrap());
    let before = serialized(&session);
    assert!(session.prepare_forge(&suite).is_err());
    assert!(session
        .complete_generation(Err(SynthesisError::Protocol), &suite)
        .is_err());
    assert_eq!(serialized(&session), before);
    let mut changed = suite.clone();
    changed.scope.push_str(" changed");
    assert_eq!(
        session.begin_generation(&changed),
        Err(AcquisitionError::SuiteChanged)
    );
    assert_eq!(
        session.recover_interrupted(&changed),
        Err(AcquisitionError::SuiteChanged)
    );
    assert_eq!(serialized(&session), before);
    session.begin_generation(&suite).unwrap();
    let pending = serialized(&session);
    assert!(session.begin_generation(&suite).is_err());
    assert!(session
        .complete_generation(Err(SynthesisError::InvalidInput), &suite)
        .is_err());
    assert_eq!(serialized(&session), pending);
    assert!(session.verified(&suite).is_err());
}

#[test]
fn generation_post_intent_recovers_once_without_resetting_to_retryable_state() {
    let suite = suite();
    let mut session = session();
    assert!(!session.recover_interrupted(&suite).unwrap());
    session.begin_generation(&suite).unwrap();
    let mut restored: AcquisitionSession = serde_json::from_slice(&serialized(&session)).unwrap();
    restored.validate(&suite).unwrap();
    assert!(restored.recover_interrupted(&suite).unwrap());
    assert!(!restored.recover_interrupted(&suite).unwrap());
    assert_eq!(restored.phase(), AcquisitionPhase::NeedsReconciliation);
    assert_eq!(
        restored.generation().phase(),
        SynthesisPhase::NeedsReconciliation
    );
    assert_eq!(
        restored.generation().failure(),
        Some(&SynthesisError::Interrupted)
    );
    let before = serialized(&restored);
    assert!(restored.begin_generation(&suite).is_err());
    assert!(!restored.advance_local(&suite).unwrap());
    assert_eq!(serialized(&restored), before);
}

#[test]
fn native_generation_failure_is_terminal_without_consuming_repair() {
    for (error, phase) in [
        (
            SynthesisError::Http { status: 429 },
            AcquisitionPhase::Failed,
        ),
        (SynthesisError::Protocol, AcquisitionPhase::Failed),
        (
            SynthesisError::AmbiguousTransport,
            AcquisitionPhase::NeedsReconciliation,
        ),
    ] {
        let suite = suite();
        let mut session = session();
        session.begin_generation(&suite).unwrap();
        session
            .complete_generation(Err(error.clone()), &suite)
            .unwrap();
        assert_eq!(session.phase(), phase);
        assert_eq!(session.attempt(), 0);
        assert_eq!(session.generation().failure(), Some(&error));
        assert!(!session.advance_local(&suite).unwrap());
        assert!(session.begin_generation(&suite).is_err());
        let restored: AcquisitionSession = serde_json::from_slice(&serialized(&session)).unwrap();
        restored.validate(&suite).unwrap();
    }
}

#[test]
fn checkpoint_drift_and_false_verified_phase_cannot_create_acceptance() {
    let session = session();
    let value = serde_json::to_value(&session).unwrap();
    for (pointer, replacement) in [
        ("/schema_version", json!(2)),
        ("/phase", json!("verified")),
        ("/plan/request/operation_id", json!("substituted")),
        ("/plan/repair/operation_id", json!("substituted-repair")),
        ("/suite_sha256", json!("0".repeat(64))),
        ("/unchanged_repair", json!(true)),
        ("/checkpoint_sha256", json!("0".repeat(64))),
    ] {
        let mut changed = value.clone();
        *changed.pointer_mut(pointer).unwrap() = replacement;
        assert!(
            serde_json::from_value::<AcquisitionSession>(changed).is_err(),
            "{pointer}"
        );
    }
    let mut unknown = value.clone();
    unknown["publication_grant"] = json!(true);
    assert!(serde_json::from_value::<AcquisitionSession>(unknown).is_err());
    // Even a recomputed local hash is not semantic evidence. A claimed outer
    // success must match a linked, independently reverified ForgeSession.
    let mut forged = value;
    forged["phase"] = json!("verified");
    let digest_input = serde_json::to_value((
        "genesis-acquisition-v1",
        1u32,
        forged["plan_sha256"].as_str().unwrap(),
        forged["suite_sha256"].as_str().unwrap(),
        session.generation(),
        Option::<()>::None,
        Option::<()>::None,
        false,
        AcquisitionPhase::Verified,
    ))
    .unwrap();
    forged["checkpoint_sha256"] = json!(json_sha256(&digest_input));
    let restored: AcquisitionSession = serde_json::from_value(forged).unwrap();
    assert_eq!(
        restored.validate(&suite()),
        Err(AcquisitionError::CorruptCheckpoint)
    );
    assert!(restored.verified(&suite()).is_err());
}

#[test]
fn synthesis_repair_context_is_closed_bounded_and_bound_without_changing_old_prompts() {
    let plan = plan();
    let original = SynthesisSession::new(plan.request.clone(), plan.model.clone()).unwrap();
    let old_checkpoint = serde_json::to_value(&original).unwrap();
    assert!(old_checkpoint.get("repair").is_none());
    assert_eq!(
        original.prompt().unwrap().user,
        serde_json::to_string(&plan.request).unwrap()
    );
    let restored: SynthesisSession = serde_json::from_value(old_checkpoint.clone()).unwrap();
    assert!(restored.repair_context().is_none());
    assert_eq!(restored.prompt().unwrap(), original.prompt().unwrap());
    let context = SynthesisRepairContext {
        previous_extractor_typescript: "export function extract() { return {}; }".into(),
        feedback: vec![
            RepairFeedback::MalformedOutput,
            RepairFeedback::ValueMismatch,
        ],
    };
    let repair =
        SynthesisSession::new_repair(plan.request.clone(), plan.model.clone(), context.clone())
            .unwrap();
    let prompt = repair.prompt().unwrap();
    let user: serde_json::Value = serde_json::from_str(&prompt.user).unwrap();
    assert_eq!(
        user["repair"]["feedback"],
        json!(["malformed_output", "value_mismatch"])
    );
    assert_eq!(
        user["request"],
        serde_json::to_value(&plan.request).unwrap()
    );
    assert_eq!(user.as_object().unwrap().len(), 2);
    assert_eq!(user["repair"].as_object().unwrap().len(), 2);
    let repair_checkpoint = serde_json::to_value(&repair).unwrap();
    assert_ne!(
        old_checkpoint["request_sha256"],
        repair_checkpoint["request_sha256"]
    );
    let mut tampered = repair_checkpoint;
    tampered["repair"]["previous_extractor_typescript"] = json!("substituted");
    assert!(serde_json::from_value::<SynthesisSession>(tampered).is_err());
    for feedback in [
        vec![],
        vec![
            RepairFeedback::ValueMismatch,
            RepairFeedback::MalformedOutput,
        ],
        vec![RepairFeedback::ValueMismatch, RepairFeedback::ValueMismatch],
    ] {
        let mut invalid = context.clone();
        invalid.feedback = feedback;
        assert!(
            SynthesisSession::new_repair(plan.request.clone(), plan.model.clone(), invalid)
                .is_err()
        );
    }
    assert!(serde_json::from_str::<RepairFeedback>("\"source_mismatch\"").is_err());
    assert!(serde_json::from_str::<SynthesisRepairContext>(r#"{"previous_extractor_typescript":"source","feedback":["value_mismatch"],"expected":"oracle answer"}"#).is_err());
    let mut oversized = context.clone();
    oversized.previous_extractor_typescript = "π".repeat(64 * 1024 + 1);
    assert!(
        SynthesisSession::new_repair(plan.request.clone(), plan.model.clone(), oversized).is_err()
    );
    // Worst-case JSON escaping remains within the explicitly enlarged repair
    // prompt bound. Nothing parses or executes this source locally.
    let escaped = SynthesisRepairContext {
        previous_extractor_typescript: "\u{0001}".repeat(128 * 1024),
        ..context
    };
    let bounded = SynthesisSession::new_repair(plan.request, plan.model, escaped).unwrap();
    assert!(bounded.prompt().unwrap().user.len() > 768 * 1024);
}
