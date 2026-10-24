//! Durable, requirement-bound independent adoption. Discovery is a bounded
//! registry read; paid execution stays in the existing capability runtime.
use crate::app::AppState;
use crate::capability_adoption_candidates::{
    self as candidates, CandidateDecision, CandidateDecisionKind as Decided,
};
use crate::capability_adoption_store::{
    self as storage, AdoptionAttention, AdoptionError, AdoptionPhase, AdoptionRecord,
};
use crate::capability_evidence::EvidencePolicyError;
use crate::capability_runtime::{CapabilityRunPhase, CapabilityRunRecord, ExecutionPurpose};
use crate::capability_store::{self, LocalAcceptance};
use crate::error::{ApiError, ApiResult};
use crate::registry::{SearchCandidate, SearchRequest};
use crate::routes::capabilities::{self, ChecksBody};
use crate::store::{EventMeta, Store};
use axum::http::StatusCode;
use chrono::{Duration, Utc};
use genesis_core::capability::CapabilityManifest;
use genesis_core::engine;
use genesis_core::jobs::Job;
use genesis_core::lifecycle::InstallationStatus;
use genesis_core::model::{OrgState, Provider};
use genesis_verification::json_sha256;
use serde_json::json;

pub(crate) fn error(error: AdoptionError) -> ApiError {
    let (status, code, message) = match error {
        AdoptionError::InvalidInput => (
            StatusCode::BAD_REQUEST,
            "invalid_adoption",
            "invalid adoption request",
        ),
        AdoptionError::NotFound => (StatusCode::NOT_FOUND, "not_found", "adoption not found"),
        AdoptionError::IdempotencyConflict => (
            StatusCode::CONFLICT,
            "idempotency_conflict",
            "adoption key is bound to another intent",
        ),
        AdoptionError::ActiveConflict => (
            StatusCode::CONFLICT,
            "active_adoption",
            "this assignment already has active adoption work",
        ),
        AdoptionError::Limit => (
            StatusCode::CONFLICT,
            "adoption_limit",
            "adoption count limit reached",
        ),
        AdoptionError::Authority(_) => (
            StatusCode::CONFLICT,
            "adoption_authority",
            "adoption authority changed or expired",
        ),
        AdoptionError::Conflict => (
            StatusCode::CONFLICT,
            "adoption_conflict",
            "adoption changed concurrently",
        ),
        _ => (
            StatusCode::INTERNAL_SERVER_ERROR,
            "adoption_state",
            "adoption state is unavailable",
        ),
    };
    ApiError::new(status, code, message)
}

fn internal() -> ApiError {
    ApiError::internal("adoption state is unavailable")
}

pub(crate) fn configured(app: &AppState) -> ApiResult<()> {
    if app.registry.is_none()
        || !app
            .providers
            .executor()
            .is_ok_and(|p| p.provider() == Provider::Apify)
    {
        return Err(ApiError::new(
            StatusCode::SERVICE_UNAVAILABLE,
            "adoption_configuration",
            "adoption requires a registry and Apify executor",
        ));
    }
    Ok(())
}

/// Child requests bind their adoption identity in the immutable fingerprint.
/// Recheck the current requirement, assignment, permissions and deadline before
/// every provider operation and again before retaining its result.
pub(crate) fn authorized_job(
    store: &Store,
    policy: &crate::capability_evidence::EvidencePolicy,
    state: &OrgState,
    job: &Job,
    run: &CapabilityRunRecord,
) -> bool {
    let ExecutionPurpose::AdoptionAcceptance {
        adoption_id,
        installation_id,
        suite_id,
    } = &run.purpose
    else {
        return false;
    };
    let Ok(record) = store.get_adoption(adoption_id) else {
        return false;
    };
    record.phase() == AdoptionPhase::Checking
        && policy
            .suite(record.suite_id())
            .is_some_and(|suite| record.suite_matches(suite).is_ok())
        && record.installation_id() == Some(installation_id)
        && record.suite_id() == suite_id
        && record.job_ids().contains(&job.job_id)
        && record.binding().role_id == job.role_id
        && record.binding().responsibility_id == job.responsibility_id
        && record.binding().grant.assignment_id == job.assignment_id
        && record.binding().grant.epoch == job.epoch
        && job.deadline == Some(record.binding().deadline)
        && record.selection().is_some_and(|manifest| {
            capabilities::artifact(manifest) == run.request.artifact
                && manifest.required_permissions == run.required_permissions
        })
        && record.authorize(state, Utc::now()).is_ok()
        && state
            .installations
            .get(installation_id)
            .is_some_and(|installation| {
                installation.status == InstallationStatus::Checking
                    && job.capability.as_ref() == Some(&installation.capability)
                    && installation.source_digest == run.request.artifact.source_sha256
            })
}

