use genesis_core::development_method::ResourceGrant;
use genesis_core::development_trial::TrialPlan;
use genesis_forge::{
    method_accounting::CostScope,
    method_admission::GenerationAdmission,
    method_roster::GenerationRoster,
    method_stage::{StageAllocation, StageLimits, StageTrialBinding},
    method_synthesis::MethodSynthesisSession,
};
#[path = "common/method_accounting_fixture.rs"]
mod fixture;

fn setup() -> (
    MethodSynthesisSession,
    GenerationAdmission,
    StageLimits,
    TrialPlan,
    CostScope,
) {
    let generated = fixture::generation("one", true);
    let prepared = MethodSynthesisSession::new(generated.plan().clone()).unwrap();
    let trial = fixture::trial(&generated);
    let admission = GenerationAdmission::new(
        GenerationRoster::freeze(
            "stage-roster".into(),
            prepared.plan().settings.clone(),
            ResourceGrant {
                model_calls: 1,
                input_tokens: 0,
                output_tokens: 512,
                wall_time_ms: 0,
                simulation_steps: 0,
            },
            std::slice::from_ref(&prepared),
        )
        .unwrap(),
    )
    .unwrap();
    let limits = StageLimits {
        id: "stage-a".into(),
        max_trial_slots: 2,
        development_grant: trial.development_grant.clone(),
        scoring_grant: trial.scoring_grant.clone(),
        total_grant: ResourceGrant {
            model_calls: 5,
            input_tokens: 40,
            output_tokens: 552,
            wall_time_ms: 400,
            simulation_steps: 400,
        },
    };
    let mut scope = fixture::scope(std::slice::from_ref(&prepared), Some(&trial));
    scope.total_grant = limits.total_grant.clone();
    (prepared, admission, limits, trial, scope)
}

#[test]
fn fixed_envelope_binds_later_trial_and_preserves_missing_failed_and_overrun_costs() {
    let (prepared, mut admission, limits, trial, scope) = setup();
    let allocation =
        StageAllocation::prepare(limits, &admission, std::slice::from_ref(&prepared)).unwrap();
    let saved = serde_json::to_vec(&allocation).unwrap();
    assert!(!String::from_utf8(saved.clone())
        .unwrap()
        .contains("Generate a method"));
    let restored: StageAllocation = serde_json::from_slice(&saved).unwrap();
    assert_eq!(restored.sha256().unwrap(), allocation.sha256().unwrap());
    let binding = allocation.bind(&trial, &scope).unwrap();
    let binding: StageTrialBinding =
        serde_json::from_slice(&serde_json::to_vec(&binding).unwrap()).unwrap();
    let missing = binding
        .assess(&restored, &admission, &scope, &[], (&trial, &[]))
        .unwrap();
    assert_eq!(missing.costs.costs.rows.len(), 5);
    assert!(missing.costs.costs.combined[0].unknown);
    admission.reserve(&prepared).unwrap();
    // A rejected source has spend but no method; no forced reuse is required.
    let rejected = fixture::generation("one", false);
    let evidence = [fixture::evidence(&trial, "candidate")];
    let report = binding
        .assess(
            &restored,
            &admission,
            &scope,
            &[rejected],
            (&trial, &evidence),
        )
        .unwrap();
    assert_eq!(report.costs.costs.combined[0].known_lower_bound, 3);
    assert!(report.costs.costs.combined[0].unknown);
    assert!(report.costs.generation.costs.combined[1].limit_exceeded);
    assert!(!report.costs.costs.combined[1].limit_exceeded);
    assert_eq!(saved, serde_json::to_vec(&allocation).unwrap());
    let mut foreign = prepared.plan().clone();
    foreign.operation_id = "other".into();
    let other = MethodSynthesisSession::new(foreign).unwrap();
    let other = GenerationAdmission::new(
        GenerationRoster::freeze(
            "other".into(),
            other.plan().settings.clone(),
            admission.roster().total_grant().clone(),
            &[other],
        )
        .unwrap(),
    )
    .unwrap();
    assert!(allocation.validate_admission(&other).is_err());
}

