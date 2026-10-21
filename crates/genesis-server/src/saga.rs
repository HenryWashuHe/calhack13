//! Organization-change saga (TECH_SPEC §5).
//!
//! `PROPOSED -> VALIDATED -> PROVISIONING -> READY -> COMMITTED`, with
//! `REJECTED` and `FAILED` exits. External provisioning is not a cross-provider
//! transaction: each returned provider binding is persisted on the change
//! record before the saga advances, so a restart resumes instead of creating
//! duplicates. The organization itself changes only in the final commit.

use crate::app::AppState;
use crate::error::{validation_error, ApiError, ApiResult};
use crate::store::{put_change, EventMeta, NewOutbox};
use axum::http::StatusCode;
use chrono::Utc;
use genesis_core::change::{KeyAllocation, OrgChangeProposal, OrgChangeRecord};
use genesis_core::engine::{self, local_catalog, ValidationCode};
use genesis_core::events::EventKind;
use genesis_core::ids::ChangeId;
use genesis_core::lifecycle::{ChangeStatus, StateMachine};
use genesis_core::model::Provider;
use genesis_providers::ProvisionRequest;

/// Validate and record a proposal. Valid proposals start provisioning in the
/// background; the returned record is in `VALIDATED`.
pub async fn submit(
    app: &AppState,
    submitted_by: &str,
    client_request_id: &str,
    proposal: OrgChangeProposal,
) -> ApiResult<(StatusCode, OrgChangeRecord)> {
    if client_request_id.trim().is_empty() {
        return Err(ApiError::bad_request("client_request_id is required"));
    }
    let guard = app.kernel.lock().await;
    let state = app.current_state()?;
    let alloc = KeyAllocation::allocate(&proposal);
    let catalog = local_catalog(&state);
    let verdict = engine::validate(&state, &proposal, &alloc, &catalog, Utc::now());
    let now = Utc::now();
    let mut record = OrgChangeRecord {
        change_id: ChangeId::generate(),
        status: ChangeStatus::Proposed,
        proposal: proposal.clone(),
        allocation: alloc,
        provisioned: Default::default(),
        errors: vec![],
        committed_revision: None,
        submitted_by: submitted_by.to_owned(),
        created_at: now,
        updated_at: now,
    };
    let (stored, fresh) = app.store.insert_change(client_request_id, &record)?;
    if !fresh {
        // Repeated client request: return what we already have.
        return Ok((StatusCode::OK, stored));
    }
    let mut effects = vec![EventKind::ChangeProposed {
        change_id: record.change_id.clone(),
        expected_revision: proposal.expected_org_revision,
        operations: proposal
            .operations
            .iter()
            .map(|o| o.name().to_owned())
            .collect(),
        rationale: proposal.rationale.clone(),
    }];
    let next = match &verdict {
        Ok(_) => ChangeStatus::Validated,
        Err(_) => ChangeStatus::Rejected,
    };
    let reasons = verdict
        .as_ref()
        .err()
        .map(|errs| errs.iter().map(|e| e.to_string()).collect())
        .unwrap_or_default();
    effects.push(transition(&mut record, next, reasons)?);
    if let Err(errs) = &verdict {
        record.errors = errs.clone();
    }
    let events = app.store.append_events(
        effects,
        &EventMeta::default().caused_by(record.change_id.to_string()),
        |tx| put_change(tx, &record),
    )?;
    drop(guard);
    app.broadcast(&events);

    match verdict {
        Ok(_) => {
            let app2 = app.clone();
            let id = record.change_id.clone();
            tokio::spawn(async move {
                if let Err(e) = run(&app2, &id).await {
                    tracing::error!(change = %id, error = %e.message, "change saga failed");
                }
            });
            Ok((StatusCode::ACCEPTED, record))
        }
        Err(errs) => {
            let mut err = validation_error(errs.clone());
            err.details = serde_json::json!({ "errors": errs, "change_id": record.change_id });
            Err(err)
        }
    }
}

fn transition(
    record: &mut OrgChangeRecord,
    to: ChangeStatus,
    reasons: Vec<String>,
) -> ApiResult<EventKind> {
    let from = record.status;
    record.status = from
        .transition(to)
        .map_err(|e| ApiError::internal(e.to_string()))?;
    record.updated_at = Utc::now();
    Ok(EventKind::ChangeStatusChanged {
        change_id: record.change_id.clone(),
        from,
        to,
        reasons,
    })
}

fn save(app: &AppState, record: &OrgChangeRecord, effects: Vec<EventKind>) -> ApiResult<()> {
    let events = app.store.append_events(
        effects,
        &EventMeta::default().caused_by(record.change_id.to_string()),
        |tx| put_change(tx, record),
    )?;
    app.broadcast(&events);
    Ok(())
}

