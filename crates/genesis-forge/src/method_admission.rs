//! Never-refundable admission for one trusted roster/journal. The caller must
//! exclusively lock and durably save a reservation before any external effect.
use crate::method_accounting::AccountingError;
use crate::method_roster::GenerationRoster;
use crate::method_synthesis::{MethodSynthesisPhase, MethodSynthesisSession};
use crate::text_generation::{encode, hash};
use serde::{Deserialize, Deserializer, Serialize};
use std::collections::BTreeSet;

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Record {
    schema_version: u32,
    roster: GenerationRoster,
    /// Append-only within this API. A reservation is not proof a POST happened.
    reserved_prepared_checkpoints: Vec<String>,
}

#[derive(Clone, Debug, Serialize)]
#[serde(transparent)]
pub struct GenerationAdmission(Record);

impl<'de> Deserialize<'de> for GenerationAdmission {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let admission = Self(Record::deserialize(deserializer)?);
        admission.validate().map_err(serde::de::Error::custom)?;
        Ok(admission)
    }
}

#[derive(Debug, Serialize)]
pub struct AdmissionSummary {
    pub schema: &'static str,
    pub journal_sha256: String,
    pub roster_sha256: String,
    pub reserved_attempts: u64,
    pub remaining_attempts: u64,
    /// Reserved requested maximums, never measured usage or refunded capacity.
    pub reserved_output_token_caps: u64,
    pub reserved_prepared_checkpoints: Vec<String>,
}

impl GenerationAdmission {
    pub fn roster(&self) -> &GenerationRoster {
        &self.0.roster
    }

    pub fn new(roster: GenerationRoster) -> Result<Self, AccountingError> {
        let admission = Self(Record {
            schema_version: 1,
            roster,
            reserved_prepared_checkpoints: vec![],
        });
        admission.validate()?;
        Ok(admission)
    }

    pub fn validate(&self) -> Result<(), AccountingError> {
        self.0.roster.validate()?;
        let attempts = self.0.roster.attempts();
        let reserved = &self.0.reserved_prepared_checkpoints;
        let mut seen = BTreeSet::new();
        if self.0.schema_version != 1
            || reserved.len() > attempts.len()
            || reserved.iter().any(|id| {
                !seen.insert(id)
                    || !attempts
                        .iter()
                        .any(|attempt| &attempt.checkpoint_sha256 == id)
            })
        {
            return Err(AccountingError::Invalid("admission reservation identity"));
        }
        Ok(())
    }

    /// Validate exact declared identity at any phase before checking admission.
    /// A false result never grants retroactive authority for an advanced session.
    pub fn is_reserved(&self, session: &MethodSynthesisSession) -> Result<bool, AccountingError> {
        self.validate()?;
        self.0.roster.assess(std::slice::from_ref(session))?;
        let id = session.prepared_checkpoint_sha256()?;
        Ok(self.0.reserved_prepared_checkpoints.contains(&id))
    }

    /// Burn one exact unused attempt. No release/reset API exists, even if the
    /// request fails or the host crashes before starting the generation intent.
    /// This in-memory transition alone cannot authorize a provider call.
    pub fn reserve(&mut self, session: &MethodSynthesisSession) -> Result<(), AccountingError> {
        let reserved = self.is_reserved(session)?;
        if session.phase() != MethodSynthesisPhase::Prepared || reserved {
            return Err(AccountingError::Invalid(
                "generation must be Prepared and never previously reserved",
            ));
        }
        self.0
            .reserved_prepared_checkpoints
            .push(session.checkpoint_sha256().into());
        Ok(())
    }

    pub fn summary(&self) -> Result<AdmissionSummary, AccountingError> {
        self.validate()?;
        let count = self.0.reserved_prepared_checkpoints.len() as u64;
        Ok(AdmissionSummary {
            schema: "genesis-generation-admission-status/1",
            journal_sha256: hash(&encode(&("genesis-generation-admission/1", self))?),
            roster_sha256: self.0.roster.sha256()?,
            reserved_attempts: count,
            remaining_attempts: self.0.roster.attempts().len() as u64 - count,
            reserved_output_token_caps: count * u64::from(self.0.roster.settings().max_tokens),
            reserved_prepared_checkpoints: self.0.reserved_prepared_checkpoints.clone(),
        })
    }
}
