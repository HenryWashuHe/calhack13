//! Pre-result generation declarations for trusted host storage. This is not a
//! spend controller, authenticated chronology or proof of exhaustive history.
use crate::method_accounting::{
    assess_costs, AccountingError, CostReport, CostScope, GenerationExpectation,
};
use crate::method_synthesis::{MethodSynthesisPhase, MethodSynthesisSession};
use crate::text_generation::{encode, hash, TextModelSettings};
use genesis_core::development_method::ResourceGrant;
use serde::{Deserialize, Deserializer, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Record {
    schema_version: u32,
    id: String,
    settings: TextModelSettings,
    total_grant: ResourceGrant,
    /// These hashes select the initial Prepared checkpoints, not later results.
    attempts: Vec<GenerationExpectation>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(transparent)]
pub struct GenerationRoster(Record);

impl<'de> Deserialize<'de> for GenerationRoster {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let roster = Self(Record::deserialize(deserializer)?);
        roster.validate().map_err(serde::de::Error::custom)?;
        Ok(roster)
    }
}

#[derive(Debug, Serialize)]
pub struct RosterCostReport {
    pub schema: &'static str,
    pub roster_sha256: String,
    pub costs: CostReport,
}

impl GenerationRoster {
    pub fn total_grant(&self) -> &ResourceGrant {
        &self.0.total_grant
    }

    pub fn attempts(&self) -> &[GenerationExpectation] {
        &self.0.attempts
    }

    pub fn settings(&self) -> &TextModelSettings {
        &self.0.settings
    }

    /// Freeze only unused attempts under one host-selected settings policy.
    /// Input order is part of identity. The roster contains no source/prompt.
    pub fn freeze(
        id: String,
        settings: TextModelSettings,
        total_grant: ResourceGrant,
        sessions: &[MethodSynthesisSession],
    ) -> Result<Self, AccountingError> {
        if sessions.is_empty() || sessions.len() > 64 {
            return Err(AccountingError::Invalid("generation roster size"));
        }
        let mut attempts = Vec::with_capacity(sessions.len());
        for session in sessions {
            session.validate()?;
            if session.phase() != MethodSynthesisPhase::Prepared
                || session.plan().settings != settings
            {
                return Err(AccountingError::Invalid(
                    "roster requires unused attempts with exact settings",
                ));
            }
            attempts.push(GenerationExpectation {
                org_id: session.plan().org_id.clone(),
                operation_id: session.plan().operation_id.clone(),
                plan_sha256: session.plan().sha256()?,
                request_sha256: session.request_sha256().into(),
                checkpoint_sha256: session.checkpoint_sha256().into(),
            });
        }
        let roster = Self(Record {
            schema_version: 1,
            id,
            settings,
            total_grant,
            attempts,
        });
        roster.validate()?;
        Ok(roster)
    }

    pub fn validate(&self) -> Result<(), AccountingError> {
        if self.0.schema_version != 1 {
            return Err(AccountingError::Invalid("generation roster version"));
        }
        self.0.settings.validate()?;
        self.initial_scope().validate()?;
        // Feasibility of declared request count/output caps, not observed usage
        // or enforcement of input tokens, elapsed time or provider behavior.
        let calls = self.0.attempts.len() as u64;
        if calls > self.0.total_grant.model_calls
            || calls * u64::from(self.0.settings.max_tokens) > self.0.total_grant.output_tokens
        {
            return Err(AccountingError::Invalid(
                "declared generation caps exceed roster grant",
            ));
        }
        Ok(())
    }

    fn initial_scope(&self) -> CostScope {
        CostScope {
            id: self.0.id.clone(),
            generations: self.0.attempts.clone(),
            trial: None,
            generated_links: vec![],
            total_grant: self.0.total_grant.clone(),
        }
    }

    pub fn sha256(&self) -> Result<String, AccountingError> {
        self.validate()?;
        Ok(hash(&encode(&("genesis-generation-roster/1", self))?))
    }

    /// Project every declared attempt into the existing exact-snapshot cost
    /// assessor. Missing/unused/failed attempts cannot disappear from the rows.
    /// Trial execution/scoring budgets are intentionally outside this roster.
    pub fn assess(
        &self,
        snapshots: &[MethodSynthesisSession],
    ) -> Result<RosterCostReport, AccountingError> {
        let roster_sha256 = self.sha256()?;
        if snapshots.len() > self.0.attempts.len() {
            return Err(AccountingError::Invalid("too many generation snapshots"));
        }
        let mut scope = self.initial_scope();
        for snapshot in snapshots {
            snapshot.validate()?;
            let plan = snapshot.plan();
            let expected = scope
                .generations
                .iter_mut()
                .find(|entry| {
                    entry.org_id == plan.org_id && entry.operation_id == plan.operation_id
                })
                .ok_or(AccountingError::Invalid("undeclared generation attempt"))?;
            if expected.plan_sha256 != plan.sha256()?
                || expected.request_sha256 != snapshot.request_sha256()
                || self.0.settings != plan.settings
                || expected.checkpoint_sha256 != snapshot.prepared_checkpoint_sha256()?
            {
                return Err(AccountingError::Invalid(
                    "generation differs from declared attempt",
                ));
            }
            // Result hashes necessarily become known after declaration; only
            // the exact matching attempt may supply its current snapshot.
            expected.checkpoint_sha256 = snapshot.checkpoint_sha256().into();
        }
        let costs = assess_costs(&scope, snapshots, None)?;
        Ok(RosterCostReport {
            schema: "genesis-generation-roster-cost/1",
            roster_sha256,
            costs,
        })
    }
}