/// Drive a validated change to a terminal state. Safe to call again after a
/// restart: completed steps are skipped.
pub async fn run(app: &AppState, change_id: &ChangeId) -> ApiResult<OrgChangeRecord> {
    let mut record = app.store.get_change(change_id)?;
    if record.status == ChangeStatus::Validated {
        let e = transition(&mut record, ChangeStatus::Provisioning, vec![])?;
        save(app, &record, vec![e])?;
    }

    if record.status == ChangeStatus::Provisioning {
        let roles = engine::roles_to_provision(&record.proposal, &record.allocation);
        for role in roles {
            if record.provisioned.contains_key(&role.key) {
                continue; // already provisioned before a restart
            }
            let req = ProvisionRequest {
                org_id: app.org_id.clone(),
                change_id: record.change_id.clone(),
                role: role.clone(),
            };
            let result = async {
                let provisioner = app.providers.provisioner(role.provider)?;
                let binding =
                    tokio::time::timeout(app.provision_timeout, provisioner.provision(&req))
                        .await
                        .map_err(|_| genesis_providers::ProviderError::Timeout)??;
                let ready = provisioner.check_ready(&binding).await?;
                if !ready {
                    return Err(genesis_providers::ProviderError::Rejected(
                        "readiness check did not pass".into(),
                    ));
                }
                Ok(binding)
            }
            .await;
            match result {
                Ok(binding) => {
                    let ev = EventKind::RoleProvisioned {
                        change_id: record.change_id.clone(),
                        role_key: role.key.clone(),
                        role_id: role.role_id.clone(),
                        provider: role.provider,
                        provider_agent_id: binding.provider_agent_id.clone(),
                        dev_stub: binding.provider == Provider::DevStub,
                    };
                    record.provisioned.insert(role.key.clone(), binding);
                    record.updated_at = Utc::now();
                    save(app, &record, vec![ev])?;
                }
                Err(err) => {
                    let msg = err.to_string();
                    let failed = EventKind::ProvisioningFailed {
                        change_id: record.change_id.clone(),
                        role_key: role.key.clone(),
                        error: msg.clone(),
                    };
                    let reasons = vec![format!("provisioning `{}` failed: {msg}", role.key)];
                    let t = transition(&mut record, ChangeStatus::Failed, reasons)?;
                    save(app, &record, vec![failed, t])?;
                    return Ok(record);
                }
            }
        }
        let e = transition(&mut record, ChangeStatus::Ready, vec![])?;
        save(app, &record, vec![e])?;
    }

    if record.status == ChangeStatus::Ready {
        commit(app, &mut record).await?;
    }
    Ok(record)
}

async fn commit(app: &AppState, record: &mut OrgChangeRecord) -> ApiResult<()> {
    let _guard = app.kernel.lock().await;
    let state = app.current_state()?;
    let catalog = local_catalog(&state);
    let verdict = engine::commit(
        &state,
        &record.proposal,
        &record.allocation,
        &record.provisioned,
        &catalog,
        Utc::now(),
    );
    match verdict {
        Err(errs) => {
            let conflict = errs
                .iter()
                .any(|e| e.code == ValidationCode::RevisionConflict);
            let mut reasons: Vec<String> = errs.iter().map(|e| e.to_string()).collect();
            if !record.provisioned.is_empty() {
                reasons.push(format!(
                    "provider resources retained for cleanup: {}",
                    record
                        .provisioned
                        .values()
                        .map(|b| b.provider_agent_id.clone())
                        .collect::<Vec<_>>()
                        .join(", ")
                ));
            }
            let to = if conflict {
                ChangeStatus::Rejected
            } else {
                ChangeStatus::Failed
            };
            record.errors = errs;
            let t = transition(record, to, reasons)?;
            save(app, record, vec![t])?;
            Ok(())
        }
        Ok(outcome) => {
            let new_revision = outcome.state.revision;
            record.committed_revision = Some(new_revision);
            let t = transition(record, ChangeStatus::Committed, vec![])?;
            let mut effects = outcome.effects;
            effects.push(t);
            // Initial dispatches: tell each newly responsible role's provider.
            let outbox: Vec<NewOutbox> = effects
                .iter()
                .filter_map(|e| match e {
                    EventKind::ResponsibilityAssigned {
                        responsibility_id,
                        owner,
                        epoch,
                    }
                    | EventKind::ResponsibilityTransferred {
                        responsibility_id,
                        to: owner,
                        epoch,
                        ..
                    } => {
                        let role = outcome.state.roles.get(owner)?;
                        let binding = role.binding.as_ref()?;
                        Some(NewOutbox {
                            destination: format!("{:?}", binding.provider).to_lowercase(),
                            kind: "assignment_activated".into(),
                            payload: serde_json::json!({
                                "role_id": owner,
                                "responsibility_id": responsibility_id,
                                "epoch": epoch,
                                "provider_agent_id": binding.provider_agent_id,
                            }),
                        })
                    }
                    _ => None,
                })
                .collect();
            let rec = record.clone();
            let events = app.store.commit_state(
                state.revision,
                &outcome.state,
                effects,
                &EventMeta::default().caused_by(record.change_id.to_string()),
                outbox,
                |tx| put_change(tx, &rec),
            )?;
            app.broadcast(&events);
            Ok(())
        }
    }
}

/// Resume sagas interrupted by a restart.
pub async fn resume_pending(app: &AppState) -> ApiResult<usize> {
    let pending: Vec<_> = app
        .store
        .list_changes()?
        .into_iter()
        .filter(|c| {
            matches!(
                c.status,
                ChangeStatus::Validated | ChangeStatus::Provisioning | ChangeStatus::Ready
            )
        })
        .collect();
    let n = pending.len();
    for c in pending {
        let app2 = app.clone();
        tokio::spawn(async move {
            if let Err(e) = run(&app2, &c.change_id).await {
                tracing::error!(change = %c.change_id, error = %e.message, "resumed saga failed");
            }
        });
    }
    Ok(n)
}
