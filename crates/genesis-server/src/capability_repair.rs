//! Failure-backed repair of a locally generated, installed capability version.
//! The invocation verifier owns failure evidence; the acquisition journal owns
//! every provider effect. This module only authenticates and freezes their link.
use crate::app::AppState;
use crate::capability_acquisition_store::{
    create_repair_acquisition_in_tx, load_record, AcquisitionRecord, CreateAcquisition, RepairSeed,
};
use crate::capability_invocation::{self, InvocationDecision, InvocationProof};
use crate::error::{ApiError, ApiResult};
use crate::routes::acquisitions::{configured, runtime_error, store_error};
use crate::store::{Store, StoreResult};
use axum::http::StatusCode;
use chrono::{DateTime, Utc};
use genesis_core::ids::JobId;
use genesis_forge::RepairFeedback;
use genesis_verification::{decode_json, json_sha256, ArtifactPin, IssueCode};
use rusqlite::{params, OptionalExtension, TransactionBehavior};
use serde::{Deserialize, Serialize};

const MAX_LINK_BYTES: usize = 64 * 1024;

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CreateCapabilityRepair {
    pub parent_acquisition_id: String,
    pub failure_job_id: JobId,
    pub acquisition: CreateAcquisition,
}

/// Public relationship metadata, never source code, answers or a dispatch grant.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CapabilityRepair {
    pub acquisition_id: String,
    pub parent_acquisition_id: String,
    pub failure_job_id: JobId,
    pub failure_proof_ref: String,
    pub parent_artifact: ArtifactPin,
    pub deadline: DateTime<Utc>,
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct StoredRepair {
    schema_version: u32,
    client_request_id: String,
    request_sha256: String,
    origin_sha256: String,
    link: CapabilityRepair,
    binding_sha256: String,
}
impl StoredRepair {
    fn digest(&self) -> ApiResult<String> {
        hash(&(
            "genesis-capability-repair-link-v1",
            self.schema_version,
            &self.client_request_id,
            &self.request_sha256,
            &self.origin_sha256,
            &self.link,
        ))
    }
    fn decode(raw: &str) -> ApiResult<Self> {
        if raw.len() > MAX_LINK_BYTES {
            return Err(invalid_record());
        }
        let stored: Self = decode_json(raw.as_bytes()).map_err(|_| invalid_record())?;
        if stored.schema_version != 1 || stored.binding_sha256 != stored.digest()? {
            return Err(invalid_record());
        }
        Ok(stored)
    }
    fn validate_child(&self, child: &AcquisitionRecord) -> ApiResult<()> {
        if self.link.acquisition_id != child.id()
            || self.link.deadline != child.binding().deadline
            || child
                .session()
                .deployed_repair_origin_sha256()
                .map_err(|_| invalid_record())?
                .as_deref()
                != Some(self.origin_sha256.as_str())
        {
            return Err(invalid_record());
        }
        Ok(())
    }
}

impl Store {
    pub fn init_capability_repairs(&self) -> StoreResult<()> {
        self.lock().execute_batch(
            "CREATE TABLE IF NOT EXISTS capability_repairs (
                parent_artifact_sha256 TEXT PRIMARY KEY,
                client_request_id TEXT NOT NULL UNIQUE,
                failure_job_id TEXT NOT NULL UNIQUE,
                acquisition_id TEXT NOT NULL UNIQUE,
                link_json TEXT NOT NULL
            );",
        )?;
        Ok(())
    }
}

struct ResolvedFailure {
    parent: AcquisitionRecord,
    proof: InvocationProof,
    artifact: ArtifactPin,
    feedback: Vec<RepairFeedback>,
}

// Caller holds the kernel lock. No network calls or new external effects.
fn resolve_failure(
    app: &AppState,
    parent_id: &str,
    failure_job: &JobId,
) -> ApiResult<ResolvedFailure> {
    if parent_id.is_empty() || parent_id.len() > 128 || failure_job.as_str().len() > 256 {
        return Err(ineligible());
    }
    let proof = capability_invocation::read_locked(app, failure_job)?;
    if proof.decision != InvocationDecision::SemanticRejected {
        return Err(ineligible());
    }
    let report = proof.case_report.as_ref().ok_or_else(ineligible)?;
    let feedback = semantic_feedback(&report.issues).ok_or_else(ineligible)?;
    let parent = app
        .acquisition_runtime
        .status(parent_id)
        .map_err(runtime_error)?;
    if parent.binding().org_id != app.org_id || parent.suite_id() != proof.suite_id {
        return Err(ineligible());
    }
    let suite = app
        .evidence_policy
        .suite(parent.suite_id())
        .ok_or_else(ineligible)?;
    // Historical source remains usable after its builder is retired. A fresh
    // replacement must acquire its own current authority in the transaction.
    let verified = parent.session().verified(suite).map_err(|_| ineligible())?;
    if verified.artifact != report.artifact {
        return Err(ineligible());
    }
    Ok(ResolvedFailure {
        parent,
        proof,
        artifact: verified.artifact,
        feedback,
    })
}

