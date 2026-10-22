//! The planning loop for one observation.
//!
//! observe -> ask the model -> record the requirement -> submit the change
//! -> on kernel rejection, feed the exact errors back and retry (bounded)
//! -> record a decision event either way.

use crate::kernel::{DecisionRecord, KernelClient, KernelError};
use crate::llm::{LlmClient, LlmRequest, Message};
use crate::output::{self, PlannerOutput, Verdict};
use crate::prompt::{self, KnownSpecialist};
use genesis_core::change::OrgChangeRecord;
use genesis_core::events::PlannerVerdict;
use genesis_core::ids::{ObservationId, RequirementId};
use genesis_core::lifecycle::ChangeStatus;
use std::sync::Arc;
use std::time::Instant;

#[derive(Debug, Clone, PartialEq)]
pub enum PlanOutcome {
    Proposed {
        change: Box<OrgChangeRecord>,
        attempts: u32,
    },
    NoChange {
        attempts: u32,
    },
    Escalated {
        reason: String,
        attempts: u32,
    },
}

#[derive(Debug, thiserror::Error)]
pub enum PlanError {
    #[error(transparent)]
    Kernel(#[from] KernelError),
    #[error("observation {0} not found")]
    UnknownObservation(ObservationId),
}

pub struct Planner {
    pub name: String,
    pub kernel: KernelClient,
    pub llm: Arc<dyn LlmClient>,
    pub specialists: Vec<KnownSpecialist>,
    /// Total model calls allowed per observation, including repairs.
    pub max_attempts: u32,
    pub max_tokens: u32,
}

impl Planner {
    pub fn new(kernel: KernelClient, llm: Arc<dyn LlmClient>) -> Self {
        Self {
            name: "genesis-planner".into(),
            kernel,
            llm,
            specialists: Vec::new(),
            max_attempts: 3,
            max_tokens: 4000,
        }
    }

    #[allow(clippy::too_many_arguments)]
    async fn decide(
        &self,
        obs: &ObservationId,
        verdict: PlannerVerdict,
        summary: &str,
        rationale: &str,
        attempt: u32,
        latency_ms: u64,
        requirement: Option<&RequirementId>,
        change: Option<&OrgChangeRecord>,
        errors: Vec<String>,
    ) -> Result<(), KernelError> {
        let record = DecisionRecord {
            planner: self.name.clone(),
            model: self.llm.model(),
            observation_id: obs.clone(),
            verdict,
            summary: truncate(summary, 400),
            rationale: truncate(rationale, 1200),
            attempt,
            latency_ms,
            requirement_id: requirement.cloned(),
            change_id: change.map(|c| c.change_id.clone()),
            kernel_errors: errors,
        };
        // The kernel records a terminal decision at most once per
        // observation, so retrying after an ambiguous failure is safe.
        let mut tries = 0;
        loop {
            tries += 1;
            match self.kernel.record_decision(&record).await {
                Err(e) if e.is_ambiguous() && tries < 3 => {
                    tokio::time::sleep(std::time::Duration::from_millis(100 * tries)).await;
                }
                other => return other,
            }
        }
    }

    /// Request ID for the `submission`-th change submission of model attempt
    /// `attempt`. Independent of the revision, so it can be reconciled.
    fn request_key(obs: &ObservationId, attempt: u32, submission: u32) -> String {
        format!("planner:{obs}:a{attempt}:s{submission}")
    }

    /// Idempotency identity for this observation's requirement.
    fn requirement_key(obs: &ObservationId) -> String {
        format!("planner:{obs}:requirement")
    }

