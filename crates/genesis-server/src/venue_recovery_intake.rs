//! Explicitly enabled, durable intake from accepted Reopen decisions to Fetch
//! recovery. This worker discovers already verified evidence; it does not fetch
//! new pages, invent venue facts, or declare a mission complete.
use crate::app::AppState;
use crate::error::{ApiError, ApiResult};
use crate::store::{Store, StoreError, StoreResult};
use crate::venue_assessment_domain::AssessmentVerdict;
use crate::venue_assessments::{self, AssessmentDecision};
use crate::venue_recovery::{self, CreateRecoveryRequest};
use crate::venue_recovery_sources::{self as sources, SourceBinding};
use axum::http::StatusCode;
use chrono::{DateTime, Duration, Utc};
use genesis_core::events::EventKind;
use genesis_core::ids::{JobId, OrgId, RequirementId};
use genesis_core::lifecycle::JobStatus;
use genesis_core::model::Provider;
use genesis_verification::{decode_json, json_sha256};
use rusqlite::params;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

const EVENT_PAGE: u32 = 64;
const SOURCE_PAGE: u32 = 16;
const ROW_PAGE: usize = 16;
const MAX_INTAKES: usize = 256;
const MAX_RECORD_BYTES: usize = 256 * 1024;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum IntakePhase {
    Observing,
    Discovering,
    Prepared,
    Linked,
    Ignored,
    Expired,
}
impl IntakePhase {
    fn active(self) -> bool {
        matches!(self, Self::Observing | Self::Discovering | Self::Prepared)
    }
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Anchor {
    proof_ref: String,
    requirement_id: RequirementId,
    expires_at: DateTime<Utc>,
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Candidate {
    job_id: JobId,
    created_at: DateTime<Utc>,
    proof_ref: String,
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Prepared {
    request: CreateRecoveryRequest,
    source: SourceBinding,
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Intake {
    org_id: OrgId,
    assessment_id: String,
    trigger_seq: u64,
    trigger_job_id: JobId,
    observed_at: DateTime<Utc>,
    revision: u64,
    phase: IntakePhase,
    anchor: Option<Anchor>,
    cursor: u64,
    through: u64,
    scan_uncertain: bool,
    candidate: Option<Candidate>,
    prepared: Option<Prepared>,
    recovery_id: Option<String>,
    externally_managed: bool,
    attention: Option<String>,
}

/// Inspection contains IDs and status, never the raw candidate facts or native
/// provider messages. Linked means a coordinator exists, not that it succeeded.
#[derive(Clone, Debug, Serialize)]
pub struct IntakeRecord {
    pub assessment_id: String,
    pub phase: IntakePhase,
    pub recovery_id: Option<String>,
    pub attention: Option<String>,
    pub client_request_id: Option<String>,
    pub candidate_job_id: Option<JobId>,
    pub deadline: Option<DateTime<Utc>>,
    pub externally_managed: bool,
}
impl Intake {
    fn record(&self) -> IntakeRecord {
        IntakeRecord {
            assessment_id: self.assessment_id.clone(),
            phase: self.phase,
            recovery_id: self.recovery_id.clone(),
            attention: self.attention.clone(),
            client_request_id: self
                .prepared
                .as_ref()
                .map(|p| p.request.client_request_id.clone()),
            candidate_job_id: self
                .prepared
                .as_ref()
                .map(|p| p.request.candidate_job_ids[0].clone()),
            deadline: self.prepared.as_ref().map(|p| p.request.deadline),
            externally_managed: self.externally_managed,
        }
    }
    fn link(&mut self, id: String, external: bool) {
        self.recovery_id = Some(id);
        self.externally_managed = external;
        self.phase = IntakePhase::Linked;
        self.attention = None;
    }
}
fn invalid(message: &'static str) -> ApiError {
    ApiError::new(StatusCode::CONFLICT, "venue_recovery_intake", message)
}
fn sealed(record: &Intake) -> ApiResult<String> {
    let data = serde_json::to_value(record)
        .map_err(|_| ApiError::internal("cannot encode recovery intake"))?;
    let text = json!({"sha256":json_sha256(&data),"data":data}).to_string();
    if text.len() > MAX_RECORD_BYTES {
        return Err(invalid("recovery intake exceeds storage bound"));
    }
    Ok(text)
}
fn key(org: &OrgId, assessment: &str, proof: &str) -> String {
    format!(
        "venue-recovery-auto:{}",
        json_sha256(&json!([org, assessment, proof]))
    )
}
fn validate(record: &Intake) -> ApiResult<()> {
    if record.cursor > record.through
        || record.trigger_seq == 0
        || record.assessment_id.is_empty()
        || (record.phase == IntakePhase::Linked) != record.recovery_id.is_some()
        || (record.phase == IntakePhase::Prepared && record.prepared.is_none())
        || (record.phase == IntakePhase::Discovering && record.anchor.is_none())
    {
        return Err(invalid("invalid recovery intake state"));
    }
    if let Some(p) = &record.prepared {
        let a = record
            .anchor
            .as_ref()
            .ok_or_else(|| invalid("missing intake anchor"))?;
        if p.request.client_request_id != key(&record.org_id, &record.assessment_id, &a.proof_ref)
            || p.request.reopened_assessment_id != record.assessment_id
            || p.request.candidate_job_ids.len() != 1
            || p.source.candidates.len() != 1
            || p.request.candidate_job_ids[0] != p.source.candidates[0].job_id
            || p.source.org_id != record.org_id
            || p.source.reopened_assessment_id != record.assessment_id
            || p.source.reopened_proof_ref != a.proof_ref
            || p.source.replacement_requirement_id != a.requirement_id
            || p.request.deadline > a.expires_at
        {
            return Err(invalid("prepared recovery intake binding changed"));
        }
    }
    Ok(())
}

impl Store {
    pub fn init_venue_recovery_intake(&self) -> StoreResult<()> {
        self.lock().execute_batch(
            "CREATE TABLE IF NOT EXISTS venue_recovery_intake_cursor (
               singleton INTEGER PRIMARY KEY CHECK(singleton=1), seq INTEGER NOT NULL);
             INSERT OR IGNORE INTO venue_recovery_intake_cursor VALUES(1,0);
             CREATE TABLE IF NOT EXISTS venue_recovery_intakes (
               assessment_id TEXT PRIMARY KEY, trigger_seq INTEGER NOT NULL UNIQUE,
               revision INTEGER NOT NULL, active INTEGER NOT NULL,
               checked_at TEXT NOT NULL, data TEXT NOT NULL);
             CREATE INDEX IF NOT EXISTS events_recovery_intake_type_seq ON events(event_type,seq);",
        )?;
        Ok(())
    }
    fn recovery_intake(&self, id: &str) -> ApiResult<Intake> {
        let (text, revision, seq, active): (String,u64,u64,bool) = self.lock().query_row(
            "SELECT data,revision,trigger_seq,active FROM venue_recovery_intakes WHERE assessment_id=?1 AND length(CAST(data AS BLOB))<=?2",
            params![id,MAX_RECORD_BYTES], |r| Ok((r.get(0)?,r.get(1)?,r.get(2)?,r.get(3)?)))
            .map_err(StoreError::from)?;
        #[derive(Deserialize)]
        #[serde(deny_unknown_fields)]
        struct Sealed {
            data: Value,
            sha256: String,
        }
        let envelope: Sealed =
            decode_json(text.as_bytes()).map_err(|_| invalid("invalid intake record"))?;
        if json_sha256(&envelope.data) != envelope.sha256 {
            return Err(invalid("intake digest changed"));
        }
        let record: Intake =
            serde_json::from_value(envelope.data).map_err(|_| invalid("invalid intake payload"))?;
        if record.assessment_id != id
            || record.revision != revision
            || record.trigger_seq != seq
            || record.phase.active() != active
        {
            return Err(invalid("intake index changed"));
        }
        validate(&record)?;
        Ok(record)
    }
    fn save_intake(&self, record: &mut Intake) -> ApiResult<()> {
        let previous = record.revision;
        record.revision += 1;
        validate(record)?;
        let changed = self.lock().execute(
            "UPDATE venue_recovery_intakes SET revision=?1,active=?2,checked_at=?3,data=?4 WHERE assessment_id=?5 AND revision=?6",
            params![record.revision,record.phase.active(),Utc::now().to_rfc3339(),sealed(record)?,record.assessment_id,previous])
            .map_err(StoreError::from)?;
        if changed != 1 {
            return Err(invalid("intake changed concurrently"));
        }
        Ok(())
    }
}

fn watermark(app: &AppState) -> ApiResult<u64> {
    Ok(app
        .store
        .lock()
        .query_row("SELECT COALESCE(MAX(seq),0) FROM events", [], |r| r.get(0))
        .map_err(StoreError::from)?)
}

// Page relevant completion events, not every SSE event in the organization's
// lifetime. SQL only locates hints; full proofs and job bindings are still read
// below. The source age filter is deliberately one minute wider than authority
// permits, avoiding boundary rounding in SQLite's date arithmetic.
fn completion_events(
    app: &AppState,
    after: u64,
    through: u64,
    assessments: bool,
    now: DateTime<Utc>,
) -> ApiResult<Vec<genesis_core::events::DomainEvent>> {
    let seqs: Vec<u64> = {
        let conn = app.store.lock();
        if assessments {
            let mut stmt = conn.prepare("SELECT e.seq FROM events e JOIN venue_assessments a ON a.job_id=json_extract(e.payload_json,'$.job_id') WHERE e.seq>?1 AND e.seq<=?2 AND e.event_type='job_status_changed' AND json_extract(e.payload_json,'$.to')='SUCCEEDED' ORDER BY e.seq LIMIT ?3").map_err(StoreError::from)?;
            let rows = stmt
                .query_map(params![after, through, EVENT_PAGE], |r| r.get(0))
                .map_err(StoreError::from)?
                .collect::<Result<_, _>>()
                .map_err(StoreError::from)?;
            rows
        } else {
            let mut stmt = conn.prepare("SELECT e.seq FROM events e JOIN jobs j ON j.job_id=json_extract(e.payload_json,'$.job_id') WHERE e.seq>?1 AND e.seq<=?2 AND e.event_type='job_status_changed' AND json_extract(e.payload_json,'$.to')='SUCCEEDED' AND json_extract(j.job_json,'$.provider')='apify' AND julianday(j.created_at)>=julianday(?3) ORDER BY e.seq LIMIT ?4").map_err(StoreError::from)?;
            let rows = stmt
                .query_map(
                    params![
                        after,
                        through,
                        (now - Duration::minutes(31)).to_rfc3339(),
                        SOURCE_PAGE
                    ],
                    |r| r.get(0),
                )
                .map_err(StoreError::from)?
                .collect::<Result<_, _>>()
                .map_err(StoreError::from)?;
            rows
        }
    };
    seqs.into_iter()
        .map(|seq| {
            let event = app
                .store
                .events_after(seq - 1, 1)?
                .into_iter()
                .next()
                .ok_or_else(|| invalid("intake event disappeared"))?;
            if event.seq != seq {
                return Err(invalid("intake event sequence changed"));
            }
            Ok(event)
        })
        .collect()
}

/// Persist each trigger and its cursor together before reconstructing provider
/// proofs. Missing CREAO journals cannot lose the event or block other rows.
fn discover(app: &AppState) -> ApiResult<()> {
    let mut cursor: u64 = app
        .store
        .lock()
        .query_row(
            "SELECT seq FROM venue_recovery_intake_cursor WHERE singleton=1",
            [],
            |r| r.get(0),
        )
        .map_err(StoreError::from)?;
    let through = watermark(app)?;
    let events = completion_events(app, cursor, through, true, Utc::now())?;
    let exhausted = events.len() < EVENT_PAGE as usize;
    for event in events {
        let mut hint = None;
        if let EventKind::JobStatusChanged {
            job_id,
            to: JobStatus::Succeeded,
            ..
        } = &event.kind
        {
            if let Some(id) = app.store.assessment_for_job(job_id)? {
                hint = Some(Intake {
                    org_id: app.org_id.clone(),
                    assessment_id: id,
                    trigger_seq: event.seq,
                    trigger_job_id: job_id.clone(),
                    observed_at: event.timestamp,
                    revision: 0,
                    phase: IntakePhase::Observing,
                    anchor: None,
                    cursor: 0,
                    through: 0,
                    scan_uncertain: false,
                    candidate: None,
                    prepared: None,
                    recovery_id: None,
                    externally_managed: false,
                    attention: None,
                });
            }
        }
        let mut conn = app.store.lock();
        let tx = conn.transaction().map_err(StoreError::from)?;
        if let Some(hint) = hint {
            let exists: bool = tx
                .query_row(
                    "SELECT EXISTS(SELECT 1 FROM venue_recovery_intakes WHERE assessment_id=?1)",
                    [&hint.assessment_id],
                    |r| r.get(0),
                )
                .map_err(StoreError::from)?;
            let count: usize = tx
                .query_row("SELECT COUNT(*) FROM venue_recovery_intakes", [], |r| {
                    r.get(0)
                })
                .map_err(StoreError::from)?;
            if !exists && count >= MAX_INTAKES {
                return Err(invalid("automatic recovery intake lifetime limit reached"));
            }
            tx.execute(
                "INSERT OR IGNORE INTO venue_recovery_intakes VALUES(?1,?2,0,1,?3,?4)",
                params![
                    hint.assessment_id,
                    hint.trigger_seq,
                    Utc::now().to_rfc3339(),
                    sealed(&hint)?
                ],
            )
            .map_err(StoreError::from)?;
        }
        if tx
            .execute(
                "UPDATE venue_recovery_intake_cursor SET seq=?1 WHERE singleton=1 AND seq=?2",
                params![event.seq, cursor],
            )
            .map_err(StoreError::from)?
            != 1
        {
            return Err(invalid("intake discovery cursor changed"));
        }
        tx.commit().map_err(StoreError::from)?;
        cursor = event.seq;
    }
    if exhausted
        && app
            .store
            .lock()
            .execute(
                "UPDATE venue_recovery_intake_cursor SET seq=?1 WHERE singleton=1 AND seq=?2",
                params![through, cursor],
            )
            .map_err(StoreError::from)?
            != 1
    {
        return Err(invalid("intake discovery cursor changed"));
    }
    Ok(())
}

fn advance(app: &AppState, row: &mut Intake) -> ApiResult<()> {
    let now = Utc::now();
    if row.org_id != app.org_id {
        return Err(invalid("intake organization changed"));
    }
    // Reconciliation precedes current freshness, provider configuration and the
    // enable flag: the previous creation may have committed before its link.
    if let Some(prepared) = &row.prepared {
        if let Some(existing) = venue_recovery::lookup_request_locked(app, &prepared.request)? {
            row.link(existing.recovery_id, false);
            return Ok(());
        }
    }
    if let Some(anchor) = &row.anchor {
        if let Some(existing) = venue_recovery::lookup_reopen_locked(
            app,
            &row.assessment_id,
            &anchor.proof_ref,
            &anchor.requirement_id,
        )? {
            row.link(existing.recovery_id, true);
            return Ok(());
        }
    } else if let Some(existing) =
        venue_recovery::lookup_assessment_locked(app, &row.assessment_id)?
    {
        row.link(existing.recovery_id, true);
        return Ok(());
    }
    if !app.automatic_venue_recovery {
        return Ok(());
    }
    if let Some(prepared) = &row.prepared {
        if prepared.request.deadline <= now {
            row.phase = IntakePhase::Expired;
            row.attention = Some("prepared_deadline_elapsed".into());
            return Ok(());
        }
        sources::revalidate(app, &prepared.source)?;
        let (record, _) = venue_recovery::create_locked(app, prepared.request.clone())?;
        row.link(record.recovery_id, false);
        return Ok(());
    }
    let expiry = row
        .anchor
        .as_ref()
        .map_or(row.observed_at + Duration::minutes(30), |a| a.expires_at);
    if expiry <= now {
        row.phase = IntakePhase::Expired;
        row.attention = Some("source_freshness_elapsed".into());
        return Ok(());
    }
    let assessment = venue_assessments::read_locked(app, &row.assessment_id)?;
    if assessment.job_id != row.trigger_job_id {
        return Err(invalid("intake assessment job changed"));
    }
    let proof = assessment
        .proof
        .as_ref()
        .ok_or_else(|| invalid("assessment proof unavailable"))?;
    if assessment.status != JobStatus::Succeeded
        || proof.decision != AssessmentDecision::Accepted
        || proof
            .output
            .as_ref()
            .is_none_or(|p| p.verdict != AssessmentVerdict::Reopen)
    {
        row.phase = IntakePhase::Ignored;
        row.attention = None;
        return Ok(());
    }
    let requirement = proof
        .requirement_id
        .as_ref()
        .ok_or_else(|| invalid("reopen requirement unavailable"))?;
    if let Some(anchor) = &row.anchor {
        if anchor.proof_ref != proof.proof_ref || &anchor.requirement_id != requirement {
            return Err(invalid("intake reopen proof changed"));
        }
    } else {
        let original = app.store.get_job(&assessment.source_job_id)?;
        if original.created_at > now || proof.verified_at > now {
            return Err(invalid("intake source is from the future"));
        }
        row.anchor = Some(Anchor {
            proof_ref: proof.proof_ref.clone(),
            requirement_id: requirement.clone(),
            expires_at: expiry.min(original.created_at + Duration::minutes(30)),
        });
        row.phase = IntakePhase::Discovering;
        // Persist the authenticated anchor before candidate discovery, including
        // the identity needed to reconcile an operator-created recovery.
        return Ok(());
    }
    let latest = watermark(app)?;
    if row.cursor == row.through {
        row.through = latest;
    }
    if row.through > latest {
        return Err(invalid("intake source watermark changed"));
    }
    let events = completion_events(app, row.cursor, row.through, false, now)?;
    let exhausted = events.len() < SOURCE_PAGE as usize;
    for event in events {
        if event.seq > row.through {
            break;
        }
        row.cursor = event.seq;
        let EventKind::JobStatusChanged {
            job_id,
            to: JobStatus::Succeeded,
            ..
        } = event.kind
        else {
            continue;
        };
        let job = match app.store.get_job(&job_id) {
            Ok(job) => job,
            Err(_) => {
                row.scan_uncertain = true;
                continue;
            }
        };
        let original = app.store.get_job(&assessment.source_job_id)?;
        if job.org_id != app.org_id
            || job.mission_id != original.mission_id
            || job.provider != Some(Provider::Apify)
            || job.status != JobStatus::Succeeded
            || job.created_at > now
            || job.created_at + Duration::minutes(30) <= now
        {
            continue;
        }
        let mut input = job.input.clone();
        let mut original_input = original.input.clone();
        let (Some(input), Some(original_input)) =
            (input.as_object_mut(), original_input.as_object_mut())
        else {
            continue;
        };
        input.remove("source_url");
        original_input.remove("source_url");
        if json_sha256(&json!(input)) != json_sha256(&json!(original_input)) {
            continue;
        }
        // A failed authentication is not a candidate. Revisit the bounded
        // history on the next completed scan while awaiting suitable evidence.
        let binding = match sources::prepare_candidate(app, &row.assessment_id, &job_id, now) {
            Ok(Some((binding, _))) => binding,
            Ok(None) => continue,
            Err(_) => {
                row.scan_uncertain = true;
                continue;
            }
        };
        let source = &binding.candidates[0];
        if row.candidate.as_ref().is_none_or(|old| {
            (source.created_at, source.job_id.as_str()) > (old.created_at, old.job_id.as_str())
        }) {
            row.candidate = Some(Candidate {
                job_id: source.job_id.clone(),
                created_at: source.created_at,
                proof_ref: source.proof_ref.clone(),
            });
        }
    }
    if exhausted {
        row.cursor = row.through;
    }
    if row.cursor < row.through {
        return Ok(());
    }
    if row.scan_uncertain {
        row.cursor = 0;
        row.through = latest;
        row.scan_uncertain = false;
        row.attention = Some("awaiting_source_verification".into());
        return Ok(());
    }
    let Some(candidate) = row.candidate.clone() else {
        row.cursor = 0;
        row.through = latest;
        row.attention = Some("awaiting_feasible_verified_source".into());
        return Ok(());
    };
    let prepared = sources::prepare_candidate(app, &row.assessment_id, &candidate.job_id, now)?;
    let Some((source, deadline)) = prepared else {
        row.candidate = None;
        row.cursor = 0;
        return Ok(());
    };
    if source.candidates[0].created_at != candidate.created_at
        || source.candidates[0].proof_ref != candidate.proof_ref
    {
        return Err(invalid("saved intake candidate changed"));
    }
    if app.fetch_recovery.is_none() || app.providers.specialist(Provider::Creao).is_err() {
        row.attention = Some("awaiting_recovery_providers".into());
        return Ok(());
    }
    let anchor = row.anchor.as_ref().unwrap();
    let deadline = deadline.min(anchor.expires_at);
    if deadline <= now {
        row.phase = IntakePhase::Expired;
        return Ok(());
    }
    let request = CreateRecoveryRequest {
        client_request_id: key(&app.org_id, &row.assessment_id, &anchor.proof_ref),
        reopened_assessment_id: row.assessment_id.clone(),
        candidate_job_ids: vec![candidate.job_id],
        deadline,
    };
    venue_recovery::preflight_locked(app, request.clone())?;
    row.prepared = Some(Prepared { request, source });
    row.phase = IntakePhase::Prepared;
    row.attention = None;
    // Creation deliberately happens on a later pass, after this exact intent
    // and all source bindings are durable. No deadline renewal is possible.
    Ok(())
}

pub async fn drain_once(app: &AppState) -> ApiResult<usize> {
    let _guard = app.kernel.lock().await;
    // A discovery error (including quota) must not starve existing intakes.
    let discovery = if app.automatic_venue_recovery {
        discover(app)
    } else {
        Ok(())
    };
    let ids: Vec<String> = {
        let conn = app.store.lock();
        let mut stmt = conn.prepare("SELECT assessment_id FROM venue_recovery_intakes WHERE active=1 ORDER BY checked_at,assessment_id LIMIT ?1").map_err(StoreError::from)?;
        let ids = stmt
            .query_map([ROW_PAGE], |r| r.get(0))
            .map_err(StoreError::from)?
            .collect::<Result<_, _>>()
            .map_err(StoreError::from)?;
        ids
    };
    for id in &ids {
        match app.store.recovery_intake(id) {
            Ok(mut row) => {
                if advance(app, &mut row).is_err() {
                    row.attention = Some("recovery_intake_requires_attention".into());
                }
                app.store.save_intake(&mut row)?;
            }
            Err(_) => {
                // Preserve corrupt evidence and rotate it out of this page.
                app.store
                    .lock()
                    .execute(
                        "UPDATE venue_recovery_intakes SET checked_at=?1 WHERE assessment_id=?2",
                        params![Utc::now().to_rfc3339(), id],
                    )
                    .map_err(StoreError::from)?;
            }
        }
    }
    discovery?;
    Ok(ids.len())
}

pub async fn list(app: &AppState) -> ApiResult<Vec<IntakeRecord>> {
    let _guard = app.kernel.lock().await;
    let ids: Vec<String> = {
        let conn = app.store.lock();
        let mut stmt = conn
            .prepare(
                "SELECT assessment_id FROM venue_recovery_intakes ORDER BY trigger_seq LIMIT ?1",
            )
            .map_err(StoreError::from)?;
        let ids = stmt
            .query_map([MAX_INTAKES], |r| r.get(0))
            .map_err(StoreError::from)?
            .collect::<Result<_, _>>()
            .map_err(StoreError::from)?;
        ids
    };
    ids.iter()
        .map(|id| app.store.recovery_intake(id).map(|r| r.record()))
        .collect()
}

pub fn attach_from_env(mut app: AppState) -> anyhow::Result<AppState> {
    match std::env::var("GENESIS_AUTOMATIC_VENUE_RECOVERY") {
        Err(std::env::VarError::NotPresent) => {}
        Ok(value) if value == "1" || value == "true" => app.automatic_venue_recovery = true,
        Ok(value) if value == "0" || value == "false" => app.automatic_venue_recovery = false,
        _ => anyhow::bail!("GENESIS_AUTOMATIC_VENUE_RECOVERY must be true, false, 1 or 0"),
    }
    Ok(app)
}
pub fn spawn(app: AppState) -> tokio::task::JoinHandle<()> {
    tokio::spawn(async move {
        let mut interval = tokio::time::interval(std::time::Duration::from_millis(500));
        loop {
            interval.tick().await;
            if drain_once(&app).await.is_err() {
                tracing::warn!(org = %app.org_id, "venue recovery intake pass requires attention");
            }
        }
    })
}
