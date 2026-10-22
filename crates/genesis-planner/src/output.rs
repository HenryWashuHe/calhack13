//! Parsing and validating model output. Model output is untrusted.

use genesis_core::change::{OrgChangeProposal, OrgOp};
use genesis_core::ids::{ObservationId, RequirementId};
use serde::Deserialize;

pub const REQUIREMENT_PLACEHOLDER: &str = "$REQUIREMENT";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Verdict {
    Change,
    NoChange,
    Escalate,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct RequirementDraft {
    pub summary: String,
    pub acceptance: String,
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct PlannerOutput {
    pub assessment: String,
    pub verdict: Verdict,
    pub requirement: Option<RequirementDraft>,
    pub rationale: String,
    #[serde(default)]
    pub budget_limit_cents: Option<u64>,
    #[serde(default)]
    pub operations: Vec<serde_json::Value>,
}

#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum OutputError {
    #[error("no JSON object found in the response")]
    NoJson,
    #[error("response is not valid planner JSON: {0}")]
    Shape(String),
    #[error("operation {index} is not a valid organization operation: {message}")]
    Operation { index: usize, message: String },
    #[error("verdict is change but {0}")]
    Incomplete(&'static str),
}

/// Extract the outermost JSON object from a model response, tolerating code
/// fences or stray prose around it.
pub fn extract_json(text: &str) -> Option<&str> {
    let start = text.find('{')?;
    let end = text.rfind('}')?;
    (end > start).then(|| &text[start..=end])
}

pub fn parse(text: &str) -> Result<PlannerOutput, OutputError> {
    let json = extract_json(text).ok_or(OutputError::NoJson)?;
    let out: PlannerOutput =
        serde_json::from_str(json).map_err(|e| OutputError::Shape(e.to_string()))?;
    if out.verdict == Verdict::Change {
        if out.requirement.is_none() && out.operations.iter().any(uses_placeholder) {
            return Err(OutputError::Incomplete(
                "operations use $REQUIREMENT without a requirement",
            ));
        }
        if out.operations.is_empty() {
            return Err(OutputError::Incomplete("operations are empty"));
        }
        // Validate shape now, with a stand-in requirement ID.
        typed_operations(&out, &RequirementId::new("req_placeholder"))?;
    }
    Ok(out)
}

fn uses_placeholder(v: &serde_json::Value) -> bool {
    match v {
        serde_json::Value::String(s) => s == REQUIREMENT_PLACEHOLDER,
        serde_json::Value::Array(a) => a.iter().any(uses_placeholder),
        serde_json::Value::Object(o) => o.values().any(uses_placeholder),
        _ => false,
    }
}

fn substitute(v: &serde_json::Value, requirement: &RequirementId) -> serde_json::Value {
    match v {
        serde_json::Value::String(s) if s == REQUIREMENT_PLACEHOLDER => {
            serde_json::Value::String(requirement.to_string())
        }
        serde_json::Value::Array(a) => {
            serde_json::Value::Array(a.iter().map(|x| substitute(x, requirement)).collect())
        }
        serde_json::Value::Object(o) => serde_json::Value::Object(
            o.iter()
                .map(|(k, x)| (k.clone(), substitute(x, requirement)))
                .collect(),
        ),
        other => other.clone(),
    }
}

/// Turn raw operations into typed kernel operations, substituting the
/// requirement placeholder.
pub fn typed_operations(
    out: &PlannerOutput,
    requirement: &RequirementId,
) -> Result<Vec<OrgOp>, OutputError> {
    out.operations
        .iter()
        .enumerate()
        .map(|(index, raw)| {
            serde_json::from_value::<OrgOp>(substitute(raw, requirement)).map_err(|e| {
                OutputError::Operation {
                    index,
                    message: e.to_string(),
                }
            })
        })
        .collect()
}

/// Build the proposal the kernel will validate. The planner, not the model,
/// fills the revision and evidence fields.
pub fn proposal(
    out: &PlannerOutput,
    requirement: Option<&RequirementId>,
    observation: &ObservationId,
    expected_revision: u64,
) -> Result<OrgChangeProposal, OutputError> {
    let stand_in = RequirementId::new("req_unused");
    let operations = typed_operations(out, requirement.unwrap_or(&stand_in))?;
    Ok(OrgChangeProposal {
        expected_org_revision: expected_revision,
        trigger_evidence_ids: vec![observation.clone()],
        rationale: format!("{} {}", out.assessment.trim(), out.rationale.trim())
            .trim()
            .to_owned(),
        operations,
        budget_limit_cents: out.budget_limit_cents,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    const GOOD: &str = r#"Here you go:
```json
{"assessment":"a","verdict":"change","requirement":{"summary":"s","acceptance":"x"},
 "rationale":"r","budget_limit_cents":1000,
 "operations":[
  {"op":"CreateRole","key":"k","name":"N","charter":"C","authority":{"scopes":["a.b"],"budget_cents":10},
   "reports_to":null,"provider":"elastic","justification":"expertise"},
  {"op":"AssignResponsibility","target":{"kind":"from_requirement","key":"r1","requirement_id":"$REQUIREMENT",
   "purpose":"p","trigger":"t","termination":"e"},"owner":{"kind":"new","key":"k"}}]}
```"#;

    #[test]
    fn parses_fenced_output_and_substitutes_requirement() {
        let out = parse(GOOD).unwrap();
        assert_eq!(out.verdict, Verdict::Change);
        let p = proposal(
            &out,
            Some(&RequirementId::new("req_real")),
            &ObservationId::new("obs_1"),
            7,
        )
        .unwrap();
        assert_eq!(p.expected_org_revision, 7);
        assert_eq!(p.trigger_evidence_ids, vec![ObservationId::new("obs_1")]);
        let v = serde_json::to_value(&p.operations[1]).unwrap();
        assert_eq!(v["target"]["requirement_id"], "req_real");
    }

    #[test]
    fn rejects_unknown_operation_fields() {
        let bad = GOOD.replace("\"CreateRole\"", "\"HireEveryone\"");
        assert!(matches!(
            parse(&bad),
            Err(OutputError::Operation { index: 0, .. })
        ));
    }

    #[test]
    fn no_change_needs_no_operations() {
        let out = parse(r#"{"assessment":"a","verdict":"no_change","requirement":null,"rationale":"r","operations":[]}"#)
            .unwrap();
        assert_eq!(out.verdict, Verdict::NoChange);
    }

    #[test]
    fn prose_without_json_is_an_error() {
        assert_eq!(
            parse("I think we should hire someone."),
            Err(OutputError::NoJson)
        );
    }

    #[test]
    fn change_without_operations_is_incomplete() {
        let e = parse(r#"{"assessment":"a","verdict":"change","requirement":{"summary":"s","acceptance":"x"},"rationale":"r","operations":[]}"#)
            .unwrap_err();
        assert!(matches!(e, OutputError::Incomplete(_)));
    }
}