fn save(
    app: &AppState,
    state: &OrgState,
    previous: &AdoptionRecord,
    next: AdoptionRecord,
) -> ApiResult<AdoptionRecord> {
    let next = next.for_revision(state.revision).map_err(error)?;
    app.store
        .commit_state(
            state.revision,
            state,
            vec![],
            &EventMeta::default(),
            vec![],
            |tx| storage::save_in_tx(tx, previous, &next),
        )
        .map_err(|_| internal())?;
    Ok(next)
}

fn attention(
    app: &AppState,
    state: &OrgState,
    record: &AdoptionRecord,
    code: AdoptionAttention,
) -> ApiResult<AdoptionRecord> {
    stop(
        app,
        state,
        record,
        record.attention(code, Utc::now()).map_err(error)?,
    )
}

/// A discovery that ends without a selection: the attention code and the
/// candidate decisions that explain it are committed together. Discovery
/// never has an installation yet, so this is a plain save.
fn stop_discovery(
    app: &AppState,
    state: &OrgState,
    record: &AdoptionRecord,
    code: AdoptionAttention,
    decisions: &[CandidateDecision],
) -> ApiResult<AdoptionRecord> {
    if record.installation_id().is_some() {
        return Err(internal());
    }
    let next = record
        .attention(code, Utc::now())
        .map_err(error)?
        .for_revision(state.revision)
        .map_err(error)?;
    app.store
        .commit_state(
            state.revision,
            state,
            vec![],
            &EventMeta::default(),
            vec![],
            |tx| {
                storage::save_in_tx(tx, record, &next)?;
                candidates::insert_in_tx(tx, decisions)
            },
        )
        .map_err(|_| internal())?;
    Ok(next)
}

/// Terminal work must not strand an unaccepted installation in Checking. This
/// records an operational stop, not a semantic verdict about the parser. The
/// old child tickets remain available; a later explicit intent is separate work.
fn stop(
    app: &AppState,
    state: &OrgState,
    previous: &AdoptionRecord,
    next: AdoptionRecord,
) -> ApiResult<AdoptionRecord> {
    let Some(id) = previous.installation_id() else {
        return save(app, state, previous, next);
    };
    let installation = state.installations.get(id).ok_or_else(internal)?;
    let manifest = previous.selection().ok_or_else(internal)?;
    if installation.capability != manifest.reference()
        || installation.source_digest != manifest.source_digest
        || !app.store.adoption_owns_installation(id).map_err(error)?
        || app.store.capability_manifest(id).map_err(|_| internal())? != *manifest
    {
        return Err(internal());
    }
    if installation.status != InstallationStatus::Checking {
        return save(app, state, previous, next);
    }
    let reference = format!("adoption-stopped:{}:{}", previous.id(), next.version());
    let outcome = engine::finish_installation(
        state,
        id,
        false,
        &reference,
        vec!["Verification stopped without an acceptance decision; retained provider effects may still exist".to_owned()],
    ).map_err(|_| internal())?;
    let next = next.for_revision(outcome.state.revision).map_err(error)?;
    let events = app
        .store
        .commit_state(
            state.revision,
            &outcome.state,
            outcome.effects,
            &EventMeta::default(),
            vec![],
            |tx| storage::save_in_tx(tx, previous, &next),
        )
        .map_err(|_| internal())?;
    app.broadcast(&events);
    Ok(next)
}

