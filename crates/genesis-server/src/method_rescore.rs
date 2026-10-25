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
    let name = path.file_name().map(|n| n.to_string_lossy().into_owned());
    let unreadable = || {
        fail(format!(
            "{} is unreadable",
            name.as_deref().unwrap_or("a file")
        ))
    };
    let file = std::fs::File::open(path).map_err(|_| unreadable())?;
    let mut bytes = Vec::new();
    file.take(limit + 1)
        .read_to_end(&mut bytes)
        .map_err(|_| unreadable())?;
    if bytes.len() as u64 > limit {
        return Err(fail(format!(
            "{} is too large",
            name.as_deref().unwrap_or("a file")
        )));
    }
    Ok(bytes)
}

/// Comparable form of an evaluation or execution receipt: everything but
/// measured wall time, which is checked against its grant separately.
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

/// Copies a pinned tool into `work`, checking the copy's digest, so a later
/// change to the configured path can't run unpinned code.
fn install(
    path: &Path,
    pin: &str,
    work: &Path,
    name: &str,
) -> Result<PathBuf, MethodAcceptanceError> {
    let bytes = bounded(path, MAX_SCORER_BYTES)?;
    if sha(&bytes) != pin {
        return Err(fail(format!(
            "the configured {name} does not match its pin"
        )));
    }
    let copy = work.join(name);
    {
        use std::io::Write;
        use std::os::unix::fs::OpenOptionsExt;
        let mut file = std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o700)
            .open(&copy)
            .map_err(|_| fail(format!("could not stage the {name}")))?;
        file.write_all(&bytes)
            .and_then(|_| file.sync_all())
            .map_err(|_| fail(format!("could not stage the {name}")))?;
    }
    Ok(copy)
}

/// Runs a staged tool with a cleared environment (plus `path_env`, if any) in
/// its own process group, until it exits or `timeout_ms` passes; on timeout the
/// whole group is killed. Returns whether it exited successfully. The staged
/// copy is re-hashed just before it starts.
fn run_tool(
    (tool, pin): (&Path, &str),
    args: &[&std::ffi::OsStr],
    path_env: Option<&str>,
    timeout_ms: u64,
    (what, context): (&str, &str),
    cwd: &Path,
) -> Result<bool, MethodAcceptanceError> {
    use std::os::unix::process::CommandExt;
    if bounded(tool, MAX_SCORER_BYTES)
        .map(|b| sha(&b))
        .ok()
        .as_deref()
        != Some(pin)
    {
        return Err(fail(format!("the staged {what} changed before it ran")));
    }
    let deadline = Duration::from_millis(timeout_ms);
    let mut attempts = 0;
    let mut child = loop {
        let mut command = Command::new(tool);
        command
            .args(args)
            .env_clear()
            .current_dir(cwd)
            .process_group(0)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null());
        if let Some(path) = path_env {
            command.env("PATH", path);
        }
        match command.spawn() {
            Ok(child) => break child,
            Err(e) if e.raw_os_error() == Some(TEXT_BUSY) && attempts < 5 => {
                attempts += 1;
                std::thread::sleep(Duration::from_millis(20));
            }
            Err(_) => return Err(fail(format!("the {what} could not start"))),
        }
    };
    let kill_group = |child: &mut std::process::Child| {
        let _ = Command::new("kill")
            .arg("-KILL")
            .arg(format!("-{}", child.id()))
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status();
        let _ = child.kill();
        let _ = child.wait();
    };
    let started = Instant::now();
    loop {
        match child.try_wait() {
            Ok(Some(status)) => return Ok(status.success()),
            Ok(None) if started.elapsed() < deadline => {
                std::thread::sleep(Duration::from_millis(20))
            }
            Ok(None) => {
                kill_group(&mut child);
                return Err(fail(format!("the {what} timed out{context}")));
            }
            Err(_) => {
                kill_group(&mut child);
                return Err(fail(format!("the {what} could not be awaited")));
            }
        }
    }
}

/// Whether a receipt's measured wall time is within `grant` milliseconds.
fn fits(bytes: &[u8], grant: u64) -> bool {
    wall_ms(bytes).is_some_and(|ms| ms <= grant)
}

/// Measured wall time of a receipt, if it was measured.
fn wall_ms(bytes: &[u8]) -> Option<u64> {
    let v: Value = serde_json::from_slice(bytes).ok()?;
    match v["usage"]["wall_time_ms"]["kind"].as_str()? {
        "measured" => v["usage"]["wall_time_ms"]["value"].as_u64(),
        _ => None,
    }
}

impl MethodRescorer {
    fn install(&self, work: &Path) -> Result<PathBuf, MethodAcceptanceError> {
        install(&self.path, &self.sha256, work, "rescorer")
    }

    /// Scores one staged slot into `out` and returns the fresh evaluation. A
    /// scorer that wrote an evaluation and then exited non-zero (for example on
    /// a scoring overrun) still counts: the evaluation is what gets compared.
    fn score(
        &self,
        scorer: &Path,
        (plan, state, out): (&Path, &Path, &Path),
        slot: &str,
        cwd: &Path,
    ) -> Result<Vec<u8>, MethodAcceptanceError> {
        let args = [
            "score".as_ref(),
            plan.as_os_str(),
            slot.as_ref(),
            state.as_os_str(),
            out.as_os_str(),
        ];
        let timeout = self.timeout_ms.unwrap_or(DEFAULT_TIMEOUT_MS);
        let context = format!(" on the {slot} slot");
        let ok = run_tool(
            (scorer, &self.sha256),
            &args,
            None,
            timeout,
            ("rescorer", &context),
            cwd,
        )?;
        let evaluation = out.join("evaluation.json");
        if !ok && !evaluation.exists() {
            return Err(fail(format!("the rescorer failed{context}")));
        }
        bounded(&evaluation, MAX_OUTPUT_BYTES)
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
        let scoring_grant = serde_json::from_slice::<TrialPlan>(&plan)
            .map_err(|_| MethodAcceptanceError::Record("plan.json".into()))?
            .scoring_grant
            .wall_time_ms;
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
            let fresh = self.score(&scorer, (&plan_path, &state, &out), slot, work.path())?;
            match (comparable(&fresh), comparable(&retained)) {
                (Some(a), Some(b)) if a == b => {}
                _ => {
                    return Err(fail(format!(
                        "the retained {slot} scoring does not reproduce"
                    )))
                }
            }
            // Wall time is the one measurement left out of the comparison, so
            // it is re-derived here: a retained claim that scoring fit its
            // grant only counts if it fits on this host too.
            if fits(&retained, scoring_grant) && !fits(&fresh, scoring_grant) {
                return Err(fail(format!(
                    "the retained {slot} scoring fits its grant but rescoring here did not"
                )));
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
        let fresh = self.score(&scorer, (&plan_path, &state, &out), slot, work.path())?;
        match (comparable(&fresh), comparable(&json(posted)?)) {
            (Some(a), Some(b)) if a == b => {}
            _ => return Err(fail("the posted evaluation does not reproduce")),
        }
        // Wall time is left out of the comparison, but the posted value decides
        // whether the result fits its scoring grant. A claim that it fits must
        // also hold here.
        let grant = plan.scoring_grant.wall_time_ms;
        let here = wall_ms(&fresh).ok_or_else(|| fail("rescoring measured no wall time"))?;
        let claims_fit = wall_ms(&json(posted)?).is_some_and(|ms| ms <= grant);
        if claims_fit && here > grant {
            return Err(fail(
                "the posted evaluation fits its scoring grant but rescoring here did not",
            ));
        }
        Ok(())
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
