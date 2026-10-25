use super::*;
use crate::method_invocation::{authorize_method_invocation, MethodInvocationRequest};
use genesis_core::development_evaluation::EvaluationReceipt;
use genesis_core::development_method::ExecutionReceipt;
use genesis_core::development_trial::TrialPlan;

fn invocation(f: &Fixture) -> MethodInvocationRequest {
    MethodInvocationRequest {
        role_id: f.intent.role_id.clone(),
        responsibility_id: f.intent.responsibility_id.clone(),
        publisher: f.intent.publisher.clone(),
        reference: f.intent.reference.clone(),
    }
}

#[test]
fn review_existing_schema_is_migrated_without_losing_adoption() {
    let f = Fixture::new();
    let (adopted, _) = f.create(&f.intent).unwrap();
    // Reconstruct the exact table/index shape shipped before this PR, retaining
    // the existing record bytes. Reopening an empty new-schema DB isn't migration.
    f.store.lock().execute_batch(
        "DROP INDEX method_adoption_adopted;
         ALTER TABLE method_adoptions DROP COLUMN fence_sha256;
         CREATE UNIQUE INDEX method_adoption_adopted ON method_adoptions(requirement_id,assignment_id,epoch,publisher,method_sha256) WHERE outcome='adopted';"
    ).unwrap();
    f.store.init_method_adoptions().unwrap();
    let loaded = f.store.method_adoption(&adopted.id);
    assert!(
        loaded.is_ok(),
        "existing records must survive upgrade: {loaded:?}"
    );
    assert_eq!(loaded.unwrap(), adopted);
    assert_eq!(f.create(&f.intent).unwrap(), (adopted, false));
}

fn assert_changed_identity_fences_existing_invocation(field: &str) {
    let f = Fixture::new();
    f.create(&f.intent).unwrap();
    let state = f.store.current_state().unwrap();
    {
        let mut receiver = f.records.receiver.clone();
        match field {
            "runtime" => receiver.runtime.sha256 = "0".repeat(64),
            _ => receiver.protocol.sha256 = "0".repeat(64),
        }
        let selected = SelectedMethod {
            publisher: f.intent.publisher.clone(),
            reference: f.intent.reference.clone(),
            package_json: package(),
        };
        assert!(matches!(
            assess_selected(&receiver, &state.org_id, &selected, "vs-fixed-cart"),
            Err(MethodAcceptanceError::Incompatible(_))
        ));
        let result =
            authorize_method_invocation(&f.store, &state, &receiver, &invocation(&f), f.now);
        assert!(
            result.is_err(),
            "{field} no longer accepts the method but still grants: {result:?}"
        );
    }
}

#[test]
fn review_changed_runtime_fences_existing_invocation() {
    assert_changed_identity_fences_existing_invocation("runtime");
}

#[test]
fn review_changed_protocol_fences_existing_invocation() {
    assert_changed_identity_fences_existing_invocation("protocol");
}

#[test]
fn review_new_accepted_plan_can_replace_a_policy_fenced_adoption() {
    let mut f = Fixture::new();
    let (old, _) = f.create(&f.intent).unwrap();
    let root = &f.records.receiver.records_root;
    let trial = root.join("new-policy");
    crate::method_adoption::tests::copy_dir(&root.join("vs-fixed-cart"), &trial);
    let mut plan: TrialPlan =
        serde_json::from_slice(&std::fs::read(trial.join("plan.json")).unwrap()).unwrap();
    plan.id = "receiver-policy-v2".into();
    plan.scoring_grant.wall_time_ms += 1;
    // Synthetic fixtures, not new execution evidence: rebind both preexisting
    // receipt shapes to a fresh valid plan to isolate the store's uniqueness.
    for slot in &plan.slots {
        let receipt_path = trial.join(&slot.slot_id).join("receipt.json");
        let mut receipt: ExecutionReceipt =
            serde_json::from_slice(&std::fs::read(&receipt_path).unwrap()).unwrap();
        receipt.binding = plan.expected_binding(&slot.slot_id).unwrap();
        receipt.binding_sha256 = receipt.binding.canonical_hash().unwrap();
        let score_path = trial
            .join(format!("{}-score", slot.slot_id))
            .join("evaluation.json");
        let mut scoring: EvaluationReceipt =
            serde_json::from_slice(&std::fs::read(&score_path).unwrap()).unwrap();
        scoring.request = plan
            .evaluation_request(&slot.slot_id, &receipt)
            .unwrap()
            .unwrap();
        scoring.request_sha256 = scoring
            .request
            .canonical_hash(&receipt, &receipt.binding)
            .unwrap();
        std::fs::write(receipt_path, serde_json::to_vec(&receipt).unwrap()).unwrap();
        std::fs::write(score_path, serde_json::to_vec(&scoring).unwrap()).unwrap();
    }
    std::fs::write(trial.join("plan.json"), serde_json::to_vec(&plan).unwrap()).unwrap();
    f.records.receiver.acceptance_plans[0].plan = plan.reference().unwrap();
    let state = f.store.current_state().unwrap();
    assert!(authorize_method_invocation(
        &f.store,
        &state,
        &f.records.receiver,
        &invocation(&f),
        f.now
    )
    .is_err());
    let selected = SelectedMethod {
        publisher: f.intent.publisher.clone(),
        reference: f.intent.reference.clone(),
        package_json: package(),
    };
    assert!(matches!(
        assess_selected(&f.records.receiver, &state.org_id, &selected, "new-policy")
            .unwrap()
            .acceptance,
        Acceptance::Accepted { .. }
    ));
    let intent = CreateMethodAdoption {
        client_request_id: "new-policy-adoption".into(),
        decision: MethodDecision::Adopt {
            trial: "new-policy".into(),
        },
        ..f.intent.clone()
    };
    let result = f.create(&intent);
    assert!(
        result.is_ok(),
        "the fenced old policy must not block a valid new acceptance: {result:?}"
    );
    assert_ne!(result.unwrap().0.id, old.id);
}
