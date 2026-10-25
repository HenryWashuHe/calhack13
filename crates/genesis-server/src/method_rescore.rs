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
//! What this establishes: the retained scoring is what this organization's
//! scorer derives from the retained outputs. It does not re-execute either
//! method, so a substituted output is still scored as given.
use crate::method_adoption::{read_retained, AcceptanceEvidence, MethodAcceptanceError};
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

/// Trusted operator configuration for re-execution: the executor
/// (`lab-exec` interface: `run <plan> <slot> <package> <state-dir> <sandbox>`),
/// its pin, the sandbox mode, and a directory of receiver-held control
/// packages, each named `<sha256>.json` after its bytes. The candidate always
/// comes from the selected registry offer.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MethodReexecutor {
    pub path: PathBuf,
    pub sha256: String,
    pub sandbox: String,
    pub packages: PathBuf,
    /// `PATH` for the executor (it finds the pinned runtime there).
    #[serde(default)]
    pub path_env: Option<String>,
    #[serde(default)]
    pub timeout_ms: Option<u64>,
}

/// The retained receipts one re-execution reproduced, by trial-relative path.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReexecutedTrial {
    pub executor_sha256: String,
    pub files: BTreeMap<String, String>,
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

/// Comparable form of an evaluation or execution receipt: everything but
/// measured wall time.
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

/// Runs a staged tool with a cleared environment (plus `path_env`, if any)
/// until it exits or `timeout_ms` passes.
fn run_tool(
    tool: &Path,
    args: &[&std::ffi::OsStr],
    path_env: Option<&str>,
    timeout_ms: u64,
    (what, context): (&str, &str),
    cwd: &Path,
) -> Result<(), MethodAcceptanceError> {
    let deadline = Duration::from_millis(timeout_ms);
    let mut attempts = 0;
    let mut child = loop {
        let mut command = Command::new(tool);
        command
            .args(args)
            .env_clear()
            .current_dir(cwd)
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
    let started = Instant::now();
    loop {
        match child.try_wait() {
            Ok(Some(status)) if status.success() => return Ok(()),
            Ok(Some(_)) => return Err(fail(format!("the {what} failed{context}"))),
            Ok(None) if started.elapsed() < deadline => {
                std::thread::sleep(Duration::from_millis(20))
            }
            Ok(None) => {
                let _ = child.kill();
                let _ = child.wait();
                return Err(fail(format!("the {what} timed out{context}")));
            }
            Err(_) => {
                let _ = child.kill();
                let _ = child.wait();
                return Err(fail(format!("the {what} could not be awaited")));
            }
        }
    }
}

impl MethodRescorer {
    fn install(&self, work: &Path) -> Result<PathBuf, MethodAcceptanceError> {
        install(&self.path, &self.sha256, work, "rescorer")
    }

    fn run(
        &self,
        scorer: &Path,
        args: &[&Path],
        slot: &str,
        cwd: &Path,
    ) -> Result<(), MethodAcceptanceError> {
        let args = [
            "score".as_ref(),
            args[0].as_os_str(),
            slot.as_ref(),
            args[1].as_os_str(),
            args[2].as_os_str(),
        ];
        let timeout = self.timeout_ms.unwrap_or(DEFAULT_TIMEOUT_MS);
        run_tool(
            scorer,
            &args,
            None,
            timeout,
            ("rescorer", &format!(" on the {slot} slot")),
            cwd,
        )
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
            reexecutor: None,
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

impl MethodReexecutor {
    fn package(
        &self,
        slot: &str,
        method_sha256: &str,
        candidate: &[u8],
    ) -> Result<Vec<u8>, MethodAcceptanceError> {
        let bytes = if slot == "candidate" {
            candidate.to_vec()
        } else {
            if method_sha256.len() != 64 || !method_sha256.bytes().all(|b| b.is_ascii_hexdigit()) {
                return Err(fail(format!("the {slot} method has no usable digest")));
            }
            bounded(
                &self.packages.join(format!("{method_sha256}.json")),
                MAX_OUTPUT_BYTES,
            )
            .map_err(|_| {
                fail(format!(
                    "this organization holds no package for the {slot} method"
                ))
            })?
        };
        if sha(&bytes) != method_sha256 {
            return Err(fail(format!("the {slot} package is not the plan's method")));
        }
        Ok(bytes)
    }

    /// Re-runs every slot that has a retained execution receipt and requires
    /// the fresh receipt to equal it (wall time aside). Blocking: call it off
    /// the async runtime and outside any store lock.
    pub fn reexecute_trial(
        &self,
        records_root: &Path,
        trial: &str,
        candidate_package: &[u8],
    ) -> Result<ReexecutedTrial, MethodAcceptanceError> {
        let work = Scratch::new()?;
        let executor = install(&self.path, &self.sha256, work.path(), "executor")?;
        let read = |rel: &str| read_retained(records_root, trial, rel);
        let plan_bytes =
            read("plan.json")?.ok_or_else(|| MethodAcceptanceError::Record("plan.json".into()))?;
        let plan: TrialPlan = serde_json::from_slice(&plan_bytes)
            .map_err(|_| MethodAcceptanceError::Record("plan.json".into()))?;
        let mut files = BTreeMap::new();
        files.insert("plan.json".to_string(), sha(&plan_bytes));
        let plan_path = work.path().join("plan.json");
        std::fs::write(&plan_path, &plan_bytes).map_err(|_| fail("could not stage the plan"))?;
        for slot in SLOTS {
            let receipt_rel = format!("{slot}/receipt.json");
            let Some(retained) = read(&receipt_rel)? else {
                continue;
            };
            let method = plan
                .slots
                .iter()
                .find(|s| s.slot_id == slot)
                .map(|s| s.method.sha256.clone())
                .ok_or_else(|| fail(format!("the plan has no {slot} slot")))?;
            let package = work.path().join(format!("{slot}.package.json"));
            std::fs::write(&package, self.package(slot, &method, candidate_package)?)
                .map_err(|_| fail("could not stage the package"))?;
            let state = work.path().join(slot);
            let args = [
                "run".as_ref(),
                plan_path.as_os_str(),
                slot.as_ref(),
                package.as_os_str(),
                state.as_os_str(),
                self.sandbox.as_ref(),
            ];
            let timeout = self.timeout_ms.unwrap_or(DEFAULT_TIMEOUT_MS);
            run_tool(
                &executor,
                &args,
                self.path_env.as_deref(),
                timeout,
                ("executor", &format!(" on the {slot} slot")),
                work.path(),
            )?;
            let fresh = bounded(&state.join("receipt.json"), MAX_OUTPUT_BYTES)?;
            match (comparable(&fresh), comparable(&retained)) {
                (Some(a), Some(b)) if a == b => {}
                _ => {
                    return Err(fail(format!(
                        "the retained {slot} execution does not reproduce"
                    )))
                }
            }
            files.insert(receipt_rel, sha(&retained));
        }
        Ok(ReexecutedTrial {
            executor_sha256: self.sha256.clone(),
            files,
        })
    }
}

impl ReexecutedTrial {
    /// The assessment must have read exactly the receipts that were
    /// reproduced, and no receipt the re-execution didn't see.
    pub fn binds(&self, evidence: &AcceptanceEvidence) -> Result<(), MethodAcceptanceError> {
        let unseen = evidence
            .files
            .keys()
            .any(|k| k.ends_with("/receipt.json") && !self.files.contains_key(k));
        let changed = self
            .files
            .iter()
            .any(|(k, v)| evidence.files.get(k) != Some(v));
        if unseen || changed {
            return Err(fail("the retained records changed after re-execution"));
        }
        Ok(())
    }
}
