//! Optional receiver-side rescoring of a retained acceptance trial.
//!
//! Without it, an adoption decision is computed from evaluation receipts the
//! operator's scorer wrote, so whoever controls the records root controls the
//! verdict. With it, this organization re-runs its own pinned scorer on a
//! private snapshot of each slot's retained plan, execution receipt and output
//! before the decision, and refuses unless every scored slot reproduces the
//! retained evaluation exactly (wall time aside). The decision then checks that
//! assessment read the same bytes the rescore was given.
//!
//! The same scorer also checks an invocation's posted evaluation against the
//! server's own frozen plan and retained receipt, with the output bytes the
//! operator supplies (checked against the receipt's output digest).
//!
//! What this establishes: the scoring is what this organization's scorer
//! derives from the outputs. It does not re-execute any method, so a
//! substituted output is still scored as given.
use crate::method_adoption::{read_retained, AcceptanceEvidence, MethodAcceptanceError};
use genesis_core::development_evaluation::EvaluationReceipt;
use genesis_core::development_method::ExecutionReceipt;
use genesis_core::development_trial::TrialPlan;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

const SLOTS: [&str; 2] = ["candidate", "control"];
const MAX_SCORER_BYTES: u64 = 256 * 1024 * 1024;
const MAX_OUTPUT_BYTES: u64 = 16 * 1024 * 1024;
const DEFAULT_TIMEOUT_MS: u64 = 600_000;
/// Linux `ETXTBSY`: a forked child elsewhere still held the copy open.
const TEXT_BUSY: i32 = 26;

/// Trusted operator configuration: the scorer executable (`lab-score`
/// interface: `score <plan> <slot> <state-dir> <score-dir>`) and its pin.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MethodRescorer {
    pub path: PathBuf,
    pub sha256: String,
    #[serde(default)]
    pub timeout_ms: Option<u64>,
}

/// The retained bytes one rescore was given, by trial-relative path.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RescoredTrial {
    pub rescorer_sha256: String,
    pub files: BTreeMap<String, String>,
}

fn sha(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}

fn fail(message: impl Into<String>) -> MethodAcceptanceError {
    MethodAcceptanceError::Rescore(message.into())
}

fn bounded(path: &Path, limit: u64) -> Result<Vec<u8>, MethodAcceptanceError> {
    use std::io::Read;
    let file = std::fs::File::open(path).map_err(|_| fail("the rescorer is unreadable"))?;
    let mut bytes = Vec::new();
    file.take(limit + 1)
        .read_to_end(&mut bytes)
        .map_err(|_| fail("the rescorer is unreadable"))?;
    if bytes.len() as u64 > limit {
        return Err(fail("the rescorer is too large"));
    }
    Ok(bytes)
}

/// Comparable form of an evaluation receipt: everything but measured wall time.
fn comparable(bytes: &[u8]) -> Option<Value> {
    let mut v: Value = serde_json::from_slice(bytes).ok()?;
    v.get_mut("usage")?
        .as_object_mut()?
        .remove("wall_time_ms")?;
    Some(v)
}

/// A private scratch directory removed when dropped.
struct Scratch(PathBuf);
impl Scratch {
    fn new() -> Result<Self, MethodAcceptanceError> {
        use std::os::unix::fs::DirBuilderExt;
        let path =
            std::env::temp_dir().join(format!("genesis-rescore-{}", uuid::Uuid::new_v4().simple()));
        std::fs::DirBuilder::new()
            .mode(0o700)
            .create(&path)
            .map_err(|_| fail("no scratch space for rescoring"))?;
        Ok(Self(path))
    }
    fn path(&self) -> &Path {
        &self.0
    }
}
impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

impl MethodRescorer {
    /// Copies the pinned scorer into `work`, checking the copy's digest, so a
    /// later change to the configured path can't run unpinned code.
    fn install(&self, work: &Path) -> Result<PathBuf, MethodAcceptanceError> {
        let bytes = bounded(&self.path, MAX_SCORER_BYTES)?;
        if sha(&bytes) != self.sha256 {
            return Err(fail("the configured rescorer does not match its pin"));
        }
        let copy = work.join("rescorer");
        {
            use std::io::Write;
            use std::os::unix::fs::OpenOptionsExt;
            let mut file = std::fs::OpenOptions::new()
                .write(true)
                .create_new(true)
                .mode(0o700)
                .open(&copy)
                .map_err(|_| fail("could not stage the rescorer"))?;
            file.write_all(&bytes)
                .and_then(|_| file.sync_all())
                .map_err(|_| fail("could not stage the rescorer"))?;
        }
        Ok(copy)
    }