    /// What an earlier, interrupted run left for this observation: an accepted
    /// change (its decision was never recorded), or the model attempts whose
    /// submissions the kernel already rejected. A rejected attempt's request
    /// keys are spent: resubmitting under them returns the cached rejection.
    async fn prior_attempts(&self, obs: &ObservationId) -> Result<Prior, KernelError> {
        // Scan every key: an attempt whose model output was unusable submits
        // nothing, so a gap does not mean later attempts are unused.
        let mut prior = Prior::default();
        for attempt in 1..=self.max_attempts {
            for submission in 0..3 {
                if let Some(c) = self
                    .kernel
                    .change_by_request(&Self::request_key(obs, attempt, submission))
                    .await?
                {
                    if c.status == ChangeStatus::Rejected {
                        prior.rejections.extend(rejection(&c).reasons());
                        prior.spent = attempt;
                    } else {
                        prior.accepted = Some(c);
                        return Ok(prior);
                    }
                }
            }
        }
        Ok(prior)
    }

    /// Prompt the model would receive for this observation. Works without a
    /// configured model.
    pub async fn preview(
        &self,
        observation: &ObservationId,
    ) -> Result<(String, String), PlanError> {
        let state = self.kernel.state().await?;
        let obs = state
            .observations
            .get(observation)
            .ok_or_else(|| PlanError::UnknownObservation(observation.clone()))?;
        Ok((
            prompt::SYSTEM_PROMPT.to_owned(),
            prompt::user_message(&state, obs, &self.specialists),
        ))
    }

