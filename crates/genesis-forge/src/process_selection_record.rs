//! Existing retained selection wire format, shared by capture and cohort checks.
use crate::process_selection::{select_development, Arm, Family, RoundInput, SelectionReport};
use genesis_core::digest::canonical_sha256;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SelectionInputs {
    pub arm: Arm,
    pub family: Family,
    pub rounds: [RoundInput; 3],
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RetainedSelection {
    pub schema: String,
    pub inputs: SelectionInputs,
    pub report: Value,
}
impl RetainedSelection {
    pub fn capture(inputs: SelectionInputs) -> Result<Self, String> {
        let report = select_development(inputs.arm, inputs.family, &inputs.rounds)?;
        Ok(Self {
            schema: "genesis-retained-process-selection/1".into(),
            inputs,
            report: serde_json::to_value(report).map_err(|e| e.to_string())?,
        })
    }
    pub fn check(&self) -> Result<SelectionReport, String> {
        if self.schema != "genesis-retained-process-selection/1" {
            return Err("unsupported retained selection schema".into());
        }
        let report = select_development(self.inputs.arm, self.inputs.family, &self.inputs.rounds)?;
        if self.report != serde_json::to_value(&report).map_err(|e| e.to_string())? {
            return Err("retained report differs from recomputed selection".into());
        }
        Ok(report)
    }
    /// Canonical parsed JSON, identical to process-select-{prepare,check} output.
    pub fn sha256(&self) -> String {
        canonical_sha256(&json!(self))
    }
}
