use crate::{CandidateSpec, ForgeStateError, SourceManifest};
use genesis_apify::{ActorTicket, ApifyClient, BuildTicket, BuiltActor, RunRequest, RunTicket};
use genesis_verification::{
    json_sha256, validate_artifact, validate_suite, verify, ArtifactPin, ExecutionEvidence,
    OracleSuite, VerificationOutcome, VerificationReport,
};
use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Phase {
    Prepared,
    ActorCreated,
    BuildSubmitted,
    Testing,
    Verified,
    Rejected,
    Failed,
    NeedsReconciliation,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ActionKind {
    CreateActor,
    StartBuild,
    PollBuild,
    SubmitCase,
    CollectCase,
}
impl ActionKind {
    pub fn is_mutating(self) -> bool {
        matches!(
            self,
            Self::CreateActor | Self::StartBuild | Self::SubmitCase
        )
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Pending {
    sequence: u64,
    kind: ActionKind,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FailureRecord {
    pub summary: String,
    pub retryable: bool,
    pub ambiguous: bool,
    pub actor_id: Option<String>,
    pub build_id: Option<String>,
    pub run_id: Option<String>,
}

/// Store this entire checkpoint in trusted kernel storage. It contains source and
/// run outputs, but never oracle answers. A phase label is not an approval token.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ForgeSession {
    schema_version: u32,
    spec: CandidateSpec,
    request_sha256: String,
    suite_sha256: String,
    phase: Phase,
    sequence: u64,
    pending: Option<Pending>,
    actor: Option<ActorTicket>,
    build: Option<BuildTicket>,
    pin: Option<ArtifactPin>,
    run: Option<RunTicket>,
    receipts: Vec<ExecutionEvidence>,
    failure: Option<FailureRecord>,
    repair_count: u8,
    parent_operation_id: Option<String>,
}

/// Created only by `prepare_next`. Consume it once, after saving the checkpoint.
/// An action is intentionally not deserializable or clonable for POST replay.
pub struct ForgeAction {
    sequence: u64,
    request_sha256: String,
    context_sha256: String,
    kind: ActionKind,
    spec: CandidateSpec,
    actor: Option<ActorTicket>,
    build: Option<BuildTicket>,
    request: Option<RunRequest>,
    run: Option<RunTicket>,
}

/// Only the adapter-backed executor can construct a completion. `complete`
/// rejects old/foreign action results before changing the checkpoint.
pub struct ActionCompletion {
    sequence: u64,
    request_sha256: String,
    context_sha256: String,
    kind: ActionKind,
    result: Result<Effect, FailureRecord>,
}
enum Effect {
    Actor(ActorTicket),
    Build(BuildTicket),
    Built(BuiltActor),
    Run(RunTicket),
    Output(ExecutionEvidence),
}

/// A manifest draft for kernel policy, not a publication or installation grant.
/// Restore a checkpoint and call `verified` again rather than deserializing this.
#[derive(Clone, Debug, Serialize)]
pub struct VerifiedCandidate {
    pub operation_id: String,
    pub parent_operation_id: Option<String>,
    pub artifact: ArtifactPin,
    pub source: SourceManifest,
    pub verification: VerificationReport,
}

impl ForgeSession {
    pub fn new(spec: CandidateSpec, suite: &OracleSuite) -> Result<Self, ForgeStateError> {
        let request_sha256 = spec.fingerprint()?;
        spec.definition()
            .validate()
            .map_err(|_| ForgeStateError::InvalidCandidate)?;
        let suite_sha256 = suite_digest(suite)?;
        Ok(Self {
            schema_version: 1,
            spec,
            request_sha256,
            suite_sha256,
            phase: Phase::Prepared,
            sequence: 0,
            pending: None,
            actor: None,
            build: None,
            pin: None,
            run: None,
            receipts: Vec::new(),
            failure: None,
            repair_count: 0,
            parent_operation_id: None,
        })
    }
    pub fn phase(&self) -> Phase {
        self.phase
    }
    pub fn spec(&self) -> &CandidateSpec {
        &self.spec
    }
    pub fn failure(&self) -> Option<&FailureRecord> {
        self.failure.as_ref()
    }
    pub fn completed_cases(&self) -> usize {
        self.receipts.len()
    }
    pub fn artifact(&self) -> Option<&ArtifactPin> {
        self.pin.as_ref()
    }
    pub fn pending_action(&self) -> Option<ActionKind> {
        self.pending.as_ref().map(|p| p.kind)
    }

    /// Record that a process restarted with an unacknowledged POST intent.
    /// Without a receipt it cannot know whether the remote effect occurred.
    /// This changes only local state and never retries the submission.
    pub fn recover_interrupted_submission(
        &mut self,
        suite: &OracleSuite,
    ) -> Result<bool, ForgeStateError> {
        self.validate(suite)?;
        if matches!(
            self.phase,
            Phase::Verified | Phase::Rejected | Phase::Failed | Phase::NeedsReconciliation
        ) || !self.pending.as_ref().is_some_and(|p| p.kind.is_mutating())
        {
            return Ok(false);
        }
        self.phase = Phase::NeedsReconciliation;
        self.failure = Some(FailureRecord {
            summary: "process interrupted after a persisted submission intent".into(),
            retryable: false,
            ambiguous: true,
            actor_id: self.actor.as_ref().map(|a| a.actor_id().into()),
            build_id: self.build.as_ref().map(|b| b.build_id().into()),
            run_id: self.run.as_ref().map(|r| r.run_id().into()),
        });
        Ok(true)
    }

    /// Revalidate every restored checkpoint against its immutable request and
    /// oracle. Persisted terminal labels alone never establish acceptance.
    pub fn validate(&self, suite: &OracleSuite) -> Result<(), ForgeStateError> {
        if self.schema_version != 1
            || self.repair_count > 1
            || self.spec.fingerprint()? != self.request_sha256
            || self.receipts.len() > suite.cases.len()
        {
            return Err(ForgeStateError::CorruptCheckpoint);
        }
        if suite_digest(suite)? != self.suite_sha256 {
            return Err(ForgeStateError::SuiteChanged);
        }
        let definition = self.spec.definition();
        definition
            .validate()
            .map_err(|_| ForgeStateError::InvalidCandidate)?;
        if let Some(actor) = &self.actor {
            actor
                .validate_for(&definition)
                .map_err(|_| ForgeStateError::CorruptCheckpoint)?;
        }
        if let Some(build) = &self.build {
            let actor = self
                .actor
                .as_ref()
                .ok_or(ForgeStateError::CorruptCheckpoint)?;
            build
                .validate_for(&definition, actor)
                .map_err(|_| ForgeStateError::CorruptCheckpoint)?;
        }
        if let Some(pin) = &self.pin {
            validate_artifact(pin).map_err(|_| ForgeStateError::CorruptCheckpoint)?;
            let build = self
                .build
                .as_ref()
                .ok_or(ForgeStateError::CorruptCheckpoint)?;
            if pin.capability_id != self.spec.capability_id
                || pin.capability_version != self.spec.capability_version
                || pin.source_sha256 != self.spec.source.sha256()?
                || pin.actor_id != build.actor_id()
                || pin.build_id != build.build_id()
                || pin.build_number != build.build_number()
            {
                return Err(ForgeStateError::CorruptCheckpoint);
            }
        }
        let layout_valid = match self.phase {
            Phase::Prepared => self.actor.is_none() && self.build.is_none() && self.pin.is_none(),
            Phase::ActorCreated => {
                self.actor.is_some() && self.build.is_none() && self.pin.is_none()
            }
            Phase::BuildSubmitted => {
                self.actor.is_some() && self.build.is_some() && self.pin.is_none()
            }
            Phase::Testing => self.pin.is_some() && self.receipts.len() < suite.cases.len(),
            Phase::Verified | Phase::Rejected => {
                self.pin.is_some()
                    && self.run.is_none()
                    && self.pending.is_none()
                    && self.receipts.len() == suite.cases.len()
            }
            Phase::Failed | Phase::NeedsReconciliation => self.failure.is_some(),
        };
        if !layout_valid
            || (self.pin.is_none() && (!self.receipts.is_empty() || self.run.is_some()))
        {
            return Err(ForgeStateError::CorruptCheckpoint);
        }
        if let Some(run) = &self.run {
            let case = suite
                .cases
                .get(self.receipts.len())
                .ok_or(ForgeStateError::CorruptCheckpoint)?;
            let request = RunRequest {
                case_id: case.case_id.clone(),
                artifact: self.pin.clone().ok_or(ForgeStateError::CorruptCheckpoint)?,
                input: case.input.clone(),
                source: case.source.clone(),
            };
            run.validate_for(&request)
                .map_err(|_| ForgeStateError::CorruptCheckpoint)?;
        }
        if let Some(pending) = &self.pending {
            if pending.sequence != self.sequence || pending.sequence == 0 {
                return Err(ForgeStateError::CorruptCheckpoint);
            }
            if !matches!(self.phase, Phase::NeedsReconciliation | Phase::Failed)
                && pending.kind != self.next_kind()?
            {
                return Err(ForgeStateError::CorruptCheckpoint);
            }
        }
        // Validate saved evidence against the correct case/pin even before the
        // full suite completes. A restored successful label must re-pass verify.
        if let Some(pin) = &self.pin {
            for (receipt, case) in self.receipts.iter().zip(&suite.cases) {
                if receipt.case_id != case.case_id
                    || receipt.artifact != *pin
                    || receipt.input_sha256 != json_sha256(&case.input)
                    || receipt.source != case.source
                {
                    return Err(ForgeStateError::CorruptCheckpoint);
                }
            }
            if matches!(self.phase, Phase::Verified | Phase::Rejected) {
                let accepted = verify(suite, pin, &self.receipts)
                    .map_err(|_| ForgeStateError::InvalidOracle)?
                    .is_accepted();
                if accepted != (self.phase == Phase::Verified) {
                    return Err(ForgeStateError::CorruptCheckpoint);
                }
            }
        }
        Ok(())
    }

    /// Mutates the checkpoint to record intent BEFORE external work. Save the
    /// checkpoint durably before `action.execute`. A pending POST is never
    /// returned twice; after a crash it requires explicit reconciliation.
    pub fn prepare_next(
        &mut self,
        suite: &OracleSuite,
    ) -> Result<Option<ForgeAction>, ForgeStateError> {
        self.validate(suite)?;
        match self.phase {
            Phase::Verified | Phase::Rejected => return Ok(None),
            Phase::NeedsReconciliation => return Err(ForgeStateError::NeedsReconciliation),
            Phase::Failed => return Err(ForgeStateError::Terminal),
            _ => {}
        }
        let kind = self.next_kind()?;
        if let Some(pending) = &self.pending {
            if pending.kind.is_mutating() {
                return Err(ForgeStateError::NeedsReconciliation);
            }
        } else {
            self.sequence = self
                .sequence
                .checked_add(1)
                .ok_or(ForgeStateError::CorruptCheckpoint)?;
            self.pending = Some(Pending {
                sequence: self.sequence,
                kind,
            });
        }
        let request = if matches!(kind, ActionKind::SubmitCase | ActionKind::CollectCase) {
            let case = suite
                .cases
                .get(self.receipts.len())
                .ok_or(ForgeStateError::CorruptCheckpoint)?;
            Some(RunRequest {
                case_id: case.case_id.clone(),
                artifact: self.pin.clone().ok_or(ForgeStateError::CorruptCheckpoint)?,
                input: case.input.clone(),
                source: case.source.clone(),
            })
        } else {
            None
        };
        Ok(Some(ForgeAction {
            sequence: self.sequence,
            request_sha256: self.request_sha256.clone(),
            context_sha256: self.action_context()?,
            kind,
            spec: self.spec.clone(),
            actor: self.actor.clone(),
            build: self.build.clone(),
            request,
            run: self.run.clone(),
        }))
    }

    pub fn complete(
        &mut self,
        completion: ActionCompletion,
        suite: &OracleSuite,
    ) -> Result<(), ForgeStateError> {
        self.validate(suite)?;
        // Failed or misrouted completions must leave the original checkpoint
        // unchanged, including when the final semantic/state checks fail.
        let mut next = self.clone();
        next.apply_completion(completion, suite)?;
        *self = next;
        Ok(())
    }

    fn apply_completion(
        &mut self,
        completion: ActionCompletion,
        suite: &OracleSuite,
    ) -> Result<(), ForgeStateError> {
        let pending = self
            .pending
            .as_ref()
            .ok_or(ForgeStateError::OutcomeMismatch)?;
        if completion.sequence != pending.sequence
            || completion.kind != pending.kind
            || completion.request_sha256 != self.request_sha256
            || completion.context_sha256 != self.action_context()?
        {
            return Err(ForgeStateError::OutcomeMismatch);
        }
        match completion.result {
            Err(failure) => {
                if completion.kind.is_mutating() && failure.ambiguous {
                    self.phase = Phase::NeedsReconciliation;
                } else if !failure.retryable || completion.kind.is_mutating() {
                    self.phase = Phase::Failed;
                }
                // Retryable reads retain the pending action; prepare_next can
                // re-issue that read after the caller's backoff/budget checks.
                self.failure = Some(failure);
            }
            Ok(effect) => {
                match effect {
                    Effect::Actor(actor) => {
                        self.actor = Some(actor);
                        self.phase = Phase::ActorCreated;
                    }
                    Effect::Build(build) => {
                        self.build = Some(build);
                        self.phase = Phase::BuildSubmitted;
                    }
                    Effect::Built(built) => {
                        self.pin = Some(ArtifactPin {
                            capability_id: self.spec.capability_id.clone(),
                            capability_version: self.spec.capability_version.clone(),
                            source_sha256: self.spec.source.sha256()?,
                            actor_id: built.actor_id().into(),
                            build_id: built.build_id().into(),
                            build_number: built.build_number().into(),
                        });
                        self.phase = Phase::Testing;
                    }
                    Effect::Run(run) => self.run = Some(run),
                    Effect::Output(output) => {
                        self.receipts.push(output);
                        self.run = None;
                    }
                }
                self.pending = None;
                self.failure = None;
                if self.phase == Phase::Testing && self.receipts.len() == suite.cases.len() {
                    let outcome = verify(
                        suite,
                        self.pin
                            .as_ref()
                            .ok_or(ForgeStateError::CorruptCheckpoint)?,
                        &self.receipts,
                    )
                    .map_err(|_| ForgeStateError::InvalidOracle)?;
                    self.phase = if outcome.is_accepted() {
                        Phase::Verified
                    } else {
                        Phase::Rejected
                    };
                }
            }
        }
        self.validate(suite)
    }

    pub fn report(&self, suite: &OracleSuite) -> Result<VerificationReport, ForgeStateError> {
        self.validate(suite)?;
        if !matches!(self.phase, Phase::Verified | Phase::Rejected) {
            return Err(ForgeStateError::NotVerified);
        }
        Ok(verify(
            suite,
            self.pin
                .as_ref()
                .ok_or(ForgeStateError::CorruptCheckpoint)?,
            &self.receipts,
        )
        .map_err(|_| ForgeStateError::InvalidOracle)?
        .report()
        .clone())
    }

    pub fn verified(&self, suite: &OracleSuite) -> Result<VerifiedCandidate, ForgeStateError> {
        self.validate(suite)?;
        if self.phase != Phase::Verified {
            return Err(ForgeStateError::NotVerified);
        }
        let pin = self
            .pin
            .as_ref()
            .ok_or(ForgeStateError::CorruptCheckpoint)?;
        match verify(suite, pin, &self.receipts).map_err(|_| ForgeStateError::InvalidOracle)? {
            VerificationOutcome::Accepted(accepted) => Ok(VerifiedCandidate {
                operation_id: self.spec.operation_id.clone(),
                parent_operation_id: self.parent_operation_id.clone(),
                artifact: pin.clone(),
                source: self.spec.source.manifest()?,
                verification: accepted.report().clone(),
            }),
            _ => Err(ForgeStateError::NotVerified),
        }
    }

    /// One repair within this persisted lineage. Kernel policy must enforce
    /// global budgets; creating a brand new session is not a budget authority.
    pub fn repair(
        &self,
        replacement: CandidateSpec,
        suite: &OracleSuite,
    ) -> Result<Self, ForgeStateError> {
        self.validate(suite)?;
        if self.phase != Phase::Rejected
            || self.repair_count >= 1
            || replacement.operation_id == self.spec.operation_id
            || replacement.actor_name == self.spec.actor_name
            || replacement.capability_id != self.spec.capability_id
            || replacement.capability_version == self.spec.capability_version
            || replacement.source.sha256()? == self.spec.source.sha256()?
        {
            return Err(ForgeStateError::RepairNotAllowed);
        }
        let mut next = Self::new(replacement, suite)?;
        next.repair_count = self.repair_count + 1;
        next.parent_operation_id = Some(self.spec.operation_id.clone());
        Ok(next)
    }

    fn next_kind(&self) -> Result<ActionKind, ForgeStateError> {
        match self.phase {
            Phase::Prepared => Ok(ActionKind::CreateActor),
            Phase::ActorCreated => Ok(ActionKind::StartBuild),
            Phase::BuildSubmitted => Ok(ActionKind::PollBuild),
            Phase::Testing if self.run.is_some() => Ok(ActionKind::CollectCase),
            Phase::Testing => Ok(ActionKind::SubmitCase),
            _ => Err(ForgeStateError::Terminal),
        }
    }

    fn action_context(&self) -> Result<String, ForgeStateError> {
        let pending = self
            .pending
            .as_ref()
            .ok_or(ForgeStateError::OutcomeMismatch)?;
        Ok(json_sha256(&serde_json::json!({
            "request": self.request_sha256,
            "suite": self.suite_sha256,
            "sequence": pending.sequence,
            "kind": pending.kind,
            "actor": self.actor,
            "build": self.build,
            "pin": self.pin,
            "run": self.run,
            "completed_cases": self.receipts.len(),
        })))
    }
}

impl ForgeAction {
    pub fn kind(&self) -> ActionKind {
        self.kind
    }
    pub fn sequence(&self) -> u64 {
        self.sequence
    }
    pub async fn execute(self, client: &ApifyClient) -> ActionCompletion {
        let definition = self.spec.definition();
        // All prerequisites are constructed by prepare_next from a validated
        // checkpoint. Missing state is still handled without panicking.
        let result = match self.kind {
            ActionKind::CreateActor => client
                .create_actor(&definition)
                .await
                .map(Effect::Actor)
                .map_err(forge_failure),
            ActionKind::StartBuild => match self.actor.as_ref() {
                Some(actor) => client
                    .start_build(&definition, actor)
                    .await
                    .map(Effect::Build)
                    .map_err(forge_failure),
                None => Err(invalid_action()),
            },
            ActionKind::PollBuild => match (self.actor.as_ref(), self.build.as_ref()) {
                (Some(actor), Some(build)) => client
                    .resume_build(&definition, actor, build)
                    .await
                    .map(Effect::Built)
                    .map_err(forge_failure),
                _ => Err(invalid_action()),
            },
            ActionKind::SubmitCase => match self.request.as_ref() {
                Some(request) => client
                    .submit(request)
                    .await
                    .map(Effect::Run)
                    .map_err(run_failure),
                None => Err(invalid_action()),
            },
            ActionKind::CollectCase => match (self.request.as_ref(), self.run.as_ref()) {
                (Some(request), Some(ticket)) => client
                    .resume(request, ticket)
                    .await
                    .map(Effect::Output)
                    .map_err(run_failure),
                _ => Err(invalid_action()),
            },
        };
        ActionCompletion {
            sequence: self.sequence,
            request_sha256: self.request_sha256,
            context_sha256: self.context_sha256,
            kind: self.kind,
            result,
        }
    }
}

fn suite_digest(suite: &OracleSuite) -> Result<String, ForgeStateError> {
    validate_suite(suite).map_err(|_| ForgeStateError::InvalidOracle)?;
    let value = serde_json::to_value(suite).map_err(|_| ForgeStateError::InvalidOracle)?;
    Ok(json_sha256(&value))
}
fn forge_failure(error: genesis_apify::ForgeError) -> FailureRecord {
    use genesis_apify::ForgeErrorKind::*;
    let retryable = matches!(
        error.kind,
        Transport | DeadlineExceeded | HttpStatus(429) | HttpStatus(500..=599)
    );
    FailureRecord {
        summary: error.to_string(),
        retryable,
        ambiguous: error.submission_may_have_occurred,
        actor_id: error.actor_id,
        build_id: error.build_id,
        run_id: None,
    }
}
fn run_failure(error: genesis_apify::Error) -> FailureRecord {
    use genesis_apify::ErrorKind::*;
    let retryable = matches!(
        error.kind,
        Transport | DeadlineExceeded | HttpStatus(429) | HttpStatus(500..=599)
    );
    FailureRecord {
        summary: error.to_string(),
        retryable,
        ambiguous: error.submission_may_have_occurred,
        actor_id: None,
        build_id: None,
        run_id: error.run_id,
    }
}
fn invalid_action() -> FailureRecord {
    FailureRecord {
        summary: "invalid forge action state".into(),
        retryable: false,
        ambiguous: false,
        actor_id: None,
        build_id: None,
        run_id: None,
    }
}
