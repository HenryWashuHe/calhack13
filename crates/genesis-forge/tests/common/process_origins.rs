//! Inert typed plans and receipts only; no method/evaluator/provider execution.
use genesis_core::development_method::*;
use genesis_forge::{
    method_revision::*, method_synthesis::*, process_cohort::*, process_generation_origins::*,
    process_selection::*, process_selection_record::*, text_generation::*,
};
use serde_json::json;
use sha2::{Digest, Sha256};
#[allow(dead_code)]
#[path = "method_accounting_fixture.rs"]
mod fixture;

fn pin(record: &RetainedSelection) -> SelectionPin {
    SelectionPin {
        arm: record.inputs.arm,
        family: record.inputs.family,
        selection_sha256: record.sha256(),
        rounds: record.inputs.rounds.clone().map(|r| match r {
            RoundInput::NoMethod { reason } => ExpectedRound::NoMethod { reason },
            RoundInput::Trial { input } => ExpectedRound::Trial {
                plan: input.plan.reference().unwrap(),
            },
        }),
    }
}
pub fn declaration(records: &[RetainedSelection; 8]) -> CohortDeclaration {
    CohortDeclaration {
        schema: "genesis-process-cohort-declaration/1".into(),
        selections: records.clone().map(|r| pin(&r)),
    }
}
fn base_records() -> [RetainedSelection; 8] {
    let base = fixture::trial(&fixture::generation("inert", true));
    let families = [
        Family::RavineCrossing,
        Family::RoughTransport,
        Family::ReturnMix,
        Family::ProjectileTarget,
    ];
    std::array::from_fn(|cell| {
        let family = families[cell % 4];
        RetainedSelection::capture(SelectionInputs {
            arm: if cell < 4 { Arm::Workflow } else { Arm::Fresh }, family,
            rounds: std::array::from_fn(|round| {
                let mut plan = base.clone();
                plan.id = format!("trial-{cell}-{round}");
                plan.context.public_task.id = family.name().into();
                plan.context.instance_set.id = family.name().into();
                plan.context.generator.id = family.name().into();
                plan.context.scoring_rule.id = family.name().into();
                plan.development_instances = (0..6).map(|seed| ExactContentRef {
                    domain: "genesis-lab".into(), schema: "lab-instance/1".into(),
                    id: format!("{}/1/{seed}", family.name()), version: "1".into(), sha256: "a".repeat(64),
                }).collect();
                let criterion = serde_json::to_vec(&json!({"rule":"success-count-margin", "min_margin":1,
                    "instances":plan.development_instances.iter().map(|r| &r.id).collect::<Vec<_>>()})).unwrap();
                plan.criterion.sha256 = format!("{:x}", Sha256::digest(&criterion));
                plan.development_grant = ResourceGrant { model_calls: 0, input_tokens: 0, output_tokens: 0, wall_time_ms: 60000, simulation_steps: 72000 };
                plan.scoring_grant = ResourceGrant { simulation_steps: if cell % 4 == 3 { 7200 } else { 14400 }, wall_time_ms: 600000, ..plan.development_grant.clone() };
                for slot in &mut plan.slots {
                    slot.run_id = format!("{cell}-{round}-{}", slot.slot_id);
                    slot.attempt_id = slot.run_id.clone().into();
                    slot.scoring_request_id = slot.run_id.clone();
                }
                plan.slots[0].method.id = format!("method-{}-{round}", cell / 4);
                RoundInput::Trial { input: Box::new(TrialInput { plan, criterion, evidence: None, metrics: None }) }
            }),
        }).unwrap()
    })
}

#[path = "method_plan.rs"]
mod plan_fixture;

pub fn complete(plan: MethodSynthesisPlan) -> MethodSynthesisSession {
    let mut session = MethodSynthesisSession::new(plan).unwrap();
    let request = session.begin().unwrap();
    let mut completion = TextCompletion::interrupted(&request).unwrap();
    completion.transport = GenerationTransport::LoopbackFixture;
    completion.actual_model = Some(request.settings.model.clone());
    completion.message_id = Some("msg_inert".into());
    completion.response_sha256 = Some("a".repeat(64));
    completion.usage.model_calls = ResourceUsage::Measured { value: 1 };
    completion.outcome = TextOutcome::Returned {
        text: r#"{"source":"inert source fixture"}"#.into(),
    };
    session.complete(completion).unwrap();
    session
}

pub fn fixture() -> (GenerationOrigins, [RetainedSelection; 8]) {
    let mut base = plan_fixture::plan();
    base.settings.model = "claude-sonnet-5-5".into();
    base.settings.reasoning = Some(ReasoningPolicy::Adaptive {
        effort: ReasoningEffort::High,
    });
    let mut positions: Vec<GenerationOrigin> = vec![];
    for index in 0..6 {
        let arm = if index < 3 { Arm::Workflow } else { Arm::Fresh };
        let round = index % 3 + 1;
        let mut plan = base.clone();
        plan.operation_id = format!("op-{index}");
        plan.target_id = format!("method-{index}");
        let revision = if arm == Arm::Workflow && round > 1 {
            let parent = positions[index - 1]
                .snapshot
                .as_ref()
                .unwrap()
                .package()
                .unwrap();
            Some(
                MethodRevisionInput::new(
                    MethodRevisionSpec {
                        generation: plan.clone(),
                        parent: parent.reference().unwrap(),
                    },
                    parent.artifact_json().unwrap(),
                )
                .unwrap(),
            )
        } else {
            None
        };
        if let Some(revision) = &revision {
            plan = revision.prepare().unwrap().plan().clone();
        }
        let snapshot = complete(plan);
        positions.push(GenerationOrigin {
            arm,
            round: round as u8,
            prepared_checkpoint_sha256: Some(snapshot.prepared_checkpoint_sha256().unwrap()),
            snapshot: Some(snapshot),
            revision,
            no_method_reason: None,
        });
    }
    let origins = GenerationOrigins {
        schema: "genesis-process-generation-origins/1".into(),
        base,
        positions: positions.try_into().unwrap(),
    };
    let mut records = base_records();
    bind(&origins, &mut records);
    (origins, records)
}

pub fn bind(origins: &GenerationOrigins, records: &mut [RetainedSelection; 8]) {
    for selection in records {
        for round in 1..=3 {
            let origin = origins
                .positions
                .iter()
                .find(|o| o.arm == selection.inputs.arm && o.round == round)
                .unwrap();
            if let Some(package) = origin.snapshot.as_ref().and_then(|s| s.package().ok()) {
                let RoundInput::Trial { input } =
                    &mut selection.inputs.rounds[usize::from(round - 1)]
                else {
                    panic!("fixture needs original trial")
                };
                input.plan.slots[0].method = package.reference().unwrap();
                input.plan.slots[0].dependencies = origin
                    .snapshot
                    .as_ref()
                    .unwrap()
                    .plan()
                    .dependencies
                    .clone();
            } else {
                selection.inputs.rounds[usize::from(round - 1)] = RoundInput::NoMethod {
                    reason: origin.no_method_reason.clone().unwrap(),
                };
            }
        }
        *selection = RetainedSelection::capture(selection.inputs.clone()).unwrap();
    }
}