    pub async fn plan(&self, observation: &ObservationId) -> Result<PlanOutcome, PlanError> {
        let state = self.kernel.state().await?;
        let obs = state
            .observations
            .get(observation)
            .cloned()
            .ok_or_else(|| PlanError::UnknownObservation(observation.clone()))?;
        // A requirement recorded for this observation by an earlier run is
        // reused, never duplicated.
        let mut requirement: Option<RequirementId> = existing_requirement(&state, &obs);
        // An interrupted run may already have an accepted change: record the
        // decision without asking the model again.
        let prior = self.prior_attempts(observation).await?;
        if let Some(change) = prior.accepted {
            self.decide(
                observation,
                PlannerVerdict::ChangeProposed,
                "Recovered an accepted change",
                "A change for this observation was accepted by an earlier, interrupted run; its decision is recorded now without a new model call.",
                0,
                0,
                requirement.as_ref(),
                Some(&change),
                vec![],
            )
            .await?;
            return Ok(PlanOutcome::Proposed {
                change: Box::new(change),
                attempts: 0,
            });
        }
        let mut messages = vec![Message::user(prompt::user_message(
            &state,
            &obs,
            &self.specialists,
        ))];
        // Resume after attempts an interrupted run already spent on
        // rejections, and tell the model what the kernel said about them.
        if prior.spent > 0 {
            messages.push(Message::user(format!(
                "An earlier attempt for this observation was rejected by the kernel:\n- {}\n\nAvoid those errors.",
                prior.rejections.join("\n- ")
            )));
        }

        for attempt in (prior.spent + 1)..=self.max_attempts {
            let started = Instant::now();
            let reply = self
                .llm
                .complete(&LlmRequest {
                    system: prompt::SYSTEM_PROMPT.to_owned(),
                    messages: messages.clone(),
                    max_tokens: self.max_tokens,
                })
                .await;
            let latency = started.elapsed().as_millis() as u64;
            let text = match reply {
                Ok(t) => t,
                Err(e) => {
                    let reason = format!("model call failed: {e}");
                    self.decide(
                        observation,
                        PlannerVerdict::Escalated,
                        "Model unavailable",
                        &reason,
                        attempt,
                        latency,
                        requirement.as_ref(),
                        None,
                        vec![],
                    )
                    .await?;
                    return Ok(PlanOutcome::Escalated {
                        reason,
                        attempts: attempt,
                    });
                }
            };

            let parsed = match output::parse(&text) {
                Ok(p) => p,
                Err(e) => {
                    tracing::warn!(%observation, attempt, error = %e, "unusable model output");
                    messages.push(Message::assistant(text));
                    messages.push(Message::user(format!(
                        "Your response could not be used: {e}. Respond again with one JSON object that follows the required format exactly."
                    )));
                    continue;
                }
            };

            match parsed.verdict {
                Verdict::NoChange => {
                    self.decide(
                        observation,
                        PlannerVerdict::NoChange,
                        &parsed.assessment,
                        &parsed.rationale,
                        attempt,
                        latency,
                        requirement.as_ref(),
                        None,
                        vec![],
                    )
                    .await?;
                    return Ok(PlanOutcome::NoChange { attempts: attempt });
                }
                Verdict::Escalate => {
                    self.decide(
                        observation,
                        PlannerVerdict::Escalated,
                        &parsed.assessment,
                        &parsed.rationale,
                        attempt,
                        latency,
                        requirement.as_ref(),
                        None,
                        vec![],
                    )
                    .await?;
                    return Ok(PlanOutcome::Escalated {
                        reason: parsed.rationale,
                        attempts: attempt,
                    });
                }
                Verdict::Change => {}
            }

            match self.submit(&obs, &parsed, &mut requirement, attempt).await {
                Ok(change) => {
                    self.decide(
                        observation,
                        PlannerVerdict::ChangeProposed,
                        &parsed.assessment,
                        &parsed.rationale,
                        attempt,
                        latency,
                        requirement.as_ref(),
                        Some(&change),
                        vec![],
                    )
                    .await?;
                    return Ok(PlanOutcome::Proposed {
                        change: Box::new(change),
                        attempts: attempt,
                    });
                }
                // Only a definite rejection enters repair. An ambiguous
                // outcome (transport, 5xx) is never turned into a new mutation.
                Err(SubmitError::Kernel(e)) if e.is_rejection() => {
                    let reasons = e.reasons();
                    self.decide(
                        observation,
                        PlannerVerdict::Rejected,
                        &parsed.assessment,
                        &parsed.rationale,
                        attempt,
                        latency,
                        requirement.as_ref(),
                        None,
                        reasons.clone(),
                    )
                    .await?;
                    messages.push(Message::assistant(text));
                    let current = self.kernel.state().await?;
                    messages.push(Message::user(format!(
                        "The kernel rejected that proposal:\n- {}\n\nThe organization is now at revision {}. Fix the proposal and respond with the full JSON object again. If it cannot be fixed, use verdict \"escalate\".",
                        reasons.join("\n- "),
                        current.revision
                    )));
                }
                Err(SubmitError::Kernel(e)) => return Err(PlanError::Kernel(e)),
                Err(SubmitError::Output(e)) => {
                    messages.push(Message::assistant(text));
                    messages.push(Message::user(format!(
                        "Your operations could not be used: {e}. Respond again with corrected JSON."
                    )));
                }
            }
        }

        let reason = format!("no valid proposal after {} attempts", self.max_attempts);
        self.decide(
            observation,
            PlannerVerdict::Escalated,
            "Planner gave up",
            &reason,
            self.max_attempts,
            0,
            requirement.as_ref(),
            None,
            vec![],
        )
        .await?;
        Ok(PlanOutcome::Escalated {
            reason,
            attempts: self.max_attempts,
        })
    }