/// One replacement attempt per exact parent artifact in this organization.
/// Replayed keys return the durable child even after a restart or authority loss;
/// stepping that child still rechecks all current authority and count limits.
pub async fn create(
    app: &AppState,
    request: &CreateCapabilityRepair,
) -> ApiResult<(CapabilityRepair, bool)> {
    let policy = configured(app)?;
    let _kernel = app.kernel.lock().await;
    let resolved = resolve_failure(app, &request.parent_acquisition_id, &request.failure_job_id)?;
    if request.acquisition.suite_id != resolved.parent.suite_id()
        || request.acquisition.repair.is_some()
    {
        return Err(ineligible());
    }
    let suite = app
        .evidence_policy
        .suite(resolved.parent.suite_id())
        .ok_or_else(ineligible)?;
    let request_sha256 = hash(request)?;
    let artifact_sha256 = hash(&resolved.artifact)?;
    let mut conn = app.store.lock();
    let tx = conn
        .transaction_with_behavior(TransactionBehavior::Immediate)
        .map_err(db_error)?;
    let existing: Option<String> = tx.query_row(
        "SELECT link_json FROM capability_repairs WHERE client_request_id=?1 OR parent_artifact_sha256=?2 OR failure_job_id=?3",
        params![request.acquisition.client_request_id, artifact_sha256, request.failure_job_id.as_str()],
        |row| row.get(0),
    ).optional().map_err(db_error)?;
    if let Some(raw) = existing {
        let stored = StoredRepair::decode(&raw)?;
        if stored.client_request_id != request.acquisition.client_request_id
            || stored.request_sha256 != request_sha256
        {
            return Err(ApiError::new(
                StatusCode::CONFLICT,
                "capability_repair_conflict",
                "this request key, failure or exact parent artifact already has a repair attempt",
            ));
        }
        validate_resolved(&stored.link, &resolved)?;
        let child = load_record(&tx, &stored.link.acquisition_id, suite).map_err(store_error)?;
        stored.validate_child(&child)?;
        tx.commit().map_err(db_error)?;
        return Ok((stored.link, false));
    }
    let seed = RepairSeed {
        parent: resolved.parent.session(),
        failure_ref: &resolved.proof.proof_ref,
        feedback: &resolved.feedback,
    };
    let (child, created) = create_repair_acquisition_in_tx(
        &tx,
        &request.acquisition,
        policy,
        suite,
        Utc::now(),
        &seed,
    )
    .map_err(store_error)?;
    if !created {
        // A valid repair child and its link commit together. An independently
        // existing acquisition must never be retroactively relabeled a repair.
        return Err(invalid_record());
    }
    let link = CapabilityRepair {
        acquisition_id: child.id().into(),
        parent_acquisition_id: request.parent_acquisition_id.clone(),
        failure_job_id: request.failure_job_id.clone(),
        failure_proof_ref: resolved.proof.proof_ref,
        parent_artifact: resolved.artifact,
        deadline: child.binding().deadline,
    };
    let mut stored = StoredRepair {
        schema_version: 1,
        client_request_id: request.acquisition.client_request_id.clone(),
        request_sha256,
        origin_sha256: child
            .session()
            .deployed_repair_origin_sha256()
            .map_err(|_| invalid_record())?
            .ok_or_else(invalid_record)?,
        link,
        binding_sha256: String::new(),
    };
    stored.binding_sha256 = stored.digest()?;
    let raw = serde_json::to_string(&stored).map_err(|_| invalid_record())?;
    if raw.len() > MAX_LINK_BYTES {
        return Err(invalid_record());
    }
    tx.execute(
        "INSERT INTO capability_repairs(parent_artifact_sha256,client_request_id,failure_job_id,acquisition_id,link_json) VALUES(?1,?2,?3,?4,?5)",
        params![artifact_sha256,stored.client_request_id,stored.link.failure_job_id.as_str(),child.id(),raw],
    ).map_err(db_error)?;
    tx.commit().map_err(db_error)?;
    Ok((stored.link, true))
}

