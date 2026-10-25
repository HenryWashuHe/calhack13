//! Independent PR196 guards using inert retained records only. No source,
//! subprocess, evaluator, provider or network execution.
use genesis_core::development_method::ResourceUsage;
use genesis_forge::{
    method_revision::{MethodRevisionInput, MethodRevisionSpec},
    method_synthesis::{MethodSynthesisPhase, MethodSynthesisSession},
    process_generation_origins::{assess_generation_origins, GenerationOrigins},
    process_selection::{Arm, RoundInput},
    process_selection_record::RetainedSelection,
    text_generation::{GenerationTransport, TextCompletion, TextFailure, TextOutcome},
};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
#[allow(dead_code)]
#[path = "common/process_origins.rs"]
mod fixture;

fn assess(origins: &GenerationOrigins, records: &[RetainedSelection; 8]) -> Result<Value, String> {
    assess_generation_origins(origins, &fixture::declaration(records), records)
}
fn recapture(records: &mut [RetainedSelection; 8]) {
    for record in records {
        *record = RetainedSelection::capture(record.inputs.clone()).unwrap();
    }
}
fn pin_snapshot(origins: &mut GenerationOrigins, index: usize, snapshot: MethodSynthesisSession) {
    origins.positions[index].prepared_checkpoint_sha256 =
        Some(snapshot.prepared_checkpoint_sha256().unwrap());
    origins.positions[index].snapshot = Some(snapshot);
}
fn position<'a>(report: &'a Value, arm: &str, round: u8) -> &'a Value {
    report["positions"]
        .as_array()
        .unwrap()
        .iter()
        .find(|r| r["arm"] == arm && r["round"] == round)
        .unwrap()
}

#[test]
fn review_full_package_identity_and_dependencies_survive_repinned_cohort() {
    for mutation in 0..6 {
        let (origins, mut records) = fixture::fixture();
        assert!(assess(&origins, &records).is_ok());
        for record in records.iter_mut().filter(|r| r.inputs.arm == Arm::Fresh) {
            let RoundInput::Trial { input } = &mut record.inputs.rounds[0] else {
                unreachable!()
            };
            let candidate = &mut input.plan.slots[0];
            match mutation {
                0 => candidate.method.domain.push_str("-foreign"),
                1 => candidate.method.schema.push_str("-foreign"),
                2 => candidate.method.id.push_str("-foreign"),
                3 => candidate.method.version.push_str("-foreign"),
                4 => candidate.method.sha256 = "f".repeat(64),
                _ => candidate.dependencies.push(origins.base.runtime.clone()),
            }
        }
        recapture(&mut records);
        let error = assess(&origins, &records).unwrap_err();
        assert!(
            error.contains("original generated package or dependencies"),
            "{mutation}: {error}"
        );
    }
}

#[test]
fn review_terminal_hash_is_not_prepared_and_snapshots_always_require_a_pin() {
    for terminal_hash in [false, true] {
        let (mut origins, records) = fixture::fixture();
        let snapshot = origins.positions[3].snapshot.as_ref().unwrap();
        assert_ne!(
            snapshot.checkpoint_sha256(),
            snapshot.prepared_checkpoint_sha256().unwrap()
        );
        origins.positions[3].prepared_checkpoint_sha256 =
            terminal_hash.then(|| snapshot.checkpoint_sha256().to_owned());
        assert!(assess(&origins, &records)
            .unwrap_err()
            .contains("pinned Prepared"));
    }
}

#[test]
fn review_reused_attempt_cannot_be_hidden_by_distinct_output_and_repinning() {
    let (mut origins, mut records) = fixture::fixture();
    let mut plan = origins.positions[4]
        .snapshot
        .as_ref()
        .unwrap()
        .plan()
        .clone();
    plan.operation_id = origins.positions[3]
        .snapshot
        .as_ref()
        .unwrap()
        .plan()
        .operation_id
        .clone();
    // Different target/package and request: only the organization+operation pair collides.
    let snapshot = fixture::complete(plan);
    assert_ne!(
        snapshot.request_sha256(),
        origins.positions[3]
            .snapshot
            .as_ref()
            .unwrap()
            .request_sha256()
    );
    pin_snapshot(&mut origins, 4, snapshot);
    fixture::bind(&origins, &mut records);
    assert!(assess(&origins, &records)
        .unwrap_err()
        .contains("attempt reused"));
}

#[test]
fn review_identical_packages_from_distinct_fresh_attempts_are_legitimate() {
    let (mut origins, mut records) = fixture::fixture();
    let mut plan = origins.positions[3]
        .snapshot
        .as_ref()
        .unwrap()
        .plan()
        .clone();
    plan.operation_id = "independent-fresh-attempt".into();
    pin_snapshot(&mut origins, 4, fixture::complete(plan));
    fixture::bind(&origins, &mut records);
    let report = assess(&origins, &records).unwrap();
    let first = position(&report, "fresh", 1);
    let second = position(&report, "fresh", 2);
    assert_eq!(first["method"], second["method"]);
    assert_ne!(
        first["prepared_checkpoint_sha256"],
        second["prepared_checkpoint_sha256"]
    );
    assert_ne!(first["operation_id"], second["operation_id"]);
    assert_eq!(first["usage"]["input_tokens"]["kind"], "unknown");
    assert!(!report.to_string().contains("inert source fixture"));
}