    fn run(
        &self,
        scorer: &Path,
        args: &[&Path],
        slot: &str,
        cwd: &Path,
    ) -> Result<(), MethodAcceptanceError> {
        let deadline = Duration::from_millis(self.timeout_ms.unwrap_or(DEFAULT_TIMEOUT_MS));
        let mut attempts = 0;
        let mut child = loop {
            let spawned = Command::new(scorer)
                .arg("score")
                .arg(args[0])
                .arg(slot)
                .arg(args[1])
                .arg(args[2])
                .env_clear()
                .current_dir(cwd)
                .stdin(Stdio::null())
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .spawn();
            match spawned {
                Ok(child) => break child,
                Err(e) if e.raw_os_error() == Some(TEXT_BUSY) && attempts < 5 => {
                    attempts += 1;
                    std::thread::sleep(Duration::from_millis(20));
                }
                Err(_) => return Err(fail("the rescorer could not start")),
            }
        };
        let started = Instant::now();
        loop {
            match child.try_wait() {
                Ok(Some(status)) if status.success() => return Ok(()),
                Ok(Some(_)) => return Err(fail(format!("the rescorer failed on the {slot} slot"))),
                Ok(None) if started.elapsed() < deadline => {
                    std::thread::sleep(Duration::from_millis(20))
                }
                Ok(None) => {
                    let _ = child.kill();
                    let _ = child.wait();
                    return Err(fail(format!("the rescorer timed out on the {slot} slot")));
                }
                Err(_) => {
                    let _ = child.kill();
                    let _ = child.wait();
                    return Err(fail("the rescorer could not be awaited"));
                }
            }
        }
    }

    /// Rescores every slot that has a retained evaluation. Blocking: call it
    /// off the async runtime and outside any store lock.
    pub fn rescore_trial(
        &self,
        records_root: &Path,
        trial: &str,
    ) -> Result<RescoredTrial, MethodAcceptanceError> {
        let work = Scratch::new()?;
        let scorer = self.install(work.path())?;
        let read = |rel: &str| read_retained(records_root, trial, rel);
        let need = |rel: &str| read(rel)?.ok_or_else(|| MethodAcceptanceError::Record(rel.into()));
        let mut files = BTreeMap::new();
        let plan = need("plan.json")?;
        files.insert("plan.json".to_string(), sha(&plan));
        let plan_path = work.path().join("plan.json");
        std::fs::write(&plan_path, &plan).map_err(|_| fail("could not stage the plan"))?;
        for slot in SLOTS {
            let eval_rel = format!("{slot}-score/evaluation.json");
            let Some(retained) = read(&eval_rel)? else {
                continue;
            };
            let receipt_rel = format!("{slot}/receipt.json");
            let receipt_bytes = need(&receipt_rel)?;
            let receipt: ExecutionReceipt = serde_json::from_slice(&receipt_bytes)
                .map_err(|_| MethodAcceptanceError::Record(receipt_rel.clone()))?;
            let state = work.path().join(slot);
            std::fs::create_dir_all(state.join("artifacts"))
                .map_err(|_| fail("could not stage the slot"))?;
            std::fs::write(state.join("receipt.json"), &receipt_bytes)
                .map_err(|_| fail("could not stage the slot"))?;
            if let Some(output) = receipt.outputs.first() {
                let digest = &output.sha256;
                if digest.len() != 64
                    || !digest
                        .bytes()
                        .all(|b| matches!(b, b'0'..=b'9' | b'a'..=b'f'))
                {
                    return Err(MethodAcceptanceError::Record(receipt_rel));
                }
                let rel = format!("{slot}/artifacts/{digest}");
                let bytes = need(&rel)?;
                if bytes.len() as u64 > MAX_OUTPUT_BYTES || sha(&bytes) != *digest {
                    return Err(MethodAcceptanceError::Record(rel));
                }
                std::fs::write(state.join("artifacts").join(digest), &bytes)
                    .map_err(|_| fail("could not stage the slot"))?;
            }
            let out = work.path().join(format!("{slot}-score"));
            self.run(&scorer, &[&plan_path, &state, &out], slot, work.path())?;
            let fresh = bounded(&out.join("evaluation.json"), MAX_OUTPUT_BYTES)?;
            match (comparable(&fresh), comparable(&retained)) {
                (Some(a), Some(b)) if a == b => {}
                _ => {
                    return Err(fail(format!(
                        "the retained {slot} scoring does not reproduce"
                    )))
                }
            }
            let metrics = serde_json::from_slice::<Value>(&fresh)
                .ok()
                .and_then(|v| v["outcome"]["metrics"]["sha256"].as_str().map(String::from));
            if let Some(digest) = metrics {
                files.insert(format!("{slot}-score/artifacts/{digest}"), digest);
            }
            files.insert(receipt_rel, sha(&receipt_bytes));
            files.insert(eval_rel, sha(&retained));
        }
        Ok(RescoredTrial {
            rescorer_sha256: self.sha256.clone(),
            files,
        })
    }
}