pub async fn read(app: &AppState, acquisition_id: &str) -> ApiResult<CapabilityRepair> {
    configured(app)?;
    let _kernel = app.kernel.lock().await;
    let raw: Option<String> = app
        .store
        .lock()
        .query_row(
            "SELECT link_json FROM capability_repairs WHERE acquisition_id=?1",
            params![acquisition_id],
            |row| row.get(0),
        )
        .optional()
        .map_err(db_error)?;
    let stored =
        StoredRepair::decode(&raw.ok_or_else(|| ApiError::not_found("capability repair"))?)?;
    if stored.link.acquisition_id != acquisition_id {
        return Err(invalid_record());
    }
    let resolved = resolve_failure(
        app,
        &stored.link.parent_acquisition_id,
        &stored.link.failure_job_id,
    )?;
    validate_resolved(&stored.link, &resolved)?;
    let child = app
        .acquisition_runtime
        .status(acquisition_id)
        .map_err(runtime_error)?;
    stored.validate_child(&child)?;
    Ok(stored.link)
}

fn validate_resolved(link: &CapabilityRepair, resolved: &ResolvedFailure) -> ApiResult<()> {
    if link.parent_acquisition_id != resolved.parent.id()
        || link.failure_job_id != resolved.proof.job_id
        || link.failure_proof_ref != resolved.proof.proof_ref
        || link.parent_artifact != resolved.artifact
    {
        return Err(invalid_record());
    }
    Ok(())
}

fn semantic_feedback(issues: &[IssueCode]) -> Option<Vec<RepairFeedback>> {
    if issues.is_empty() {
        return None;
    }
    issues
        .iter()
        .map(|issue| {
            Some(match issue {
                IssueCode::MalformedOutput => RepairFeedback::MalformedOutput,
                IssueCode::InvalidOutput => RepairFeedback::InvalidOutput,
                IssueCode::OutcomeMismatch => RepairFeedback::OutcomeMismatch,
                IssueCode::MissingVenue => RepairFeedback::MissingVenue,
                IssueCode::UnexpectedVenue => RepairFeedback::UnexpectedVenue,
                IssueCode::DuplicateVenue => RepairFeedback::DuplicateVenue,
                IssueCode::ValueMismatch => RepairFeedback::ValueMismatch,
                _ => return None,
            })
        })
        .collect::<Option<std::collections::BTreeSet<_>>>()
        .map(|issues| issues.into_iter().collect())
}
fn hash(value: &impl Serialize) -> ApiResult<String> {
    Ok(json_sha256(
        &serde_json::to_value(value).map_err(|_| invalid_record())?,
    ))
}
fn db_error(_: rusqlite::Error) -> ApiError {
    ApiError::internal("capability repair persistence failed")
}
fn invalid_record() -> ApiError {
    ApiError::new(
        StatusCode::CONFLICT,
        "capability_repair_binding",
        "capability repair binding failed validation",
    )
}
fn ineligible() -> ApiError {
    ApiError::new(StatusCode::UNPROCESSABLE_ENTITY, "invalid_capability_repair",
        "repair requires a semantic invocation failure of the exact locally generated parent and its unchanged oracle suite")
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn only_closed_semantic_vocabulary_can_seed_repair() {
        let semantic = [
            IssueCode::MalformedOutput,
            IssueCode::InvalidOutput,
            IssueCode::OutcomeMismatch,
            IssueCode::MissingVenue,
            IssueCode::UnexpectedVenue,
            IssueCode::DuplicateVenue,
            IssueCode::ValueMismatch,
        ];
        assert_eq!(semantic_feedback(&semantic).unwrap().len(), 7);
        assert!(semantic_feedback(&[]).is_none());
        for forbidden in [
            IssueCode::MissingCase,
            IssueCode::DuplicateCase,
            IssueCode::UnknownCase,
            IssueCode::ReusedReceipt,
            IssueCode::InvalidReceipt,
            IssueCode::ArtifactMismatch,
            IssueCode::InputMismatch,
            IssueCode::SourceMismatch,
            IssueCode::RunNotSucceeded,
        ] {
            assert!(semantic_feedback(&[IssueCode::ValueMismatch, forbidden]).is_none());
        }
        assert_eq!(
            semantic_feedback(&[
                IssueCode::ValueMismatch,
                IssueCode::MissingVenue,
                IssueCode::ValueMismatch
            ])
            .unwrap(),
            vec![RepairFeedback::MissingVenue, RepairFeedback::ValueMismatch]
        );
    }
}