/// Returns a fenced or paused record, or None when current work is authorized.
fn reconcile(
    app: &AppState,
    state: &OrgState,
    record: &AdoptionRecord,
) -> ApiResult<Option<AdoptionRecord>> {
    if matches!(
        record.phase(),
        AdoptionPhase::Accepted
            | AdoptionPhase::Rejected
            | AdoptionPhase::Attention
            | AdoptionPhase::Fenced
    ) {
        return Ok(Some(record.clone()));
    }
    if let Err(failure) = record.authorize(state, Utc::now()) {
        return match failure {
            AdoptionError::Authority(reason) => Ok(Some(stop(
                app,
                state,
                record,
                record.fence(reason, Utc::now()).map_err(error)?,
            )?)),
            other => Err(error(other)),
        };
    }
    let valid = app
        .evidence_policy
        .suite(record.suite_id())
        .is_some_and(|suite| record.suite_matches(suite).is_ok());
    if !valid || configured(app).is_err() {
        return Ok(Some(attention(
            app,
            state,
            record,
            AdoptionAttention::ConfigurationChanged,
        )?));
    }
    Ok(None)
}

fn candidate_matches(candidate: &SearchCandidate, manifest: &CapabilityManifest) -> bool {
    candidate.capability_id == manifest.capability_id
        && candidate.version == manifest.version
        && candidate.publisher == manifest.publisher
        && candidate.source_digest == manifest.source_digest
        && candidate
            .required_permissions
            .iter()
            .collect::<std::collections::BTreeSet<_>>()
            == manifest.required_permissions.iter().collect()
}

/// Outcome of one registry read: what was decided about each candidate (in
/// registry rank order), and either the selected manifest (with its rank),
/// a definitive empty answer, or why discovery stopped.
struct Discovery {
    decisions: Vec<CandidateDecision>,
    result: Result<Option<(usize, CapabilityManifest)>, AdoptionAttention>,
}

async fn discover(app: &AppState, record: &AdoptionRecord, scopes: Vec<String>) -> Discovery {
    let mut decisions = Vec::new();
    let result = discover_into(app, record, scopes, &mut decisions).await;
    Discovery { decisions, result }
}

