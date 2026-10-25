//! Final development roster checks, not generation history or fresh-panel results.
use crate::{
    process_selection::{Arm, Family, RoundInput, SelectionReport},
    process_selection_record::RetainedSelection,
};
use genesis_core::{
    development_method::{AcceptanceRole, ExactContentRef, ResourceGrant},
    development_trial::TrialPurpose,
    digest::canonical_sha256,
};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::{BTreeMap, BTreeSet};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum ExpectedRound {
    Trial { plan: ExactContentRef },
    NoMethod { reason: String },
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SelectionPin {
    pub arm: Arm,
    pub family: Family,
    pub selection_sha256: String,
    pub rounds: [ExpectedRound; 3],
}
/// Supply from an independently retained declaration, not from untrusted reports.
/// Freeze before fresh-panel preparation. This hash does not prove when it existed.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CohortDeclaration {
    pub schema: String,
    pub selections: [SelectionPin; 8],
}
#[derive(Debug, Serialize)]
pub struct CohortCell {
    pub selection_sha256: String,
    /// Includes all three original rows and the full selected plan/method refs.
    pub selection: SelectionReport,
}
#[derive(Debug, Serialize)]
pub struct CohortReport {
    pub schema: &'static str,
    pub protocol: &'static str,
    pub scope: &'static str,
    pub declaration_sha256: String,
    pub cells: Vec<CohortCell>,
}
fn equal_policy<K: Ord>(
    seen: &mut BTreeMap<K, Value>,
    key: K,
    value: Value,
    message: &str,
) -> Result<(), String> {
    if seen
        .insert(key, value.clone())
        .is_some_and(|old| old != value)
    {
        return Err(message.into());
    }
    Ok(())
}
fn grant(steps: u64, millis: u64) -> ResourceGrant {
    ResourceGrant {
        model_calls: 0,
        input_tokens: 0,
        output_tokens: 0,
        wall_time_ms: millis,
        simulation_steps: steps,
    }
}
/// Recompute every externally pinned selection, retaining no_candidate and all
/// failed/unknown/missing rounds. This neither grants authority nor authenticates
/// the supplied declarations/evidence. The caller must bound serialized inputs.
pub fn assess_cohort(
    declaration: &CohortDeclaration,
    selections: &[RetainedSelection; 8],
) -> Result<CohortReport, String> {
    if declaration.schema != "genesis-process-cohort-declaration/1" {
        return Err("unsupported process cohort schema".into());
    }
    let mut received = BTreeMap::new();
    for selection in selections {
        if received
            .insert((selection.inputs.arm, selection.inputs.family), selection)
            .is_some()
        {
            return Err("repeated arm/family selection".into());
        }
    }
    let (mut keys, mut trials, mut runs, mut attempts, mut scores) = (
        BTreeSet::new(),
        BTreeSet::new(),
        BTreeSet::new(),
        BTreeSet::new(),
        BTreeSet::new(),
    );
    let (mut candidates, mut family_policy, mut shared_policy) =
        (BTreeMap::new(), BTreeMap::new(), BTreeMap::new());
    let mut cells = Vec::with_capacity(8);
    for pin in &declaration.selections {
        let key = (pin.arm, pin.family);
        if !keys.insert(key) {
            return Err("repeated arm/family declaration".into());
        }
        let retained = received
            .get(&key)
            .ok_or("missing declared arm/family selection")?;
        if retained.sha256() != pin.selection_sha256 {
            return Err("selection differs from external canonical digest".into());
        }
        let report = retained.check()?;
        for (index, (expected, input)) in pin.rounds.iter().zip(&retained.inputs.rounds).enumerate()
        {
            let candidate = match (expected, input) {
                (ExpectedRound::NoMethod { reason }, RoundInput::NoMethod { reason: actual })
                    if reason == actual =>
                {
                    json!({"no_method":reason})
                }
                (ExpectedRound::Trial { plan: expected }, RoundInput::Trial { input }) => {
                    let plan = &input.plan;
                    let reference = plan.reference().map_err(|e| e.to_string())?;
                    if reference != *expected {
                        return Err("trial differs from declared full reference".into());
                    }
                    if !trials.insert((reference.id, reference.version)) {
                        return Err("repeated trial across cohort".into());
                    }
                    for slot in &plan.slots {
                        if !runs.insert((slot.org_id.clone(), slot.run_id.clone()))
                            || !attempts.insert((slot.org_id.clone(), slot.attempt_id.clone()))
                            || !scores
                                .insert((slot.org_id.clone(), slot.scoring_request_id.clone()))
                        {
                            return Err(
                                "repeated run, attempt or scoring identity across cohort".into()
                            );
                        }
                    }
                    let candidate = plan
                        .slots
                        .iter()
                        .find(|s| {
                            s.purpose
                                == TrialPurpose::Acceptance {
                                    role: AcceptanceRole::Candidate,
                                }
                        })
                        .ok_or("missing candidate")?;
                    let control = plan
                        .slots
                        .iter()
                        .find(|s| {
                            s.purpose
                                == TrialPurpose::Acceptance {
                                    role: AcceptanceRole::Control,
                                }
                        })
                        .ok_or("missing control")?;
                    let max_steps = if pin.family == Family::ProjectileTarget {
                        1200
                    } else {
                        2400
                    };
                    if plan.development_grant != grant(72000, 60000)
                        || plan.scoring_grant != grant(6 * max_steps, 600000)
                    {
                        return Err("trial ceilings differ from process-comparison/1".into());
                    }
                    // Context/task/criterion/instances must agree between W/F and
                    // rounds. Across families only the task-specific refs vary.
                    let mut policy = json!(plan);
                    policy.as_object_mut().unwrap().remove("id");
                    policy.as_object_mut().unwrap().remove("slots");
                    equal_policy(
                        &mut family_policy,
                        pin.family,
                        policy,
                        "family task, criterion or policy differs across arms",
                    )?;
                    equal_policy(
                        &mut shared_policy,
                        (),
                        json!({
                            "org":candidate.org_id, "control":control.method,
                            "control_dependencies":control.dependencies,
                            "model_settings":plan.model_settings,
                            "environment":plan.development_environment,
                            "evaluator":plan.context.evaluator,
                            "scoring_environment":plan.context.environment,
                            "design_schema":plan.context.design_schema,
                            "metric_schema":plan.context.metric_schema,
                        }),
                        "control, settings or environment differs across cohort",
                    )?;
                    json!({"method":candidate.method,"dependencies":candidate.dependencies})
                }
                _ => return Err("round presence or absence reason differs from declaration".into()),
            };
            equal_policy(
                &mut candidates,
                (pin.arm, index),
                candidate,
                "arm/round method, dependencies or absence differs across families",
            )?;
        }
        cells.push(CohortCell {
            selection_sha256: pin.selection_sha256.clone(),
            selection: report,
        });
    }
    // Arm/Family are closed enums: eight distinct pairs exhaust their product.
    cells.sort_by_key(|c| (c.selection.arm, c.selection.family));
    Ok(CohortReport {
        schema: "genesis-process-cohort-report/1",
        protocol: "process-comparison/1",
        scope: "complete_development_selection_roster",
        declaration_sha256: canonical_sha256(&json!(declaration)),
        cells,
    })
}
