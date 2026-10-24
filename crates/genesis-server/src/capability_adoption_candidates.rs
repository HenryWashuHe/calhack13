//! Henry's Claude: a durable record of why each registry candidate was or
//! wasn't used by an adoption.
//!
//! A registry search ranks candidates by similarity; that rank and score are
//! ordering only. Whether a candidate may be used is decided by this
//! organization: its own publications are skipped, permissions must be held,
//! the manifest must match the search answer and be usable, and only this
//! organization's own acceptance suite can make it applicable. Each decision
//! is written in the same transaction as the adoption outcome it explains, so
//! a restart or a repeated step can't produce a second, different story.
use crate::registry::SearchCandidate;
use crate::store::{Store, StoreError, StoreResult};
use chrono::{DateTime, Utc};
use genesis_core::ids::*;
use genesis_verification::{decode_json, VerificationReport};
use rusqlite::{params, Transaction};
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;

pub(crate) const SCHEMA: &str = "CREATE TABLE IF NOT EXISTS adoption_candidates(\
    adoption_id TEXT NOT NULL,\
    rank INTEGER NOT NULL CHECK(rank >= 0),\
    decision_json TEXT NOT NULL,\
    PRIMARY KEY(adoption_id, rank));";

/// What this organization decided about one candidate.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CandidateDecisionKind {
    /// Chosen for local acceptance; replaced by `adopted` or `not_applicable`
    /// once the suite decides.
    Selected,
    /// This organization's own acceptance suite accepted it.
    Adopted,
    /// This organization's own acceptance suite rejected it.
    NotApplicable,
    /// Published by this organization; adoption only inherits from others.
    OwnPublication,
    /// Needs a permission the requesting role doesn't hold.
    MissingScope,
    /// The manifest disagrees with the search answer or fails local checks.
    InvalidManifest,
    /// The manifest is in a state that can't be installed.
    UnusableState,
    /// The manifest couldn't be read; its applicability is unknown.
    ManifestUnavailable,
    /// The registry answer was invalid: more than five candidates (nothing
    /// in it is evaluated), or, reached in rank order, a score that isn't a
    /// positive finite number (discovery stops there).
    InvalidResponse,
    /// Eligible, but this organization already holds an installation of the
    /// same exact version; this intent stops without running checks.
    InstallationConflict,
    /// A higher-ranked candidate was selected or discovery stopped first.
    NotEvaluated,
}

impl CandidateDecisionKind {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Selected => "selected",
            Self::Adopted => "adopted",
            Self::NotApplicable => "not_applicable",
            Self::OwnPublication => "own_publication",
            Self::MissingScope => "missing_scope",
            Self::InvalidManifest => "invalid_manifest",
            Self::UnusableState => "unusable_state",
            Self::ManifestUnavailable => "manifest_unavailable",
            Self::InvalidResponse => "invalid_response",
            Self::InstallationConflict => "installation_conflict",
            Self::NotEvaluated => "not_evaluated",
        }
    }
}

/// One candidate as the registry described it, with this organization's
/// decision. `applicability_conditions` are the publisher's own claims, kept
/// for display; they never decide applicability here. The free-text intent is
/// deliberately not copied: it can echo the private requirement query.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CandidateDecision {
    pub schema_version: u32,
    pub adoption_id: String,
    pub rank: u32,
    /// The registry's similarity score, or `None` when it wasn't a finite
    /// number (JSON can't carry infinity, and a JSON number past `f32` range
    /// decodes to it). Ordering only.
    pub score: Option<f32>,
    pub capability_id: CapabilityId,
    pub version: String,
    pub publisher: OrgId,
    pub source_digest: String,
    pub applicability_conditions: Vec<String>,
    pub required_permissions: Vec<String>,
    pub decision: CandidateDecisionKind,
    pub reason: String,
    #[serde(default)]
    pub missing_scopes: Vec<String>,
    /// Acceptance or rejection reference from this organization's suite.
    #[serde(default)]
    pub acceptance_ref: Option<String>,
    pub decided_at: DateTime<Utc>,
}

const MAX_TEXT: usize = 4096;

fn clip(text: &str) -> String {
    text.chars().take(MAX_TEXT).collect()
}

impl CandidateDecision {
    pub(crate) fn new(
        adoption_id: &str,
        rank: usize,
        candidate: &SearchCandidate,
        decision: CandidateDecisionKind,
        reason: impl Into<String>,
        at: DateTime<Utc>,
    ) -> Self {
        Self {
            schema_version: 1,
            adoption_id: adoption_id.to_owned(),
            rank: u32::try_from(rank).unwrap_or(u32::MAX),
            score: candidate.score.is_finite().then_some(candidate.score),
            capability_id: candidate.capability_id.clone(),
            version: clip(&candidate.version),
            publisher: candidate.publisher.clone(),
            source_digest: clip(&candidate.source_digest),
            applicability_conditions: candidate
                .applicability_conditions
                .iter()
                .take(32)
                .map(|c| clip(c))
                .collect(),
            required_permissions: candidate
                .required_permissions
                .iter()
                .take(64)
                .map(|p| clip(p))
                .collect(),
            decision,
            reason: clip(&reason.into()),
            missing_scopes: Vec::new(),
            acceptance_ref: None,
            decided_at: at,
        }
    }