/// Rank and score only order candidates; each one is still checked here
/// (publisher, permissions, manifest identity and state) and only this
/// organization's acceptance suite can make it applicable later.
async fn discover_into(
    app: &AppState,
    record: &AdoptionRecord,
    scopes: Vec<String>,
    decisions: &mut Vec<CandidateDecision>,
) -> Result<Option<(usize, CapabilityManifest)>, AdoptionAttention> {
    let registry = app
        .registry
        .as_ref()
        .ok_or(AdoptionAttention::ConfigurationChanged)?;
    let candidates = registry
        .search(&SearchRequest {
            query: record.query().to_owned(),
            limit: 5,
            available_scopes: Some(scopes.clone()),
        })
        .await
        .map_err(|_| AdoptionAttention::RegistryUnavailable)?;
    let now = Utc::now();
    let id = record.id();
    if candidates.len() > 5
        || candidates
            .iter()
            .any(|c| !c.score.is_finite() || c.score <= 0.0)
    {
        // A malformed answer is evaluated as a whole: nothing in it is used.
        decisions.extend(candidates.iter().take(5).enumerate().map(|(rank, c)| {
            CandidateDecision::new(
                id,
                rank,
                c,
                Decided::InvalidResponse,
                "the registry answer was invalid (too many candidates or an unusable score), so no candidate in it was evaluated",
                now,
            )
        }));
        return Err(AdoptionAttention::InvalidCandidate);
    }
    let mut outcome = Ok(None);
    for (rank, candidate) in candidates.iter().enumerate() {
        if !matches!(outcome, Ok(None)) {
            decisions.push(CandidateDecision::new(
                id,
                rank,
                candidate,
                Decided::NotEvaluated,
                match &outcome {
                    Ok(Some(_)) => "a higher-ranked candidate was selected first",
                    _ => "discovery stopped at a higher-ranked candidate",
                },
                now,
            ));
            continue;
        }
        if candidate.publisher == app.org_id {
            decisions.push(CandidateDecision::new(
                id,
                rank,
                candidate,
                Decided::OwnPublication,
                "published by this organization; adoption only inherits from other organizations",
                now,
            ));
            continue;
        }
        let missing: Vec<String> = candidate
            .required_permissions
            .iter()
            .filter(|scope| !scopes.contains(scope))
            .cloned()
            .collect();
        if !missing.is_empty() {
            decisions.push(
                CandidateDecision::new(
                    id,
                    rank,
                    candidate,
                    Decided::MissingScope,
                    format!(
                        "needs permissions the requesting role doesn't hold: {}",
                        missing.join(", ")
                    ),
                    now,
                )
                .with_missing(missing),
            );
            continue;
        }
        let manifest = match registry
            .manifest(&candidate.capability_id, &candidate.version)
            .await
        {
            Ok(manifest) => manifest,
            Err(_) => {
                decisions.push(CandidateDecision::new(
                    id,
                    rank,
                    candidate,
                    Decided::ManifestUnavailable,
                    "its manifest couldn't be read, so its applicability is unknown",
                    now,
                ));
                outcome = Err(AdoptionAttention::RegistryUnavailable);
                continue;
            }
        };
        let (decided, reason) = if !candidate_matches(candidate, &manifest)
            || capabilities::check_manifest(app, &manifest).is_err()
        {
            (
                Decided::InvalidManifest,
                "its manifest disagrees with the search answer or fails local manifest checks",
            )
        } else if !manifest.state.is_usable() {
            (
                Decided::UnusableState,
                "its manifest is not in an installable state",
            )
        } else {
            decisions.push(CandidateDecision::new(
                id,
                rank,
                candidate,
                Decided::Selected,
                "first eligible candidate by registry rank; selected for this organization's own acceptance checks",
                now,
            ));
            outcome = Ok(Some((rank, manifest)));
            continue;
        };
        decisions.push(CandidateDecision::new(
            id, rank, candidate, decided, reason, now,
        ));
        outcome = Err(AdoptionAttention::InvalidCandidate);
    }
    outcome
}

/// Performs one durable transition. Repeated calls cannot duplicate the selected
/// installation or acceptance batch; provider submissions are never made here.
pub async fn step(app: &AppState, id: &str) -> ApiResult<AdoptionRecord> {
    let _gate = app.adoption_gate.lock().await;
    let (mut record, scopes) = {
        let _kernel = app.kernel.lock().await;
        let record = app.store.get_adoption(id).map_err(error)?;
        let state = app.current_state().map_err(|_| internal())?;
        if let Some(record) = reconcile(app, &state, &record)? {
            return Ok(record);
        }
        if record.phase() != AdoptionPhase::Discovering {
            return advance_local(app, &state, &record);
        }
        if record.discovery_attempts() >= 3 {
            return attention(app, &state, &record, AdoptionAttention::RegistryUnavailable);
        }
        let scopes = state.roles[&record.binding().role_id]
            .authority
            .scopes
            .iter()
            .filter(|scope| state.constitution.scopes.contains(*scope))
            .cloned()
            .collect();
        let next = record.begin_discovery(Utc::now()).map_err(error)?;
        (save(app, &state, &record, next)?, scopes)
    };
    let Discovery {
        mut decisions,
        result,
    } = discover(app, &record, scopes).await;
    let _kernel = app.kernel.lock().await;
    let current = app.store.get_adoption(id).map_err(error)?;
    if current.version() != record.version() {
        return Err(error(AdoptionError::Conflict));
    }
    record = current;
    let state = app.current_state().map_err(|_| internal())?;
    if let Some(record) = reconcile(app, &state, &record)? {
        return Ok(record);
    }
    let (selected_rank, manifest) = match result {
        Ok(Some(selected)) => selected,
        Ok(None) => {
            return stop_discovery(
                app,
                &state,
                &record,
                AdoptionAttention::NoCandidate,
                &decisions,
            )
        }
        Err(code) => return stop_discovery(app, &state, &record, code, &decisions),
    };
    if capabilities::authorize_permissions(
        &state,
        &record.binding().role_id,
        &record.binding().responsibility_id,
        &manifest,
    )
    .is_err()
    {
        if let Some(d) = decisions
            .iter_mut()
            .find(|d| d.rank as usize == selected_rank)
        {
            d.decision = Decided::MissingScope;
            d.reason =
                "the requesting role's current authority doesn't cover its manifest permissions"
                    .into();
        }
        return stop_discovery(
            app,
            &state,
            &record,
            AdoptionAttention::InvalidCandidate,
            &decisions,
        );
    }
    let (outcome, installation_id) =
        match engine::begin_installation(&state, &manifest, json!({}), Utc::now()) {
            Ok(value) => value,
            Err(_) => {
                return attention(
                    app,
                    &state,
                    &record,
                    AdoptionAttention::InstallationConflict,
                )
            }
        };
    let next = record
        .select(manifest.clone(), installation_id.clone(), Utc::now())
        .map_err(error)?
        .for_revision(outcome.state.revision)
        .map_err(error)?;
    let events = app
        .store
        .commit_state(
            state.revision,
            &outcome.state,
            outcome.effects,
            &EventMeta::default(),
            vec![],
            |tx| {
                capability_store::insert_pin(tx, &installation_id, &manifest)?;
                storage::save_in_tx(tx, &record, &next)?;
                candidates::insert_in_tx(tx, &decisions)
            },
        )
        .map_err(|_| internal())?;
    app.broadcast(&events);
    Ok(next)
}

