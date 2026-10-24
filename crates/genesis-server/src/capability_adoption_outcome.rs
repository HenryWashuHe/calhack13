//! Henry's Claude: what an adopt-first gap actually concluded, in terms an
//! operator or planner can act on.
//!
//! Adoption can end in very different places, and they must not blur:
//! a candidate this organization accepted, one it tested and rejected, one it
//! couldn't evaluate, an honest empty search, or an outcome nobody knows yet.
//! Only `no_candidate` from a definitive empty search is ever a reason to
//! synthesize; the classifier is read-only and changes no fallback.
//!
//! Similarity rank and score never appear in the verdict. The verdict comes
//! from this organization's own decisions (candidate log) and the stopped
//! adoption's attention code.
use crate::capability_adoption_candidates::{CandidateDecision, CandidateDecisionKind as K};
use crate::capability_gap_adoption::{GapAdoptionCode as C, GapAdoptionState as S};
use genesis_core::ids::{CapabilityId, OrgId};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

/// The five evidence-distinguished conclusions, plus "still working" and
/// "stopped for a reason that says nothing about applicability".
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AdoptionOutcome {
    /// Adoption still owns the gap; no conclusion yet.
    Pending,
    /// A foreign capability passed this organization's own acceptance suite
    /// and was installed for the gap.
    Adopted,
    /// The selected candidate ran this organization's suite and was rejected.
    NotApplicable,
    /// A candidate was offered but couldn't be evaluated to a verdict: an
    /// invalid or unusable manifest or score, or checks that ended with a
    /// confirmed failure and no evidence.
    InsufficientEvidence,
    /// The search answered and nothing in it was eligible (own publications,
    /// missing permissions) or it was empty.
    NoCandidate,
    /// The registry, a manifest, or a provider check couldn't be read or is
    /// still unresolved. Never treated as empty.
    Unknown,
    /// Stopped by authority, configuration, a conflict or an inconsistent
    /// record. Not a statement about the candidate.
    Stopped,
}

impl AdoptionOutcome {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Pending => "pending",
            Self::Adopted => "adopted",
            Self::NotApplicable => "not_applicable",
            Self::InsufficientEvidence => "insufficient_evidence",
            Self::NoCandidate => "no_candidate",
            Self::Unknown => "unknown",
            Self::Stopped => "stopped",
        }
    }
}

/// The candidate the conclusion is about, when there is one.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct OutcomeCandidate {
    pub capability_id: CapabilityId,
    pub version: String,
    pub publisher: OrgId,
    pub decision: K,
    pub acceptance_ref: Option<String>,
}

/// A bounded, oracle-free account of one gap adoption's conclusion.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct OutcomeView {
    pub outcome: AdoptionOutcome,
    pub reason: String,
    /// Candidates the registry offered to the current adoption.
    pub considered: usize,
    /// How many candidates got each decision, by decision name.
    pub decisions: BTreeMap<String, usize>,
    pub candidate: Option<OutcomeCandidate>,
}

/// Classify one gap adoption. `checks_settled` is the retry gate's answer
/// for a `checks` stop: every native run definitely ended or never started.
pub fn classify(
    state: S,
    code: Option<C>,
    rows: &[CandidateDecision],
    checks_settled: bool,
) -> OutcomeView {
    let mut decisions = BTreeMap::new();
    for d in rows {
        *decisions.entry(d.decision.as_str().to_owned()).or_insert(0) += 1;
    }
    let focus = rows.iter().find(|d| {
        matches!(
            d.decision,
            K::Adopted
                | K::NotApplicable
                | K::Selected
                | K::InvalidManifest
                | K::UnusableState
                | K::ManifestUnavailable
                | K::InstallationConflict
        )
    });
    let skipped = || {
        let own = decisions
            .get(K::OwnPublication.as_str())
            .copied()
            .unwrap_or(0);
        let scope = decisions
            .get(K::MissingScope.as_str())
            .copied()
            .unwrap_or(0);
        if rows.is_empty() {
            "the registry search answered with no candidates".to_owned()
        } else {
            format!(
                "none of {} candidates was eligible ({own} published by this organization, {scope} needing permissions the role doesn't hold)",
                rows.len()
            )
        }
    };
    let (outcome, reason) = match (state, code) {
        (S::Active, _) => (AdoptionOutcome::Pending, "adoption is still working".into()),
        (S::Installed, _) => (
            AdoptionOutcome::Adopted,
            "passed this organization's own acceptance suite and was installed for the gap".into(),
        ),
        (S::Released, _) | (_, Some(C::NoCandidate)) => (AdoptionOutcome::NoCandidate, skipped()),
        (_, Some(C::Rejected)) => (
            AdoptionOutcome::NotApplicable,
            focus
                .filter(|d| d.decision == K::NotApplicable)
                .map(|d| d.reason.clone())
                .unwrap_or_else(|| {
                    "this organization's acceptance suite rejected the selected candidate".into()
                }),
        ),
        (_, Some(C::Registry)) => (
            AdoptionOutcome::Unknown,
            if focus.is_some_and(|d| d.decision == K::ManifestUnavailable) {
                "a candidate's manifest couldn't be read, so its applicability is unknown"
            } else {
                "the registry couldn't be searched, so it is unknown whether a candidate exists"
            }
            .into(),
        ),
        (_, Some(C::Candidate)) => (
            AdoptionOutcome::InsufficientEvidence,
            focus
                .map(|d| d.reason.clone())
                .or_else(|| {
                    rows.iter()
                        .find(|d| d.decision == K::InvalidResponse)
                        .map(|d| d.reason.clone())
                })
                .unwrap_or_else(|| "the offered candidate couldn't be evaluated".into()),
        ),
        (_, Some(C::Checks)) if checks_settled => (
            AdoptionOutcome::InsufficientEvidence,
            "acceptance checks ended without evidence (a confirmed provider failure), so no verdict was reached".into(),
        ),
        (_, Some(C::Checks)) => (
            AdoptionOutcome::Unknown,
            "an acceptance check's provider outcome is unknown or unresolved; it is not treated as a failure or as empty".into(),
        ),
        (_, Some(C::Authority)) => (
            AdoptionOutcome::Stopped,
            "the requirement, assignment or authority changed; earlier evidence can't authorize this gap".into(),
        ),
        (_, Some(C::Configuration)) => (
            AdoptionOutcome::Stopped,
            "the adoption rule, suite or executor configuration is missing or changed".into(),
        ),
        (_, Some(C::Conflict)) => (
            AdoptionOutcome::Stopped,
            focus
                .filter(|d| d.decision == K::InstallationConflict)
                .map(|d| d.reason.clone())
                .unwrap_or_else(|| {
                    "another adoption, an installation or a limit conflicted".into()
                }),
        ),
        (_, Some(C::InvalidRecord)) => (
            AdoptionOutcome::Stopped,
            "stored records disagree with the frozen intent".into(),
        ),
        (S::Attention | S::Refused, None) => (
            AdoptionOutcome::Stopped,
            "stopped without a recorded code".into(),
        ),
    };
    let candidate = match outcome {
        AdoptionOutcome::NoCandidate | AdoptionOutcome::Pending => None,
        _ => focus.map(|d| OutcomeCandidate {
            capability_id: d.capability_id.clone(),
            version: d.version.clone(),
            publisher: d.publisher.clone(),
            decision: d.decision,
            acceptance_ref: d.acceptance_ref.clone(),
        }),
    };
    OutcomeView {
        outcome,
        reason,
        considered: rows.len(),
        decisions,
        candidate,
    }
}
