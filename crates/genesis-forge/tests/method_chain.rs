use genesis_core::development_method::ResourceGrant;
use genesis_core::development_trial::TrialPlan;
use genesis_forge::{
    method_accounting::CostScope,
    method_admission::GenerationAdmission,
    method_chain::{ChainAllocation, ChainStage, ChainStageBinding, ChainWork},
    method_roster::GenerationRoster,
    method_stage::{StageAllocation, StageLimits},
    method_synthesis::MethodSynthesisSession,
};
#[path = "common/method_accounting_fixture.rs"]
mod fixture;

fn grant(value: u64) -> ResourceGrant {
    ResourceGrant {
        model_calls: value,
        input_tokens: value,
        output_tokens: value,
        wall_time_ms: value,
        simulation_steps: value,
    }
}
fn setup() -> (ChainStage, StageAllocation, TrialPlan, CostScope) {
    let generated = fixture::generation("one", true);
    let prepared = MethodSynthesisSession::new(generated.plan().clone()).unwrap();
    let mut trial = fixture::trial(&generated);
    for slot in &mut trial.slots {
        slot.org_id = "org_a".into();
    }
    let limits = StageLimits {
        id: "develop-a".into(),
        max_trial_slots: 2,
        development_grant: trial.development_grant.clone(),
        scoring_grant: trial.scoring_grant.clone(),
        total_grant: grant(3000),
    };
    let roster = GenerationRoster::freeze(
        "roster".into(),
        prepared.plan().settings.clone(),
        grant(2000),
        std::slice::from_ref(&prepared),
    )
    .unwrap();
    let allocation = StageAllocation::prepare(
        limits.clone(),
        &GenerationAdmission::new(roster).unwrap(),
        std::slice::from_ref(&prepared),
    )
    .unwrap();
    let stage = ChainStage {
        org_id: "org_a".into(),
        limits,
        trial_model_settings: trial.model_settings.clone(),
        work: ChainWork::Generation {
            settings: prepared.plan().settings.clone(),
            max_attempts: 1,
            grant: grant(2000),
        },
    };
    let mut scope = fixture::scope(&[prepared], Some(&trial));
    scope.total_grant = grant(3000);
    (stage, allocation, trial, scope)
}
fn chain(stages: Vec<ChainStage>) -> ChainAllocation {
    ChainAllocation::new("chain".into(), grant(3000 * stages.len() as u64), stages).unwrap()
}

#[test]
fn generation_and_trial_only_stages_bind_without_fabricated_generation() {
    let (stage, allocation, trial, mut scope) = setup();
    let mut inherited = stage.clone();
    inherited.limits.id = "return-a".into();
    inherited.work = ChainWork::TrialOnly;
    let envelope = chain(vec![stage, inherited]);
    let saved = serde_json::to_vec(&envelope).unwrap();
    let restored: ChainAllocation = serde_json::from_slice(&saved).unwrap();
    assert_eq!(envelope.sha256().unwrap(), restored.sha256().unwrap());
    let binding = envelope.bind_generation("develop-a", &allocation).unwrap();
    let trial_binding = allocation.bind(&trial, &scope).unwrap();
    binding
        .validate_trial(
            &restored,
            Some((&allocation, &trial_binding)),
            &trial,
            &scope,
        )
        .unwrap();
    scope.generations.clear();
    let evidence = [fixture::evidence(&trial, "candidate")];
    let costs =
        genesis_forge::method_accounting::assess_costs(&scope, &[], Some((&trial, &evidence)))
            .unwrap();
    assert_eq!(costs.rows.len(), 4);
    assert!(costs.combined[0].unknown);
    let returned = envelope
        .bind_trial_only("return-a", &trial, &scope)
        .unwrap();
    let returned: ChainStageBinding =
        serde_json::from_slice(&serde_json::to_vec(&returned).unwrap()).unwrap();
    returned
        .validate_trial(&restored, None, &trial, &scope)
        .unwrap();
    assert!(envelope.bind_generation("return-a", &allocation).is_err());
    assert!(returned
        .validate_trial(
            &envelope,
            Some((&allocation, &trial_binding)),
            &trial,
            &scope
        )
        .is_err());
}

#[test]
fn full_declared_ceilings_cannot_be_recycled_and_order_is_identity() {
    let (a, _, _, _) = setup();
    let mut b = a.clone();
    b.limits.id = "revise-b".into();
    b.org_id = "org_b".into();
    let original = chain(vec![a.clone(), b.clone()]);
    assert_ne!(
        original.sha256().unwrap(),
        chain(vec![b.clone(), a.clone()]).sha256().unwrap()
    );
    assert!(ChainAllocation::new("chain".into(), grant(5999), vec![a.clone(), b]).is_err());
    assert!(ChainAllocation::new("chain".into(), grant(6000), vec![a.clone(), a]).is_err());
    assert!(ChainAllocation::new("chain".into(), grant(0), vec![]).is_err());
    let mut saved = serde_json::to_value(original).unwrap();
    saved["schema_version"] = 2.into();
    assert!(serde_json::from_value::<ChainAllocation>(saved).is_err());
}

