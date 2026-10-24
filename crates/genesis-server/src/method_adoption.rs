//! Receiver-side acceptance check for one exact selected development method.
//!
//! The evidence is never supplied by the request. It is read from a retained
//! trial directory under the receiver's own configured records root, in the
//! layout the lab tools write (`plan.json`, `criterion.json`, `candidate/`,
//! `candidate-score/`, `control/`, `control-score/`). Runtime, protocol,
//! environments, evaluators and allowed dependencies come from the receiver's
//! configuration, never from the offered bytes or the plan. The verdict is
//! recomputed from those records; that establishes internal consistency, not
//! trustworthy measurement. Nothing here authorizes or executes work.
use genesis_core::development_evaluation::{EvaluationOutcome, EvaluationReceipt};
use genesis_core::development_method::{AcceptanceRole, ExactContentRef, ExecutionOutcome};
use genesis_core::development_package::MethodPackage;
use genesis_core::development_trial::{TrialPlan, TrialPurpose};
use genesis_core::development_verdict::{decide, SlotEvidence, Verdict};
use genesis_core::ids::OrgId;
use serde::{de::DeserializeOwned, Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

const MAX_RECORD_FILE_BYTES: u64 = 1024 * 1024;

/// What this organization can run and which retained records it trusts.
/// Loaded from operator configuration only.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MethodReceiver {
    pub runtime: ExactContentRef,
    pub protocol: ExactContentRef,
    pub environments: Vec<ExactContentRef>,
    pub evaluators: Vec<ExactContentRef>,
    pub allowed_dependencies: Vec<ExactContentRef>,
    pub records_root: PathBuf,
}

/// One deliberately selected registry offer and its exact canonical bytes.
#[derive(Debug, Clone)]
pub struct SelectedMethod {
    pub publisher: OrgId,
    pub reference: ExactContentRef,
    pub package_json: String,
}

/// The receiver's acceptance outcome. Unknown execution or scoring wins over
/// every verdict; otherwise the recomputed verdict decides.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "acceptance", rename_all = "snake_case")]
pub enum Acceptance {
    Accepted {
        candidate: u64,
        control: u64,
        of: u64,
    },
    Rejected {
        reason: String,
    },
    Insufficient {
        reason: String,
    },
    Unknown {
        reason: String,
    },
}

/// What the acceptance was computed from, pinned by digest.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AcceptanceEvidence {
    pub trial: String,
    pub plan: ExactContentRef,
    pub criterion: ExactContentRef,
    pub public_task: ExactContentRef,
    pub evaluator: ExactContentRef,
    pub environment: ExactContentRef,
    pub dependencies: Vec<ExactContentRef>,
    /// Retained file (relative to the trial) → SHA-256 of the bytes read.
    pub files: BTreeMap<String, String>,
    pub acceptance: Acceptance,
}

#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum MethodAcceptanceError {
    #[error("invalid trial name")]
    TrialName,
    #[error("the selected method is this organization's own publication")]
    OwnPublication,
    #[error("retained record {0} is missing, unreadable or unsafe")]
    Record(String),
    #[error("the selected method is not runnable here: {0}")]
    Incompatible(String),
    #[error("retained records do not bind the selection: {0}")]
    Binding(String),
}
use MethodAcceptanceError as E;

fn sha(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}

fn trial_name_ok(name: &str) -> bool {
    !name.is_empty()
        && name.len() <= 128
        && !name.starts_with('.')
        && name
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b"-_.".contains(&b))
}

struct Reader {
    root: PathBuf,
    trial: String,
    files: BTreeMap<String, String>,
}
impl Reader {
    /// Regular files only, never through a symlink below the records root
    /// (the trial directory included), bounded before reading.
    fn bytes(&mut self, rel: &str) -> Result<Option<Vec<u8>>, E> {
        let path = self.root.join(&self.trial).join(rel);
        let fail = || E::Record(rel.into());
        let mut probe = self.root.clone();
        for part in Path::new(&self.trial).join(rel).components() {
            probe.push(part);
            match std::fs::symlink_metadata(&probe) {
                Ok(m) if m.file_type().is_symlink() => return Err(fail()),
                Ok(_) => {}
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
                Err(_) => return Err(fail()),
            }
        }
        let meta = std::fs::symlink_metadata(&path).map_err(|_| fail())?;
        if !meta.is_file() || meta.len() > MAX_RECORD_FILE_BYTES {
            return Err(fail());
        }
        let bytes = std::fs::read(&path).map_err(|_| fail())?;
        if bytes.len() as u64 > MAX_RECORD_FILE_BYTES {
            return Err(fail());
        }
        self.files.insert(rel.into(), sha(&bytes));
        Ok(Some(bytes))
    }
    fn required(&mut self, rel: &str) -> Result<Vec<u8>, E> {
        self.bytes(rel)?.ok_or_else(|| E::Record(rel.into()))
    }
    fn json<T: DeserializeOwned>(&mut self, rel: &str) -> Result<T, E> {
        serde_json::from_slice(&self.required(rel)?).map_err(|_| E::Record(rel.into()))
    }
    fn slot(&mut self, state: &str) -> Result<SlotEvidence, E> {
        let execution = self.json(&format!("{state}/receipt.json"))?;
        let score = format!("{state}-score");
        let scoring: Option<EvaluationReceipt> =
            match self.bytes(&format!("{score}/evaluation.json"))? {
                Some(b) => Some(serde_json::from_slice(&b).map_err(|_| E::Record(score.clone()))?),
                None => None,
            };
        let metrics = match scoring.as_ref().map(|r| &r.outcome) {
            Some(EvaluationOutcome::Scored { metrics }) => {
                if !metrics.sha256.bytes().all(|b| b.is_ascii_hexdigit())
                    || metrics.sha256.len() != 64
                {
                    return Err(E::Record(score));
                }
                Some(self.required(&format!("{score}/artifacts/{}", metrics.sha256))?)
            }
            _ => None,
        };
        Ok(SlotEvidence {
            execution,
            scoring,
            metrics,
        })
    }
}