#[test]
fn every_resource_reserves_full_generation_and_all_possible_slot_grants() {
    let (prepared, admission, limits, _, _) = setup();
    for resource in [
        "model_calls",
        "input_tokens",
        "output_tokens",
        "wall_time_ms",
        "simulation_steps",
    ] {
        let mut value = serde_json::to_value(&limits).unwrap();
        let n = value["total_grant"][resource].as_u64().unwrap();
        value["total_grant"][resource] = (n - 1).into();
        assert!(
            StageAllocation::prepare(
                serde_json::from_value(value).unwrap(),
                &admission,
                std::slice::from_ref(&prepared)
            )
            .is_err(),
            "{resource}"
        );
        let mut value = serde_json::to_value(&limits).unwrap();
        value["development_grant"][resource] = u64::MAX.into();
        value["total_grant"][resource] = u64::MAX.into();
        assert!(
            StageAllocation::prepare(
                serde_json::from_value(value).unwrap(),
                &admission,
                std::slice::from_ref(&prepared)
            )
            .is_err(),
            "overflow {resource}"
        );
    }
    let mut multiplied = limits.clone();
    multiplied.max_trial_slots = 3;
    multiplied.development_grant.model_calls = u64::MAX / 2;
    multiplied.scoring_grant.model_calls = 0;
    multiplied.total_grant.model_calls = u64::MAX;
    assert!(
        StageAllocation::prepare(multiplied, &admission, std::slice::from_ref(&prepared)).is_err()
    );
    for slots in [0, 3, 65] {
        let mut changed = limits.clone();
        changed.max_trial_slots = slots;
        assert!(
            StageAllocation::prepare(changed, &admission, std::slice::from_ref(&prepared)).is_err()
        );
    }
    let allocation =
        StageAllocation::prepare(limits, &admission, std::slice::from_ref(&prepared)).unwrap();
    let mut encoded = serde_json::to_value(&allocation).unwrap();
    encoded["schema_version"] = 2.into();
    assert!(serde_json::from_value::<StageAllocation>(encoded).is_err());
    let mut encoded = serde_json::to_value(&allocation).unwrap();
    encoded["limits"]["total_grant"]["model_calls"] = 0.into();
    assert!(serde_json::from_value::<StageAllocation>(encoded).is_err());
}

#[test]
fn preparation_refuses_missing_duplicate_foreign_started_or_reserved_attempts() {
    let (prepared, mut admission, limits, _, _) = setup();
    for observations in [
        vec![],
        vec![prepared.clone(), prepared.clone()],
        vec![fixture::generation("one", true)],
        vec![fixture::generation("other", true)],
    ] {
        assert!(StageAllocation::prepare(limits.clone(), &admission, &observations).is_err());
    }
    let mut pending = prepared.clone();
    pending.begin().unwrap();
    assert!(StageAllocation::prepare(limits.clone(), &admission, &[pending]).is_err());
    admission.reserve(&prepared).unwrap();
    assert!(StageAllocation::prepare(limits, &admission, &[prepared]).is_err());
}

#[test]
fn exact_binding_rejects_changed_grants_slots_scope_and_plan_after_roundtrip() {
    let (prepared, admission, limits, trial, scope) = setup();
    let allocation = StageAllocation::prepare(limits, &admission, &[prepared]).unwrap();
    let binding = allocation.bind(&trial, &scope).unwrap();
    let (p, journal, mut limits, two_slots, costs) = setup();
    limits.max_trial_slots = 1;
    assert!(StageAllocation::prepare(limits, &journal, &[p])
        .unwrap()
        .bind(&two_slots, &costs)
        .is_err());
    for change in 0..8 {
        let mut p = trial.clone();
        let mut s = scope.clone();
        match change {
            0 => p.development_grant.model_calls += 1,
            1 => p.scoring_grant.wall_time_ms += 1,
            2 => s.total_grant.input_tokens += 1,
            3 => s.generations[0].request_sha256 = "a".repeat(64),
            4 => s.trial.as_mut().unwrap().slots.reverse(),
            5 => s.trial = None,
            6 => {
                s.generations[0].checkpoint_sha256 =
                    fixture::generation("one", true).checkpoint_sha256().into()
            }
            _ => p.id = "substituted".into(),
        }
        assert!(
            binding.validate_against(&allocation, &p, &s).is_err(),
            "{change}"
        );
    }
    // A later valid alternative plan can be constructed in memory, but cannot
    // validate as the original retained binding. Once-only storage is the host's job.
    let mut p = trial.clone();
    p.id = "other-valid-trial".into();
    let mut s = scope.clone();
    s.trial.as_mut().unwrap().plan = p.reference().unwrap();
    let other = allocation.bind(&p, &s).unwrap();
    assert_ne!(binding, other);
    assert!(binding.validate_against(&allocation, &p, &s).is_err());
    let mut altered = serde_json::to_value(&binding).unwrap();
    altered["schema_version"] = 2.into();
    let altered: StageTrialBinding = serde_json::from_value(altered).unwrap();
    assert!(altered
        .validate_against(&allocation, &trial, &scope)
        .is_err());
}
