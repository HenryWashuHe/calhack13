//! Whether one role may invoke one exact method version for one
//! responsibility right now. Only an `adopted` record for exactly that
//! publisher and reference, bound to exactly that role and responsibility,
//! can authorize, and only while its requirement/owner grant is unchanged.
//! Another role's valid grant, a declined or rejected decision, or a newer
//! version of the same method authorizes nothing. This is a check, not an
//! execution: running the method under the grant is a separate step.
//!
//! The receiver's current policy also has to still stand behind the record:
//! the requirement's pin must name the same publisher and the exact plan the
//! adoption was decided under, and the trial's environment, evaluator and
//! dependencies must still be ones the receiver accepts. Tightening or
//! changing that configuration fences older decisions; nothing is rewritten.
use crate::capability_adoption_store::AdoptionFence;
use crate::method_adoption::MethodReceiver;
use crate::method_adoption_store::{MethodAdoptionError, MethodAdoptionRecord};
use crate::store::Store;
use chrono::{DateTime, Utc};
use genesis_core::development_method::ExactContentRef;
use genesis_core::engine::DispatchGrant;
use genesis_core::ids::{OrgId, RequirementId, ResponsibilityId, RoleId};
use genesis_core::model::OrgState;
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MethodInvocationRequest {
    pub role_id: RoleId,
    pub responsibility_id: ResponsibilityId,
    pub publisher: OrgId,
    pub reference: ExactContentRef,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MethodInvocationGrant {
    pub adoption_id: String,
    pub requirement_id: RequirementId,
    pub publisher: OrgId,
    pub reference: ExactContentRef,
    pub grant: DispatchGrant,
}

/// Whether the receiver's current configuration still accepts this record.
fn receiver_still_accepts(receiver: &MethodReceiver, record: &MethodAdoptionRecord) -> bool {
    let (Some(pin), Some(evidence)) = (
        receiver.acceptance_plan(&record.requirement_id),
        record.acceptance_evidence(),
    ) else {
        return false;
    };
    pin.publisher == record.publisher
        && pin.plan == evidence.plan
        && receiver.environments.contains(&evidence.environment)
        && receiver.evaluators.contains(&evidence.evaluator)
        && evidence
            .dependencies
            .iter()
            .all(|d| receiver.allowed_dependencies.contains(d))
}

pub fn authorize_method_invocation(
    store: &Store,
    state: &OrgState,
    receiver: &MethodReceiver,
    request: &MethodInvocationRequest,
    now: DateTime<Utc>,
) -> Result<MethodInvocationGrant, MethodAdoptionError> {
    if request.reference.validate().is_err() {
        return Err(MethodAdoptionError::InvalidInput);
    }
    let matching: Vec<MethodAdoptionRecord> = store
        .adopted_methods(&request.publisher, &request.reference.sha256)?
        .into_iter()
        .filter(|r| {
            r.reference == request.reference
                && r.role_id == request.role_id
                && r.responsibility_id == request.responsibility_id
        })
        .collect();
    let mut last = MethodAdoptionError::NotAdopted;
    for record in matching {
        if !receiver_still_accepts(receiver, &record) {
            last = MethodAdoptionError::Authority(AdoptionFence::AuthorityChanged);
            continue;
        }
        match record.authorize(state, now) {
            Ok(grant) => {
                return Ok(MethodInvocationGrant {
                    adoption_id: record.id,
                    requirement_id: record.requirement_id,
                    publisher: record.publisher,
                    reference: record.reference,
                    grant,
                })
            }
            Err(error) => last = error,
        }
    }
    Err(last)
}