/// Checks the selected offer against this receiver and its own retained trial,
/// then recomputes the verdict. `Err` means the records don't belong to this
/// selection or receiver; it is never an acceptance outcome.
pub fn assess_selected(
    receiver: &MethodReceiver,
    org_id: &OrgId,
    selected: &SelectedMethod,
    trial: &str,
) -> Result<AcceptanceEvidence, MethodAcceptanceError> {
    if !trial_name_ok(trial) {
        return Err(E::TrialName);
    }
    if selected.publisher == *org_id {
        return Err(E::OwnPublication);
    }
    let mut reader = Reader {
        root: receiver.records_root.clone(),
        trial: trial.into(),
        files: BTreeMap::new(),
    };
    let plan: TrialPlan = reader.json("plan.json")?;
    let candidate = plan
        .slots
        .iter()
        .find(|s| {
            matches!(
                s.purpose,
                TrialPurpose::Acceptance {
                    role: AcceptanceRole::Candidate
                }
            )
        })
        .ok_or_else(|| E::Binding("the plan has no candidate slot".into()))?;
    if candidate.org_id != *org_id {
        return Err(E::Binding(
            "the trial belongs to another organization".into(),
        ));
    }
    if candidate.method != selected.reference {
        return Err(E::Binding(
            "the trial's candidate is not the selected method".into(),
        ));
    }
    if let Some(d) = candidate
        .dependencies
        .iter()
        .find(|d| !receiver.allowed_dependencies.contains(d))
    {
        return Err(E::Incompatible(format!(
            "dependency {}@{} is not allowed",
            d.id, d.version
        )));
    }
    MethodPackage::load_source_for(
        selected.package_json.as_bytes(),
        &selected.reference,
        &candidate.dependencies,
        &receiver.runtime,
        &receiver.protocol,
    )
    .map_err(|e| E::Incompatible(format!("package does not fit this receiver: {e}")))?;
    if !receiver
        .environments
        .contains(&plan.development_environment)
    {
        return Err(E::Incompatible(
            "the trial ran in an environment this receiver does not accept".into(),
        ));
    }
    if !receiver.evaluators.contains(&plan.context.evaluator) {
        return Err(E::Incompatible(
            "the trial was scored by an evaluator this receiver does not accept".into(),
        ));
    }
    let criterion = reader.required("criterion.json")?;
    let candidate_evidence = reader.slot("candidate")?;
    let control_evidence = reader.slot("control")?;
    let verdict =
        decide(&plan, &criterion, &candidate_evidence, &control_evidence).map_err(E::Binding)?;
    let mut acceptance = match verdict {
        Verdict::Accepted {
            candidate,
            control,
            of,
        } => Acceptance::Accepted {
            candidate,
            control,
            of,
        },
        Verdict::Rejected { reason } => Acceptance::Rejected { reason },
        Verdict::Insufficient { reason } => Acceptance::Insufficient { reason },
    };
    for (name, e) in [
        ("candidate", &candidate_evidence),
        ("control", &control_evidence),
    ] {
        if e.execution.outcome == ExecutionOutcome::Unknown {
            acceptance = Acceptance::Unknown {
                reason: format!("the {name} execution outcome is unknown"),
            };
        } else if let Some(EvaluationOutcome::Unknown { .. }) =
            e.scoring.as_ref().map(|s| &s.outcome)
        {
            acceptance = Acceptance::Unknown {
                reason: format!("the {name} scoring outcome is unknown"),
            };
        }
        if matches!(acceptance, Acceptance::Unknown { .. }) {
            break;
        }
    }
    Ok(AcceptanceEvidence {
        trial: trial.into(),
        plan: plan
            .reference()
            .map_err(|e| E::Binding(format!("plan: {e:?}")))?,
        criterion: plan.criterion.clone(),
        public_task: plan.context.public_task.clone(),
        evaluator: plan.context.evaluator.clone(),
        environment: plan.development_environment.clone(),
        dependencies: candidate.dependencies.clone(),
        files: reader.files,
        acceptance,
    })
}

#[cfg(test)]
#[path = "method_adoption/tests.rs"]
pub(crate) mod tests;
