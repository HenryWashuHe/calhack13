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
        self.kernel
            .record_decision(&DecisionRecord {
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
            })
            .await
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
        let mut messages = vec![Message::user(prompt::user_message(
            &state,
            &obs,
            &self.specialists,
        ))];
        let mut requirement: Option<RequirementId> = None;

        for attempt in 1..=self.max_attempts {
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
                Err(SubmitError::Kernel(e)) if e.code().is_some() => {
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
        if requirement.is_none() {
            if let Some(draft) = &parsed.requirement {
                let id = self
                    .kernel
                    .record_requirement(
                        &obs.mission_id,
                        &draft.summary,
                        std::slice::from_ref(&obs.observation_id),
                        &draft.acceptance,
                    )
                    .await?;
                *requirement = Some(id);
            }
        }
        // A revision conflict means state moved under us: rebuild against the
        // fresh revision once without spending another model call.
        for _ in 0..2 {
            let revision = self.kernel.state().await?.revision;
            let proposal =
                output::proposal(parsed, requirement.as_ref(), &obs.observation_id, revision)
                    .map_err(SubmitError::Output)?;
            let request_id = format!("planner:{}:{}:r{}", obs.observation_id, attempt, revision);
            match self.kernel.submit_change(&request_id, &proposal).await {
                Err(e) if e.code() == Some("revision_conflict") => continue,
                other => return other.map_err(SubmitError::Kernel),
            }
        }
        let revision = self.kernel.state().await?.revision;
        let proposal =
            output::proposal(parsed, requirement.as_ref(), &obs.observation_id, revision)
                .map_err(SubmitError::Output)?;
        self.kernel
            .submit_change(
                &format!("planner:{}:{}:final", obs.observation_id, attempt),
                &proposal,
            )
            .await
            .map_err(SubmitError::Kernel)
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