fn advance_local(
    app: &AppState,
    state: &OrgState,
    record: &AdoptionRecord,
) -> ApiResult<AdoptionRecord> {
    let installation_id = record.installation_id().ok_or_else(internal)?;
    let manifest = record.selection().ok_or_else(internal)?;
    if app
        .store
        .capability_manifest(installation_id)
        .map_err(|_| internal())?
        != *manifest
    {
        return attention(app, state, record, AdoptionAttention::InvalidCandidate);
    }
    match record.phase() {
        AdoptionPhase::Selected => {
            let body = ChecksBody {
                client_request_id: format!("adoption:{}", record.id()),
                role_id: record.binding().role_id.clone(),
                responsibility_id: record.binding().responsibility_id.clone(),
                suite_id: record.suite_id().to_owned(),
            };
            let plan = match capabilities::plan_checks(
                app,
                state,
                installation_id,
                &body,
                Some((record.id(), record.binding().deadline)),
            ) {
                Ok(plan) => plan,
                Err(_) => return attention(app, state, record, AdoptionAttention::ChecksFailed),
            };
            let next = record
                .checking(plan.job_ids.clone(), Utc::now())
                .map_err(error)?
                .for_revision(state.revision)
                .map_err(error)?;
            let events = app
                .store
                .commit_state(
                    state.revision,
                    state,
                    plan.effects.clone(),
                    &EventMeta::default(),
                    vec![],
                    |tx| {
                        capabilities::persist_checks(tx, &plan)?;
                        storage::save_in_tx(tx, record, &next)
                    },
                )
                .map_err(|_| internal())?;
            app.broadcast(&events);
            Ok(next)
        }
        AdoptionPhase::Checking => finalize(app, state, record),
        _ => Err(internal()),
    }
}