#[test]
fn review_workflow_revision_cannot_jump_back_to_a_valid_earlier_parent() {
    let (mut origins, mut records) = fixture::fixture();
    let grandparent = origins.positions[0]
        .snapshot
        .as_ref()
        .unwrap()
        .package()
        .unwrap();
    let spec = MethodRevisionSpec {
        generation: origins.positions[2]
            .revision
            .as_ref()
            .unwrap()
            .spec
            .generation
            .clone(),
        parent: grandparent.reference().unwrap(),
    };
    let revision = MethodRevisionInput::new(spec, grandparent.artifact_json().unwrap()).unwrap();
    let snapshot = fixture::complete(revision.prepare().unwrap().plan().clone());
    origins.positions[2].revision = Some(revision);
    pin_snapshot(&mut origins, 2, snapshot);
    fixture::bind(&origins, &mut records);
    assert!(assess(&origins, &records)
        .unwrap_err()
        .contains("immediate parent"));
}

#[test]
fn review_reordered_revision_base_documents_fail_even_when_new_revision_is_valid() {
    let (mut origins, mut records) = fixture::fixture();
    let mut revision = origins.positions[1].revision.as_ref().unwrap().clone();
    // The appended document is allowed feedback, but it cannot replace the ordered base prefix.
    let extra = revision.spec.generation.public_task.clone();
    revision.spec.generation.public_inputs.insert(0, extra);
    let snapshot = fixture::complete(revision.prepare().unwrap().plan().clone());
    origins.positions[1].revision = Some(revision);
    pin_snapshot(&mut origins, 1, snapshot);
    fixture::bind(&origins, &mut records);
    assert!(assess(&origins, &records)
        .unwrap_err()
        .contains("base policy"));
}

#[test]
fn review_all_non_generated_phases_remain_absent_with_original_unknown_usage() {
    for phase in [
        MethodSynthesisPhase::Prepared,
        MethodSynthesisPhase::Requesting,
        MethodSynthesisPhase::Failed,
        MethodSynthesisPhase::NeedsReconciliation,
        MethodSynthesisPhase::RejectedSource,
    ] {
        let (mut origins, mut records) = fixture::fixture();
        let plan = origins.positions[3]
            .snapshot
            .as_ref()
            .unwrap()
            .plan()
            .clone();
        let mut snapshot = MethodSynthesisSession::new(plan).unwrap();
        if phase != MethodSynthesisPhase::Prepared {
            let request = snapshot.begin().unwrap();
            if phase != MethodSynthesisPhase::Requesting {
                let mut completion = TextCompletion::interrupted(&request).unwrap();
                match phase {
                    MethodSynthesisPhase::Failed => {
                        completion.outcome = TextOutcome::Failed {
                            reason: TextFailure::Http,
                        }
                    }
                    MethodSynthesisPhase::RejectedSource => {
                        completion.transport = GenerationTransport::LoopbackFixture;
                        completion.actual_model = Some(request.settings.model.clone());
                        completion.message_id = Some("msg_rejected_fixture".into());
                        completion.response_sha256 = Some("a".repeat(64));
                        completion.usage.model_calls = ResourceUsage::Measured { value: 1 };
                        completion.outcome = TextOutcome::Returned {
                            text: "not source JSON".into(),
                        };
                    }
                    _ => {}
                }
                snapshot.complete(completion).unwrap();
            }
        }
        assert_eq!(snapshot.phase(), phase);
        let original_usage = snapshot.completion().map(|c| json!(c.usage));
        pin_snapshot(&mut origins, 3, snapshot);
        origins.positions[3].no_method_reason = Some("retained generation unavailable".into());
        fixture::bind(&origins, &mut records);
        let report = assess(&origins, &records).unwrap();
        let row = position(&report, "fresh", 1);
        assert_eq!(row["phase"], json!(phase));
        assert!(row["method"].is_null());
        assert_eq!(row["usage"], original_usage.unwrap_or(Value::Null));
        if row["usage"].is_object() {
            assert_eq!(row["usage"]["input_tokens"]["kind"], "unknown");
        }
        // A single mismatched family absence must not be laundered by a fresh declaration.
        records[4].inputs.rounds[0] = RoundInput::NoMethod {
            reason: "different absence".into(),
        };
        recapture(&mut records);
        assert!(assess(&origins, &records).is_err());
    }
}

#[test]
fn review_resealed_custom_system_prompt_does_not_satisfy_canonical_generation() {
    let (mut origins, mut records) = fixture::fixture();
    let plan = origins.positions[3]
        .snapshot
        .as_ref()
        .unwrap()
        .plan()
        .clone();
    let prepared = MethodSynthesisSession::new(plan.clone()).unwrap();
    let mut request = plan.request().unwrap();
    request.system.push_str(" Different host contract.");
    let request_sha256 = request.sha256().unwrap();
    let checkpoint = serde_json::to_vec(&(
        "genesis-method-generation-checkpoint/1",
        1_u32,
        &plan,
        &request,
        &request_sha256,
        "genesis-method-source/1",
        MethodSynthesisPhase::Prepared,
        Option::<TextCompletion>::None,
    ))
    .unwrap();
    let mut value = json!(prepared);
    value["request"] = json!(request);
    value["request_sha256"] = json!(request_sha256);
    value["checkpoint_sha256"] = json!(format!("{:x}", Sha256::digest(checkpoint)));
    // Historical session integrity accepts a correctly resealed stored request.
    // The new process binding additionally requires the canonical host contract.
    let snapshot: MethodSynthesisSession = serde_json::from_value(value).unwrap();
    snapshot.validate().unwrap();
    pin_snapshot(&mut origins, 3, snapshot);
    origins.positions[3].no_method_reason = Some("pending".into());
    fixture::bind(&origins, &mut records);
    assert!(assess(&origins, &records)
        .unwrap_err()
        .contains("canonical source-generation"));
}
