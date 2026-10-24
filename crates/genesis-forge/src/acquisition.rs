//! Durable local generation -> independent verification -> one bounded repair.
//! This coordinator owns no publication authority or global retry/spend budget.
//! Checkpoints require trusted storage; hashes detect drift, not a hostile writer.
use crate::{
    generated_candidate, ActionCompletion, CandidateIdentity, ForgeAction, ForgeSession,
    ForgeStateError, GenerationReceipt, ModelConfig, Phase, RepairFeedback, SynthesisError,
    SynthesisPhase, SynthesisPrompt, SynthesisRepairContext, SynthesisRequest, SynthesisSession,
    VerifiedCandidate,
};
use genesis_verification::{json_sha256, validate_suite, IssueCode, OracleSuite};
use serde::{Deserialize, Deserializer, Serialize};
use std::collections::BTreeSet;

pub const MAX_ACQUISITION_CHECKPOINT_BYTES: usize = 8 * 1024 * 1024;
pub const MAX_ACQUISITION_BEFORE_EFFECT_BYTES: usize = 6 * 1024 * 1024;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RepairIdentity {
    pub operation_id: String,
    pub candidate: CandidateIdentity,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AcquisitionPlan {
    pub request: SynthesisRequest,
    pub model: ModelConfig,
    pub repair: Option<RepairIdentity>,
}
impl AcquisitionPlan {
    fn validate_public(&self) -> Result<(), AcquisitionError> {
        self.request
            .validate()
            .map_err(|_| AcquisitionError::InvalidPlan)?;
        self.model
            .validate()
            .map_err(|_| AcquisitionError::InvalidPlan)?;
        if let Some(repair) = &self.repair {
            let replacement = self.repair_request()?;
            replacement
                .validate()
                .map_err(|_| AcquisitionError::InvalidPlan)?;
            if repair.operation_id == self.request.operation_id
                || repair.candidate.actor_name == self.request.candidate.actor_name
                || repair.candidate.capability_id != self.request.candidate.capability_id
                || repair.candidate.capability_version == self.request.candidate.capability_version
            {
                return Err(AcquisitionError::InvalidPlan);
            }
        }
        Ok(())
    }
    pub fn validate(&self, suite: &OracleSuite) -> Result<(), AcquisitionError> {
        self.validate_public()?;
        validate_suite(suite).map_err(|_| AcquisitionError::InvalidOracle)?;
        // Oracle selection never edits the public allowlist or enters prompts.
        // The fixed venue Actor must receive the exact allowed source URL that
        // its independent evidence is going to be checked against.
        for case in &suite.cases {
            if !self.request.allowed_source_urls.contains(&case.source.url)
                || case
                    .input
                    .get("source_url")
                    .and_then(|value| value.as_str())
                    != Some(case.source.url.as_str())
            {
                return Err(AcquisitionError::InvalidPlan);
            }
        }
        Ok(())
    }
    fn repair_request(&self) -> Result<SynthesisRequest, AcquisitionError> {
        let repair = self.repair.as_ref().ok_or(AcquisitionError::InvalidPlan)?;
        let mut request = self.request.clone();
        request.operation_id = repair.operation_id.clone();
        request.candidate = repair.candidate.clone();
        Ok(request)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AcquisitionPhase {
    Generating,
    Forging,
    Verified,
    Rejected,
    Failed,
    NeedsReconciliation,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum AcquisitionError {
    InvalidPlan,
    InvalidOracle,
    SuiteChanged,
    CorruptCheckpoint,
    InvalidState,
    NotVerified,
    CheckpointTooLarge,
    Synthesis(SynthesisError),
    Forge(ForgeStateError),
}
impl std::fmt::Display for AcquisitionError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::InvalidPlan => {
                f.write_str("acquisition plan is invalid or incompatible with its oracle")
            }
            Self::InvalidOracle => {
                f.write_str("acquisition requires a valid independent oracle suite")
            }
            Self::SuiteChanged => f.write_str("acquisition oracle suite changed"),
            Self::CorruptCheckpoint => {
                f.write_str("acquisition checkpoint integrity validation failed")
            }
            Self::InvalidState => f.write_str("acquisition operation is not valid in this phase"),
            Self::NotVerified => f.write_str("acquisition has no independently verified candidate"),
            Self::CheckpointTooLarge => {
                f.write_str("acquisition checkpoint exceeds its bounded storage or effect headroom")
            }
            Self::Synthesis(error) => write!(f, "acquisition generation: {error}"),
            Self::Forge(error) => write!(f, "acquisition verification: {error}"),
        }
    }
}
impl std::error::Error for AcquisitionError {}
impl From<SynthesisError> for AcquisitionError {
    fn from(value: SynthesisError) -> Self {
        Self::Synthesis(value)
    }
}
impl From<ForgeStateError> for AcquisitionError {
    fn from(value: ForgeStateError) -> Self {
        Self::Forge(value)
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct ArchivedAttempt {
    generation: SynthesisSession,
    forge: ForgeSession,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct AcquisitionRecord {
    schema_version: u32,
    plan: AcquisitionPlan,
    plan_sha256: String,
    suite_sha256: String,
    generation: SynthesisSession,
    forge: Option<ForgeSession>,
    original: Option<ArchivedAttempt>,
    unchanged_repair: bool,
    phase: AcquisitionPhase,
    checkpoint_sha256: String,
}

/// Persist after every transition and before executing a returned effect. On
/// restore, validate with the independently supplied suite and then recover
/// interrupted intents. `phase()` is status metadata; only `verified(suite)`
/// recomputes and returns semantic acceptance evidence.
#[derive(Clone, Debug, Serialize)]
#[serde(transparent)]
pub struct AcquisitionSession {
    record: AcquisitionRecord,
}
impl<'de> Deserialize<'de> for AcquisitionSession {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let session = Self {
            record: AcquisitionRecord::deserialize(deserializer)?,
        };
        // Oracle-dependent evidence cannot be authenticated at deserialization.
        // Every transition (and verified) additionally requires validate(suite).
        session
            .validate_envelope()
            .map_err(serde::de::Error::custom)?;
        Ok(session)
    }
}
impl AcquisitionSession {
    pub fn new(plan: AcquisitionPlan, suite: &OracleSuite) -> Result<Self, AcquisitionError> {
        plan.validate(suite)?;
        let generation = SynthesisSession::new(plan.request.clone(), plan.model.clone())?;
        let mut session = Self {
            record: AcquisitionRecord {
                schema_version: 1,
                plan_sha256: digest(&plan)?,
                suite_sha256: suite_digest(suite)?,
                plan,
                generation,
                forge: None,
                original: None,
                unchanged_repair: false,
                phase: AcquisitionPhase::Generating,
                checkpoint_sha256: String::new(),
            },
        };
        session.seal()?;
        session.validate(suite)?;
        Ok(session)
    }
    pub fn plan(&self) -> &AcquisitionPlan {
        &self.record.plan
    }
    pub fn phase(&self) -> AcquisitionPhase {
        self.record.phase
    }
    pub fn generation(&self) -> &SynthesisSession {
        &self.record.generation
    }
    pub fn forge(&self) -> Option<&ForgeSession> {
        self.record.forge.as_ref()
    }
    pub fn attempt(&self) -> usize {
        usize::from(self.record.original.is_some())
    }
    pub fn unchanged_repair(&self) -> bool {
        self.record.unchanged_repair
    }

    pub fn validate(&self, suite: &OracleSuite) -> Result<(), AcquisitionError> {
        self.validate_envelope()?;
        if suite_digest(suite)? != self.record.suite_sha256 {
            return Err(AcquisitionError::SuiteChanged);
        }
        self.record.plan.validate(suite)?;
        let r = &self.record;
        r.generation.validate()?;
        if r.generation.config() != &r.plan.model {
            return Err(AcquisitionError::CorruptCheckpoint);
        }
        if let Some(original) = &r.original {
            validate_original_generation(&original.generation, &r.plan)?;
            if original.generation.phase() != SynthesisPhase::Generated
                || original.forge.phase() != Phase::Rejected
            {
                return Err(AcquisitionError::CorruptCheckpoint);
            }
            validate_forge_binding(&original.generation, &original.forge, suite, None)?;
            let feedback = repair_feedback(&original.forge, suite)?
                .ok_or(AcquisitionError::CorruptCheckpoint)?;
            let expected_context = SynthesisRepairContext {
                previous_extractor_typescript: original.generation.extractor()?.into(),
                feedback,
            };
            if r.generation.request() != &r.plan.repair_request()?
                || r.generation.repair_context() != Some(&expected_context)
            {
                return Err(AcquisitionError::CorruptCheckpoint);
            }
            if let Some(forge) = &r.forge {
                if same_parser(&r.generation, &original.generation)? {
                    return Err(AcquisitionError::CorruptCheckpoint);
                }
                validate_forge_binding(&r.generation, forge, suite, Some(&original.forge))?;
            }
        } else {
            validate_original_generation(&r.generation, &r.plan)?;
            if let Some(forge) = &r.forge {
                validate_forge_binding(&r.generation, forge, suite, None)?;
            }
        }
        if r.unchanged_repair {
            let original = r
                .original
                .as_ref()
                .ok_or(AcquisitionError::CorruptCheckpoint)?;
            if r.forge.is_some()
                || r.generation.phase() != SynthesisPhase::Generated
                || !same_parser(&r.generation, &original.generation)?
            {
                return Err(AcquisitionError::CorruptCheckpoint);
            }
        }
        if r.phase != self.derived_phase() {
            return Err(AcquisitionError::CorruptCheckpoint);
        }
        Ok(())
    }

    /// Performs at most one local transition. Call again until it returns false
    /// before deciding whether a status is terminal or an effect is needed.
    pub fn advance_local(&mut self, suite: &OracleSuite) -> Result<bool, AcquisitionError> {
        self.validate(suite)?;
        let mut next = self.clone();
        if next.record.phase == AcquisitionPhase::Generating
            && next.record.generation.phase() == SynthesisPhase::Generated
        {
            if let Some(original) = &next.record.original {
                if same_parser(&next.record.generation, &original.generation)? {
                    next.record.unchanged_repair = true;
                } else {
                    let candidate = generated_candidate(&next.record.generation)?;
                    next.record.forge = Some(original.forge.repair(candidate, suite)?);
                }
            } else {
                next.record.forge = Some(ForgeSession::new(
                    generated_candidate(&next.record.generation)?,
                    suite,
                )?);
            }
        } else if next.record.phase == AcquisitionPhase::Rejected
            && next.record.original.is_none()
            && next.record.plan.repair.is_some()
        {
            let forge = next
                .record
                .forge
                .as_ref()
                .ok_or(AcquisitionError::CorruptCheckpoint)?;
            let Some(feedback) = repair_feedback(forge, suite)? else {
                return Ok(false);
            };
            let generation = SynthesisSession::new_repair(
                next.record.plan.repair_request()?,
                next.record.plan.model.clone(),
                SynthesisRepairContext {
                    previous_extractor_typescript: next.record.generation.extractor()?.into(),
                    feedback,
                },
            )?;
            next.record.original = Some(ArchivedAttempt {
                generation: next.record.generation.clone(),
                forge: forge.clone(),
            });
            next.record.generation = generation;
            next.record.forge = None;
        } else {
            return Ok(false);
        }
        next.finish_transition(suite)?;
        *self = next;
        Ok(true)
    }

    pub fn recover_interrupted(&mut self, suite: &OracleSuite) -> Result<bool, AcquisitionError> {
        self.validate(suite)?;
        let mut next = self.clone();
        let changed = if let Some(forge) = &mut next.record.forge {
            forge.recover_interrupted_submission(suite)?
        } else {
            next.record.generation.recover_interrupted()?
        };
        if !changed {
            return Ok(false);
        }
        next.finish_transition(suite)?;
        *self = next;
        Ok(true)
    }

    pub fn begin_generation(
        &mut self,
        suite: &OracleSuite,
    ) -> Result<SynthesisPrompt, AcquisitionError> {
        self.validate(suite)?;
        if self.record.phase != AcquisitionPhase::Generating || self.record.forge.is_some() {
            return Err(AcquisitionError::InvalidState);
        }
        let mut next = self.clone();
        let prompt = next.record.generation.begin()?;
        next.finish_transition(suite)?;
        next.check_effect_headroom()?;
        *self = next;
        Ok(prompt)
    }
    pub fn complete_generation(
        &mut self,
        result: Result<GenerationReceipt, SynthesisError>,
        suite: &OracleSuite,
    ) -> Result<(), AcquisitionError> {
        self.validate(suite)?;
        if self.record.phase != AcquisitionPhase::Generating || self.record.forge.is_some() {
            return Err(AcquisitionError::InvalidState);
        }
        let mut next = self.clone();
        next.record.generation.complete(result)?;
        next.finish_transition(suite)?;
        *self = next;
        Ok(())
    }
    pub fn prepare_forge(
        &mut self,
        suite: &OracleSuite,
    ) -> Result<Option<ForgeAction>, AcquisitionError> {
        self.validate(suite)?;
        if matches!(
            self.record.phase,
            AcquisitionPhase::Verified | AcquisitionPhase::Rejected
        ) {
            return Ok(None);
        }
        if self.record.phase != AcquisitionPhase::Forging {
            return Err(AcquisitionError::InvalidState);
        }
        let mut next = self.clone();
        let action = next
            .record
            .forge
            .as_mut()
            .ok_or(AcquisitionError::InvalidState)?
            .prepare_next(suite)?;
        next.finish_transition(suite)?;
        next.check_effect_headroom()?;
        *self = next;
        Ok(action)
    }
    pub fn complete_forge(
        &mut self,
        completion: ActionCompletion,
        suite: &OracleSuite,
    ) -> Result<(), AcquisitionError> {
        self.validate(suite)?;
        if self.record.phase != AcquisitionPhase::Forging {
            return Err(AcquisitionError::InvalidState);
        }
        let mut next = self.clone();
        next.record
            .forge
            .as_mut()
            .ok_or(AcquisitionError::InvalidState)?
            .complete(completion, suite)?;
        next.finish_transition(suite)?;
        *self = next;
        Ok(())
    }
    pub fn verified(&self, suite: &OracleSuite) -> Result<VerifiedCandidate, AcquisitionError> {
        self.validate(suite)?;
        if self.record.phase != AcquisitionPhase::Verified {
            return Err(AcquisitionError::NotVerified);
        }
        self.record
            .forge
            .as_ref()
            .ok_or(AcquisitionError::NotVerified)?
            .verified(suite)
            .map_err(Into::into)
    }

    fn validate_envelope(&self) -> Result<(), AcquisitionError> {
        self.record.plan.validate_public()?;
        if self.record.schema_version != 1
            || self.record.plan_sha256 != digest(&self.record.plan)?
            || self.record.checkpoint_sha256 != self.digest()?
            || self.record.suite_sha256.len() != 64
            || !self
                .record
                .suite_sha256
                .bytes()
                .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
        {
            return Err(AcquisitionError::CorruptCheckpoint);
        }
        if encoded_size(self)? > MAX_ACQUISITION_CHECKPOINT_BYTES {
            return Err(AcquisitionError::CheckpointTooLarge);
        }
        Ok(())
    }
    fn check_effect_headroom(&self) -> Result<(), AcquisitionError> {
        if encoded_size(self)? > MAX_ACQUISITION_BEFORE_EFFECT_BYTES {
            return Err(AcquisitionError::CheckpointTooLarge);
        }
        Ok(())
    }
    fn derived_phase(&self) -> AcquisitionPhase {
        if self.record.unchanged_repair {
            return AcquisitionPhase::Rejected;
        }
        if let Some(forge) = &self.record.forge {
            return match forge.phase() {
                Phase::Verified => AcquisitionPhase::Verified,
                Phase::Rejected => AcquisitionPhase::Rejected,
                Phase::Failed => AcquisitionPhase::Failed,
                Phase::NeedsReconciliation => AcquisitionPhase::NeedsReconciliation,
                _ => AcquisitionPhase::Forging,
            };
        }
        match self.record.generation.phase() {
            SynthesisPhase::Failed => AcquisitionPhase::Failed,
            SynthesisPhase::NeedsReconciliation => AcquisitionPhase::NeedsReconciliation,
            _ => AcquisitionPhase::Generating,
        }
    }
    fn finish_transition(&mut self, suite: &OracleSuite) -> Result<(), AcquisitionError> {
        self.record.phase = self.derived_phase();
        self.seal()?;
        self.validate(suite)
    }
    fn digest(&self) -> Result<String, AcquisitionError> {
        digest(&(
            "genesis-acquisition-v1",
            self.record.schema_version,
            &self.record.plan_sha256,
            &self.record.suite_sha256,
            &self.record.generation,
            &self.record.forge,
            &self.record.original,
            self.record.unchanged_repair,
            self.record.phase,
        ))
    }
    fn seal(&mut self) -> Result<(), AcquisitionError> {
        self.record.checkpoint_sha256 = self.digest()?;
        Ok(())
    }
}

fn validate_original_generation(
    generation: &SynthesisSession,
    plan: &AcquisitionPlan,
) -> Result<(), AcquisitionError> {
    generation.validate()?;
    if generation.request() != &plan.request
        || generation.config() != &plan.model
        || generation.repair_context().is_some()
    {
        return Err(AcquisitionError::CorruptCheckpoint);
    }
    Ok(())
}
fn validate_forge_binding(
    generation: &SynthesisSession,
    forge: &ForgeSession,
    suite: &OracleSuite,
    parent: Option<&ForgeSession>,
) -> Result<(), AcquisitionError> {
    forge.validate(suite)?;
    let mut expected = generated_candidate(generation)?;
    if expected.fingerprint()? != forge.spec().fingerprint()? {
        // Preserve exact v1 packages already frozen in durable records. This
        // validates old bytes; it does not repack, upgrade or retry their effects.
        expected = crate::synthesis_source::legacy_generated_candidate(generation)?;
        if expected.fingerprint()? != forge.spec().fingerprint()? {
            return Err(AcquisitionError::CorruptCheckpoint);
        }
    }
    let expected_lineage = if let Some(parent) = parent {
        parent.repair(expected, suite)?
    } else {
        ForgeSession::new(expected, suite)?
    };
    if forge.repair_count() != expected_lineage.repair_count()
        || forge.parent_operation_id() != expected_lineage.parent_operation_id()
    {
        return Err(AcquisitionError::CorruptCheckpoint);
    }
    Ok(())
}
fn same_parser(
    generation: &SynthesisSession,
    original: &SynthesisSession,
) -> Result<bool, AcquisitionError> {
    let current = generation
        .receipt()
        .ok_or(AcquisitionError::CorruptCheckpoint)?;
    let previous = original
        .receipt()
        .ok_or(AcquisitionError::CorruptCheckpoint)?;
    Ok(current.extractor_sha256() == previous.extractor_sha256())
}
fn repair_feedback(
    forge: &ForgeSession,
    suite: &OracleSuite,
) -> Result<Option<Vec<RepairFeedback>>, AcquisitionError> {
    if forge.phase() != Phase::Rejected {
        return Ok(None);
    }
    let report = forge.report(suite)?;
    let mut feedback = BTreeSet::new();
    for issue in report
        .issues
        .iter()
        .chain(report.cases.iter().flat_map(|case| case.issues.iter()))
    {
        let category = match issue.code {
            IssueCode::MalformedOutput => RepairFeedback::MalformedOutput,
            IssueCode::InvalidOutput => RepairFeedback::InvalidOutput,
            IssueCode::OutcomeMismatch => RepairFeedback::OutcomeMismatch,
            IssueCode::MissingVenue => RepairFeedback::MissingVenue,
            IssueCode::UnexpectedVenue => RepairFeedback::UnexpectedVenue,
            IssueCode::DuplicateVenue => RepairFeedback::DuplicateVenue,
            IssueCode::ValueMismatch => RepairFeedback::ValueMismatch,
            // Every new verifier category is denied by default. In particular,
            // receipt/artifact/input/source/run failures cannot trigger repair.
            _ => return Ok(None),
        };
        feedback.insert(category);
    }
    if feedback.is_empty() {
        return Ok(None);
    }
    Ok(Some(feedback.into_iter().collect()))
}
fn suite_digest(suite: &OracleSuite) -> Result<String, AcquisitionError> {
    validate_suite(suite).map_err(|_| AcquisitionError::InvalidOracle)?;
    Ok(json_sha256(
        &serde_json::to_value(suite).map_err(|_| AcquisitionError::InvalidOracle)?,
    ))
}
fn digest(value: &impl Serialize) -> Result<String, AcquisitionError> {
    // Forge evidence contains arbitrary JSON objects; canonical hashing keeps
    // object key order and serde_json/preserve_order from changing checkpoints.
    let value = serde_json::to_value(value).map_err(|_| AcquisitionError::CorruptCheckpoint)?;
    Ok(json_sha256(&value))
}
fn encoded_size(value: &impl Serialize) -> Result<usize, AcquisitionError> {
    Ok(serde_json::to_vec(value)
        .map_err(|_| AcquisitionError::CorruptCheckpoint)?
        .len())
}