fn finalize(
    app: &AppState,
    state: &OrgState,
    record: &AdoptionRecord,
) -> ApiResult<AdoptionRecord> {
    let installation_id = record.installation_id().ok_or_else(internal)?;
    let manifest = record.selection().ok_or_else(internal)?;
    let mut pending = false;
    for id in record.job_ids() {
        let run = app.store.get_capability_run(id).map_err(|_| internal())?;
        let job = app.store.get_job(id).map_err(|_| internal())?;
        run.validate(&job).map_err(|_| internal())?;
        if !authorized_job(&app.store, &app.evidence_policy, state, &job, &run) {
            return attention(app, state, record, AdoptionAttention::ChecksFailed);
        }
        if run.failure.is_some()
            || matches!(
                run.phase,
                CapabilityRunPhase::Failed
                    | CapabilityRunPhase::NeedsReconciliation
                    | CapabilityRunPhase::Rejected
            )
        {
            return attention(app, state, record, AdoptionAttention::ChecksFailed);
        }
        pending |= run.phase != CapabilityRunPhase::Collected;
    }
    if pending {
        let now = Utc::now();
        return save(
            app,
            state,
            record,
            record
                .wait_until(
                    (now + Duration::seconds(1)).min(record.binding().deadline),
                    now,
                )
                .map_err(error)?,
        );
    }
    let runs = capabilities::local_runs(app, installation_id, record.suite_id(), record.job_ids())
        .map_err(|_| internal())?;
    let mut rejection: Option<String> = None;
    let (passed, reference, proof) = match app.evidence_policy.accept_suite(
        record.suite_id(),
        &capabilities::artifact(manifest),
        &runs,
    ) {
        Ok(evidence) => {
            let proof = LocalAcceptance {
                suite_id: record.suite_id().to_owned(),
                job_ids: record.job_ids().to_vec(),
                report_ref: evidence.reference.clone(),
            };
            (true, evidence.reference, Some(proof))
        }
        Err(EvidencePolicyError::Rejected(report)) => {
            let reference = format!(
                "adoption-rejection:{}",
                json_sha256(&json!({"adoption_id":record.id(),"report":report}))
            );
            rejection = Some(candidates::rejection_summary(&report));
            (false, reference, None)
        }
        Err(_) => return attention(app, state, record, AdoptionAttention::ConfigurationChanged),
    };
    let reasons = if passed {
        vec![]
    } else {
        vec!["Independent local verification rejected the capability".to_owned()]
    };
    let outcome = engine::finish_installation(state, installation_id, passed, &reference, reasons)
        .map_err(|_| internal())?;
    // An adopt-first gap intent that owns this adoption records its gap
    // installed in this same transaction (no-op for ordinary adoptions).
    let (outcome, gap_link) = if passed {
        crate::capability_gap_adoption::link_accepted(&app.store, outcome, record)
            .map_err(|_| internal())?
    } else {
        (outcome, None)
    };
    let next = if passed {
        record.accept(reference.clone(), Utc::now())
    } else {
        record.rejected(reference.clone(), Utc::now())
    }
    .map_err(error)?
    .for_revision(outcome.state.revision)
    .map_err(error)?;
    let events = app
        .store
        .commit_state(
            state.revision,
            &outcome.state,
            outcome.effects,
            &EventMeta::default(),
            vec![],
            |tx| {
                if let Some(proof) = &proof {
                    capability_store::save_acceptance(tx, installation_id, proof)?;
                }
                storage::save_in_tx(tx, record, &next)?;
                if passed {
                    candidates::conclude_in_tx(
                        tx,
                        record.id(),
                        Decided::Adopted,
                        "this organization's acceptance suite accepted it",
                        &reference,
                        Utc::now(),
                    )?;
                } else {
                    candidates::conclude_in_tx(
                        tx,
                        record.id(),
                        Decided::NotApplicable,
                        rejection
                            .as_deref()
                            .unwrap_or("this organization's acceptance suite rejected it"),
                        &reference,
                        Utc::now(),
                    )?;
                }
                match &gap_link {
                    Some(link) => crate::capability_gap_adoption::save_link(tx, link),
                    None => Ok(()),
                }
            },
        )
        .map_err(|_| internal())?;
    app.broadcast(&events);
    Ok(next)
}

pub(crate) fn spawn(app: AppState) -> tokio::task::JoinHandle<()> {
    tokio::spawn(async move {
        let mut interval = tokio::time::interval(std::time::Duration::from_millis(250));
        loop {
            interval.tick().await;
            let Ok(ids) = app.store.due_adoption_ids(Utc::now(), 4) else {
                tracing::warn!("adoption scan unavailable");
                continue;
            };
            for id in ids {
                if step(&app, &id).await.is_err() {
                    tracing::warn!(adoption_id = %id, "adoption transition unavailable");
                }
            }
        }
    })
}