    pub(crate) fn with_missing(mut self, missing: Vec<String>) -> Self {
        self.missing_scopes = missing;
        self
    }
}

/// A short, oracle-free summary of a local rejection: how many cases failed,
/// which cases, and which issue kinds. Expected values never leave the
/// verifier.
pub(crate) fn rejection_summary(report: &VerificationReport) -> String {
    let failed: Vec<&str> = report
        .cases
        .iter()
        .filter(|c| !c.issues.is_empty())
        .map(|c| c.case_id.as_str())
        .take(16)
        .collect();
    let codes: BTreeSet<String> = report
        .issues
        .iter()
        .chain(report.cases.iter().flat_map(|c| c.issues.iter()))
        .map(|i| format!("{:?}", i.code))
        .collect();
    let codes: Vec<String> = codes.into_iter().take(8).collect();
    format!(
        "this organization's acceptance suite rejected it: {} of {} cases failed{}{}",
        report.cases.iter().filter(|c| !c.issues.is_empty()).count(),
        report.cases.len(),
        if failed.is_empty() {
            String::new()
        } else {
            format!(" ({})", failed.join(", "))
        },
        if codes.is_empty() {
            String::new()
        } else {
            format!("; issues: {}", codes.join(", "))
        },
    )
}

fn db(message: &'static str) -> StoreError {
    StoreError::Db(rusqlite::Error::InvalidParameterName(message.into()))
}

/// Write one discovery's decisions. Called exactly once per adoption, inside
/// the transaction that commits the discovery outcome; a second write fails
/// the transaction (primary key), so outcomes and explanations can't diverge.
pub(crate) fn insert_in_tx(
    tx: &Transaction<'_>,
    decisions: &[CandidateDecision],
) -> StoreResult<()> {
    for d in decisions {
        let json = serde_json::to_string(d).map_err(|_| db("candidate decision encoding"))?;
        tx.execute(
            "INSERT INTO adoption_candidates(adoption_id,rank,decision_json) VALUES(?1,?2,?3)",
            params![d.adoption_id, d.rank, json],
        )?;
    }
    Ok(())
}

/// Record the suite's verdict on the selected candidate, in the transaction
/// that finishes the installation. A missing selected row (an adoption made
/// before this log existed) is left alone.
pub(crate) fn conclude_in_tx(
    tx: &Transaction<'_>,
    adoption_id: &str,
    decision: CandidateDecisionKind,
    reason: &str,
    acceptance_ref: &str,
    at: DateTime<Utc>,
) -> StoreResult<()> {
    debug_assert!(matches!(
        decision,
        CandidateDecisionKind::Adopted | CandidateDecisionKind::NotApplicable
    ));
    let rows: Vec<(u32, String)> = {
        let mut stmt = tx.prepare(
            "SELECT rank,decision_json FROM adoption_candidates WHERE adoption_id=?1 ORDER BY rank",
        )?;
        let rows = stmt
            .query_map(params![adoption_id], |r| Ok((r.get(0)?, r.get(1)?)))?
            .collect::<Result<Vec<_>, _>>()?;
        rows
    };
    for (rank, json) in rows {
        let mut d: CandidateDecision =
            decode_json(json.as_bytes()).map_err(|_| db("candidate decision decoding"))?;
        if d.decision != CandidateDecisionKind::Selected {
            continue;
        }
        d.decision = decision;
        d.reason = clip(reason);
        d.acceptance_ref = Some(clip(acceptance_ref));
        d.decided_at = at;
        let json = serde_json::to_string(&d).map_err(|_| db("candidate decision encoding"))?;
        tx.execute(
            "UPDATE adoption_candidates SET decision_json=?3 WHERE adoption_id=?1 AND rank=?2",
            params![adoption_id, rank, json],
        )?;
    }
    Ok(())
}

impl Store {
    /// Every recorded decision for one adoption, by registry rank.
    pub fn adoption_candidates(&self, adoption_id: &str) -> StoreResult<Vec<CandidateDecision>> {
        let conn = self.lock();
        let mut stmt = conn.prepare(
            "SELECT rank,decision_json FROM adoption_candidates WHERE adoption_id=?1 ORDER BY rank",
        )?;
        let rows = stmt
            .query_map(params![adoption_id], |r| {
                Ok((r.get::<_, u32>(0)?, r.get::<_, String>(1)?))
            })?
            .collect::<Result<Vec<_>, _>>()?;
        rows.into_iter()
            .map(|(rank, json)| {
                let d: CandidateDecision =
                    decode_json(json.as_bytes()).map_err(|_| db("candidate decision decoding"))?;
                if d.rank != rank || d.adoption_id != adoption_id || d.schema_version != 1 {
                    return Err(db("candidate decision does not match its row"));
                }
                Ok(d)
            })
            .collect()
    }
}