#[test]
fn every_resource_has_checked_stage_and_chain_arithmetic() {
    let (stage, _, _, _) = setup();
    for resource in [
        "model_calls",
        "input_tokens",
        "output_tokens",
        "wall_time_ms",
        "simulation_steps",
    ] {
        for target in ["stage", "chain"] {
            let mut value = serde_json::to_value(&stage).unwrap();
            value["limits"]["total_grant"][resource] = u64::MAX.into();
            if target == "stage" {
                value["limits"]["development_grant"][resource] = u64::MAX.into();
            }
            let a: ChainStage = serde_json::from_value(value).unwrap();
            let mut b = a.clone();
            b.limits.id = "other".into();
            let stages = if target == "stage" {
                vec![a]
            } else {
                vec![a, b]
            };
            assert!(
                ChainAllocation::new("chain".into(), grant(u64::MAX), stages).is_err(),
                "{resource}/{target}"
            );
        }
    }
}

#[test]
fn generated_binding_rejects_settings_organization_count_and_ceiling_substitution() {
    let (stage, allocation, _, _) = setup();
    let original = chain(vec![stage.clone()]);
    let binding = original.bind_generation("develop-a", &allocation).unwrap();
    for field in ["org", "settings", "grant", "limits"] {
        let mut changed = stage.clone();
        match field {
            "org" => changed.org_id = "other".into(),
            "settings" => {
                if let ChainWork::Generation { settings, .. } = &mut changed.work {
                    settings.model = "other-model".into();
                }
            }
            "grant" => {
                if let ChainWork::Generation { grant, .. } = &mut changed.work {
                    grant.input_tokens += 1;
                }
            }
            _ => changed.limits.total_grant.input_tokens += 1,
        }
        let changed = ChainAllocation::new("chain".into(), grant(4000), vec![changed]).unwrap();
        assert!(changed.bind_generation("develop-a", &allocation).is_err());
        assert!(
            binding.validate_generation(&changed, &allocation).is_err(),
            "{field}"
        );
    }
    let sessions: Vec<_> = ["one", "two"]
        .into_iter()
        .map(|id| {
            MethodSynthesisSession::new(fixture::generation(id, true).plan().clone()).unwrap()
        })
        .collect();
    let roster = GenerationRoster::freeze(
        "more".into(),
        sessions[0].plan().settings.clone(),
        grant(2000),
        &sessions,
    )
    .unwrap();
    let more = StageAllocation::prepare(
        stage.limits.clone(),
        &GenerationAdmission::new(roster).unwrap(),
        &sessions,
    )
    .unwrap();
    assert!(original.bind_generation("develop-a", &more).is_err());
    assert!(original.bind_generation("undeclared", &allocation).is_err());
}

#[test]
fn trial_only_rejects_smuggled_generation_and_changed_exact_trial() {
    let (mut stage, _, trial, mut scope) = setup();
    stage.work = ChainWork::TrialOnly;
    let envelope = chain(vec![stage]);
    assert!(envelope
        .bind_trial_only("develop-a", &trial, &scope)
        .is_err());
    scope.generations.clear();
    let binding = envelope
        .bind_trial_only("develop-a", &trial, &scope)
        .unwrap();
    let mut reordered = scope.clone();
    reordered.trial.as_mut().unwrap().slots.reverse();
    assert!(envelope
        .bind_trial_only("develop-a", &trial, &reordered)
        .is_err());
    for field in ["identity", "model", "org", "grant"] {
        let mut changed = trial.clone();
        match field {
            "identity" => changed.id = "later-trial".into(),
            "model" => changed.model_settings.sha256 = "0".repeat(64),
            "org" => {
                for slot in &mut changed.slots {
                    slot.org_id = "other".into();
                }
            }
            _ => changed.development_grant.input_tokens += 1,
        }
        let mut changed_scope = fixture::scope(&[], Some(&changed));
        changed_scope.total_grant = scope.total_grant.clone();
        assert_eq!(
            envelope
                .bind_trial_only("develop-a", &changed, &changed_scope)
                .is_ok(),
            field == "identity"
        );
        assert!(
            binding
                .validate_trial(&envelope, None, &changed, &changed_scope)
                .is_err(),
            "{field}"
        );
    }
}