impl MethodRescorer {
    /// Rescores one completed invocation run. `output` must be the bytes the
    /// receipt names; the posted evaluation must reproduce exactly (wall time
    /// aside). Blocking, like [`Self::rescore_trial`].
    pub fn rescore_run(
        &self,
        plan: &TrialPlan,
        slot: &str,
        receipt: &ExecutionReceipt,
        output: &[u8],
        posted: &EvaluationReceipt,
    ) -> Result<(), MethodAcceptanceError> {
        let digest = receipt
            .outputs
            .first()
            .map(|o| o.sha256.clone())
            .ok_or_else(|| fail("the run produced no output to rescore"))?;
        if output.len() as u64 > MAX_OUTPUT_BYTES || sha(output) != digest {
            return Err(fail("the supplied output is not the run's output"));
        }
        let work = Scratch::new()?;
        let scorer = self.install(work.path())?;
        let stage = |path: &Path, bytes: &[u8]| {
            std::fs::write(path, bytes).map_err(|_| fail("could not stage the run"))
        };
        let plan_path = work.path().join("plan.json");
        stage(&plan_path, &json(plan)?)?;
        let state = work.path().join("state");
        std::fs::create_dir_all(state.join("artifacts"))
            .map_err(|_| fail("could not stage the run"))?;
        stage(&state.join("receipt.json"), &json(receipt)?)?;
        stage(&state.join("artifacts").join(&digest), output)?;
        let out = work.path().join("score");
        self.run(&scorer, &[&plan_path, &state, &out], slot, work.path())?;
        let fresh = bounded(&out.join("evaluation.json"), MAX_OUTPUT_BYTES)?;
        match (comparable(&fresh), comparable(&json(posted)?)) {
            (Some(a), Some(b)) if a == b => Ok(()),
            _ => Err(fail("the posted evaluation does not reproduce")),
        }
    }
}

fn json<T: Serialize>(value: &T) -> Result<Vec<u8>, MethodAcceptanceError> {
    serde_json::to_vec(value).map_err(|_| fail("could not stage the run"))
}

impl RescoredTrial {
    /// The assessment must have read exactly the bytes that were rescored, and
    /// no scoring the rescore didn't see.
    pub fn binds(&self, evidence: &AcceptanceEvidence) -> Result<(), MethodAcceptanceError> {
        let unseen = evidence
            .files
            .keys()
            .any(|k| k.contains("-score/") && !self.files.contains_key(k));
        let changed = self
            .files
            .iter()
            .any(|(k, v)| evidence.files.get(k) != Some(v));
        if unseen || changed {
            return Err(fail("the retained records changed after rescoring"));
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::method_adoption::Acceptance;
    use genesis_core::development_method::ExactContentRef;
    use genesis_core::ids::OrgId;

    fn r(id: &str) -> ExactContentRef {
        ExactContentRef {
            domain: "d".into(),
            schema: "s".into(),
            id: id.into(),
            version: "1".into(),
            sha256: "0".repeat(64),
        }
    }

    fn evidence(files: &[(&str, &str)]) -> AcceptanceEvidence {
        AcceptanceEvidence {
            trial: "t".into(),
            publisher: OrgId::new("org_a"),
            method: r("m"),
            plan: r("p"),
            criterion: r("c"),
            public_task: r("task"),
            evaluator: r("e"),
            environment: r("env"),
            dependencies: vec![],
            files: files
                .iter()
                .map(|(k, v)| (k.to_string(), v.to_string()))
                .collect(),
            acceptance: Acceptance::Rejected { reason: "r".into() },
            rescorer: None,
        }
    }

    #[test]
    fn binding_requires_the_rescored_bytes_and_nothing_unseen() {
        let rescored = RescoredTrial {
            rescorer_sha256: "f".repeat(64),
            files: [("plan.json", "1"), ("control-score/evaluation.json", "2")]
                .iter()
                .map(|(k, v)| (k.to_string(), v.to_string()))
                .collect(),
        };
        let same = [("plan.json", "1"), ("control-score/evaluation.json", "2")];
        assert!(rescored.binds(&evidence(&same)).is_ok());
        // A file changed between the rescore and the assessment.
        let changed = [("plan.json", "1"), ("control-score/evaluation.json", "3")];
        assert!(rescored.binds(&evidence(&changed)).is_err());
        // Scoring appeared that the rescore never saw.
        let unseen = [
            ("plan.json", "1"),
            ("control-score/evaluation.json", "2"),
            ("candidate-score/evaluation.json", "4"),
        ];
        assert!(rescored.binds(&evidence(&unseen)).is_err());
    }
}