    async fn submit(
        &self,
        obs: &genesis_core::model::Observation,
        parsed: &PlannerOutput,
        requirement: &mut Option<RequirementId>,
        attempt: u32,
    ) -> Result<OrgChangeRecord, SubmitError> {
        // Record the requirement once per observation; reuse it on repairs.
        // The request carries a per-observation idempotency key. After an
        // ambiguous outcome a read that finds the requirement proves the write
        // and this pass continues. A read that finds nothing proves nothing:
        // the pass ends, and the runner's next pass (or a restarted planner)
        // resends the identical request, which the kernel resolves to the same
        // requirement even if the original lands later.
        if requirement.is_none() {
            if let Some(draft) = &parsed.requirement {
                match self
                    .kernel
                    .record_requirement_once(
                        &Self::requirement_key(&obs.observation_id),
                        &obs.mission_id,
                        &draft.summary,
                        std::slice::from_ref(&obs.observation_id),
                        &draft.acceptance,
                    )
                    .await
                {
                    Ok(id) => *requirement = Some(id),
                    Err(e) if e.is_ambiguous() => {
                        *requirement = Some(self.reconcile_requirement(obs).await.ok_or(e)?);
                    }
                    Err(e) => return Err(e.into()),
                }
            }
        }
        // A revision conflict means state moved under us: rebuild against the
        // fresh revision without spending another model call. Each submission
        // has its own deterministic request ID.
        let mut last = None;
        for submission in 0..3 {
            let revision = self.kernel.state().await?.revision;
            let proposal =
                output::proposal(parsed, requirement.as_ref(), &obs.observation_id, revision)
                    .map_err(SubmitError::Output)?;
            let request_id = Self::request_key(&obs.observation_id, attempt, submission);
            match self.kernel.submit_change(&request_id, &proposal).await {
                // An idempotent replay returns the stored record, which may be
                // a rejection: that is a rejection, not an accepted proposal.
                Ok(c) => return accepted(c),
                Err(e) if e.code() == Some("revision_conflict") => last = Some(e),
                Err(e) if e.is_ambiguous() => {
                    // It may have been accepted: reconcile by request ID and
                    // never resubmit an unknown outcome.
                    return match self.reconcile_change(&request_id).await {
                        Some(c) => accepted(c),
                        None => Err(e.into()),
                    };
                }
                Err(e) => return Err(e.into()),
            }
        }
        Err(SubmitError::Kernel(last.expect("loop ran")))
    }

    /// Look for a requirement an ambiguous request may have recorded. `None`
    /// is not evidence that it was not recorded.
    async fn reconcile_requirement(
        &self,
        obs: &genesis_core::model::Observation,
    ) -> Option<RequirementId> {
        for tries in 1..=3u64 {
            if let Ok(state) = self.kernel.state().await {
                if let Some(id) = existing_requirement(&state, obs) {
                    return Some(id);
                }
            }
            tokio::time::sleep(std::time::Duration::from_millis(100 * tries)).await;
        }
        None
    }

    async fn reconcile_change(&self, request_id: &str) -> Option<OrgChangeRecord> {
        for tries in 1..=3u64 {
            match self.kernel.change_by_request(request_id).await {
                Ok(found) => return found,
                Err(_) => tokio::time::sleep(std::time::Duration::from_millis(100 * tries)).await,
            }
        }
        None
    }
}

/// The requirement recorded from exactly this observation, if any.
fn existing_requirement(
    state: &genesis_core::model::OrgState,
    obs: &genesis_core::model::Observation,
) -> Option<RequirementId> {
    state
        .requirements
        .values()
        .find(|r| {
            r.mission_id == obs.mission_id && r.triggering_evidence == [obs.observation_id.clone()]
        })
        .map(|r| r.requirement_id.clone())
}

#[derive(Default)]
struct Prior {
    accepted: Option<OrgChangeRecord>,
    /// Highest model attempt whose submission the kernel rejected; the next
    /// run resumes after it so no spent request key is reused.
    spent: u32,
    rejections: Vec<String>,
}

/// A stored change record, read back after an idempotent replay or
/// reconciliation, expressed as the rejection it records.
fn rejection(c: &OrgChangeRecord) -> KernelError {
    KernelError::Api {
        status: 422,
        code: "change_rejected".into(),
        message: format!("change {} was rejected", c.change_id),
        details: serde_json::json!({ "errors": c.errors }),
    }
}

fn accepted(c: OrgChangeRecord) -> Result<OrgChangeRecord, SubmitError> {
    if c.status == ChangeStatus::Rejected {
        Err(SubmitError::Kernel(rejection(&c)))
    } else {
        Ok(c)
    }
}

#[derive(Debug, thiserror::Error)]
enum SubmitError {
    #[error(transparent)]
    Kernel(#[from] KernelError),
    #[error(transparent)]
    Output(output::OutputError),
}

fn truncate(s: &str, n: usize) -> String {
    if s.chars().count() <= n {
        s.to_owned()
    } else {
        let t: String = s.chars().take(n).collect();
        format!("{t}…")
    }
}
