//! Receiver-owned development needs enter the ordinary observation/planner path.
//! A checked registry selection is evidence, never installation or authority.
use crate::app::AppState;
use crate::auth::Principal;
use crate::error::{ApiError, ApiResult};
use crate::registry::methods::MethodCandidate;
use crate::store::{EventMeta, Store, StoreResult};
use axum::{
    extract::{Path, State},
    http::StatusCode,
    Json,
};
use chrono::Utc;
use genesis_core::{
    digest::canonical_sha256,
    engine,
    ids::{MissionId, ObservationId, OrgId},
    lifecycle::MissionStatus,
    model::OrgState,
};
use rusqlite::{params, OptionalExtension};
use serde::{Deserialize, Serialize};
use serde_json::json;

const SCHEMA: &str = "genesis-method-development-intake/1";

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MethodDevelopmentRequest {
    pub client_request_id: String,
    pub need: String,
    pub acceptance: String,
    pub selection: MethodCandidate,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MethodDevelopmentIntake {
    pub schema: String,
    pub org_id: OrgId,
    pub mission_id: MissionId,
    pub observation_id: ObservationId,
    pub request_sha256: String,
    pub request: MethodDevelopmentRequest,
}

impl MethodDevelopmentRequest {
    fn validate(&self) -> ApiResult<()> {
        if self.client_request_id.is_empty()
            || self.client_request_id.len() > 128
            || !self
                .client_request_id
                .bytes()
                .all(|c| c.is_ascii_alphanumeric() || b"-_.".contains(&c))
            || [&self.need, &self.acceptance]
                .iter()
                .any(|s| s.trim().is_empty() || s.len() > 2048)
        {
            return Err(ApiError::bad_request("invalid method development request"));
        }
        self.selection.validate()
    }
    fn digest(&self, org: &OrgId, mission: &MissionId) -> String {
        canonical_sha256(&json!({"schema":SCHEMA,"org_id":org,"mission_id":mission,"request":self}))
    }
}

impl Store {
    pub(crate) fn init_method_development(&self) -> StoreResult<()> {
        self.lock().execute_batch(
            "CREATE TABLE IF NOT EXISTS method_development_requests (
                client_request_id TEXT PRIMARY KEY, record_json TEXT NOT NULL);",
        )?;
        Ok(())
    }
    fn method_development(&self, key: &str) -> ApiResult<Option<MethodDevelopmentIntake>> {
        let text: Option<String> = self
            .lock()
            .query_row(
                "SELECT record_json FROM method_development_requests WHERE client_request_id=?1",
                [key],
                |row| row.get(0),
            )
            .optional()
            .map_err(crate::store::StoreError::from)?;
        let Some(text) = text else {
            return Ok(None);
        };
        let record: MethodDevelopmentIntake = serde_json::from_str(&text)
            .map_err(|_| ApiError::internal("invalid retained method development request"))?;
        record.request.validate()?;
        if record.schema != SCHEMA
            || &record.org_id != self.org_id()
            || record.request.client_request_id != key
            || record.request_sha256 != record.request.digest(&record.org_id, &record.mission_id)
        {
            return Err(ApiError::internal(
                "method development request binding changed",
            ));
        }
        Ok(Some(record))
    }
}

fn active_mission(state: &OrgState, mission: &MissionId) -> ApiResult<()> {
    let m = state
        .missions
        .get(mission)
        .ok_or_else(|| ApiError::not_found("receiver mission"))?;
    if m.status != MissionStatus::Active || m.deadline.is_some_and(|d| d <= Utc::now()) {
        return Err(ApiError::new(
            StatusCode::CONFLICT,
            "inactive_mission",
            "receiver mission is not active",
        ));
    }
    Ok(())
}

fn replay(
    app: &AppState,
    mission: &MissionId,
    body: &MethodDevelopmentRequest,
) -> ApiResult<Option<MethodDevelopmentIntake>> {
    let record = app.store.method_development(&body.client_request_id)?;
    if record
        .as_ref()
        .is_some_and(|r| &r.mission_id != mission || r.request != *body)
    {
        return Err(ApiError::new(
            StatusCode::CONFLICT,
            "idempotency_conflict",
            "client_request_id already names a different method development request",
        ));
    }
    Ok(record)
}

pub async fn create(
    State(app): State<AppState>,
    p: Principal,
    Path(id): Path<String>,
    Json(body): Json<MethodDevelopmentRequest>,
) -> ApiResult<(StatusCode, Json<MethodDevelopmentIntake>)> {
    p.require(&[Principal::Operator, Principal::Agent])?;
    body.validate()?;
    let mission = MissionId::new(id);
    {
        let _guard = app.kernel.lock().await;
        if let Some(record) = replay(&app, &mission, &body)? {
            return Ok((StatusCode::OK, Json(record)));
        }
        active_mission(&app.current_state()?, &mission)?;
    }
    let registry = app.registry.as_ref().ok_or_else(|| {
        ApiError::new(
            StatusCode::SERVICE_UNAVAILABLE,
            "registry_unconfigured",
            "method registry is not configured",
        )
    })?;
    // Fetch once through the checked client; source is deliberately not retained in
    // the organizational observation, planner prompt or intake record.
    registry.fetch_method_candidate(&body.selection).await?;
    let _guard = app.kernel.lock().await;
    if let Some(record) = replay(&app, &mission, &body)? {
        return Ok((StatusCode::OK, Json(record)));
    }
    let state = app.current_state()?;
    active_mission(&state, &mission)?;
    let digest = body.digest(&app.org_id, &mission);
    let summary = format!(
        "Receiver-owned capability-development need: {}\nDesired acceptance: {}\nAn exact network method selection is retained as evidence ({digest}). It is not an installed Actor capability or an accepted method. Assess continuing responsibility using the existing owners; create a specialist only when distinct continuing work justifies it. Availability alone requires no organizational change.",
        body.need, body.acceptance,
    );
    let (outcome, observation_id) = engine::record_observation(
        &state,
        &mission,
        "method-development",
        &summary,
        Some(format!("genesis:method-development:{digest}")),
        Utc::now(),
    )?;
    let record = MethodDevelopmentIntake {
        schema: SCHEMA.into(),
        org_id: app.org_id.clone(),
        mission_id: mission.clone(),
        observation_id,
        request_sha256: digest,
        request: body,
    };
    let text = serde_json::to_string(&record)
        .map_err(|_| ApiError::internal("cannot encode method development request"))?;
    let events = app.store.commit_state(
        state.revision,
        &outcome.state,
        outcome.effects,
        &EventMeta::mission(mission).caused_by(record.request.client_request_id.clone()),
        Vec::new(),
        |tx| {
            tx.execute(
                "INSERT INTO method_development_requests VALUES(?1,?2)",
                params![record.request.client_request_id, text],
            )?;
            Ok(())
        },
    )?;
    app.broadcast(&events);
    Ok((StatusCode::CREATED, Json(record)))
}

pub async fn get(
    State(app): State<AppState>,
    p: Principal,
    Path((mission, key)): Path<(String, String)>,
) -> ApiResult<Json<MethodDevelopmentIntake>> {
    p.require(&[Principal::Operator, Principal::Agent])?;
    let record = app
        .store
        .method_development(&key)?
        .filter(|r| r.mission_id.as_str() == mission)
        .ok_or_else(|| ApiError::not_found("method development request"))?;
    Ok(Json(record))
}
