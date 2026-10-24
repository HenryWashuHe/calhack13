//! Runs one frozen trial slot as a process: identity checks, spawn, request
//! loop, wall-time kill, receipt. Ledger transitions live in `session`.
use crate::refs;
use crate::sandbox::Mode;
use crate::session::{finish, Finished, RunError, RunState, Session};
use crate::store::Store;
use genesis_core::development_experiment::{
    ExperimentLedger, ExperimentOutcome, ExperimentScope, ExperimentSettlement,
};
use genesis_core::development_method::{ExactContentRef, ExecutionOutcome, ResourceUsage};
use genesis_core::development_trial::TrialPlan;
use std::io::{BufRead, BufReader, Read, Write};
use std::path::{Path, PathBuf};
use std::sync::mpsc;
use std::time::{Duration, Instant};

/// Longest request line a method may send.
pub const MAX_LINE: usize = 1 << 20;
/// The method is killed this long before its wall-time grant ends, so the
/// measured time stays inside the grant in the ordinary case.
pub const KILL_MARGIN_MS: u64 = 250;

pub struct Method {
    pub manifest: refs::MethodManifest,
    pub manifest_bytes: Vec<u8>,
    pub entry_bytes: Vec<u8>,
}

impl Method {
    pub fn load(manifest_path: &Path) -> Result<Self, String> {
        let manifest_bytes =
            std::fs::read(manifest_path).map_err(|e| format!("read manifest: {e}"))?;
        let manifest: refs::MethodManifest =
            serde_json::from_slice(&manifest_bytes).map_err(|e| format!("method manifest: {e}"))?;
        let entry = manifest_path
            .parent()
            .unwrap_or(Path::new("."))
            .join(&manifest.entry);
        let entry_bytes = std::fs::read(&entry).map_err(|e| format!("read entry: {e}"))?;
        Ok(Self {
            manifest,
            manifest_bytes,
            entry_bytes,
        })
    }
    pub fn reference(&self) -> Result<ExactContentRef, String> {
        refs::method_ref(&self.manifest_bytes, &self.entry_bytes)
    }
}

/// `<interpreter> --version`, pinned in the environment reference.
pub fn interpreter_version(interpreter: &str) -> String {
    std::process::Command::new(interpreter)
        .arg("--version")
        .output()
        .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_owned())
        .unwrap_or_else(|_| format!("{interpreter} unavailable"))
}

pub struct Config {
    pub sandbox: Mode,
    pub interpreter_version: String,
    /// Where entry scripts are staged; must not be under a hidden directory.
    pub stage_root: PathBuf,
}

impl Config {
    pub fn environment(&self) -> ExactContentRef {
        refs::environment(self.sandbox, &self.interpreter_version)
    }
}

/// Recovers a slot whose executor stopped mid-run: a pending call is settled
/// unknown (never resubmitted), the ledger is sealed and the receipt says
/// `unknown`. The method is not started again.
pub fn recover(
    plan: &TrialPlan,
    slot_id: &str,
    dir: &Path,
    cfg: &Config,
) -> Result<Finished, RunError> {
    let store = Store::open(dir)?;
    if store.exists("receipt.json") {
        return Err(RunError::AlreadyStarted);
    }
    let ledger: ExperimentLedger = match store.read("ledger.json") {
        Ok(bytes) => {
            serde_json::from_slice(&bytes).map_err(|e| RunError::Refused(format!("ledger: {e}")))?
        }
        Err(_) => return Err(RunError::Refused("no execution to recover".into())),
    };
    let state: RunState = store
        .read("run.json")
        .ok()
        .and_then(|b| serde_json::from_slice(&b).ok())
        .unwrap_or_default();
    let mut slot = Session {
        plan,
        slot_id,
        store,
        ledger,
        state,
    };
    if let Some(last) = slot
        .ledger
        .records()
        .last()
        .filter(|r| r.settlement.is_none())
    {
        let call = last.call.clone();
        let reason = "the executor stopped before this experiment's result was recorded";
        slot.ledger.settle(
            slot.scope(),
            &call,
            ExperimentSettlement {
                outcome: ExperimentOutcome::Unknown {
                    reason: reason.into(),
                },
                simulation_steps: ResourceUsage::Unknown {
                    reason: reason.into(),
                },
            },
        )?;
    }
    finish(
        &mut slot,
        cfg.environment(),
        cfg.sandbox.bounds_model_use(),
        ExecutionOutcome::Unknown,
        ResourceUsage::Unknown {
            reason: "the executor stopped before the method finished".into(),
        },
    )
}

/// Runs the slot once. Identity checks happen before anything is spawned.
pub fn execute(
    plan: &TrialPlan,
    slot_id: &str,
    method: &Method,
    dir: &Path,
    cfg: &Config,
) -> Result<Finished, RunError> {
    let binding = plan.expected_binding(slot_id)?;
    let actual = method.reference().map_err(RunError::Refused)?;
    if actual != binding.method {
        return Err(RunError::Refused(
            "method digest differs from the plan's slot".into(),
        ));
    }
    if cfg.environment() != binding.environment {
        return Err(RunError::Refused(
            "executor environment differs from the plan".into(),
        ));
    }
    if refs::evaluator() != binding.evaluator {
        return Err(RunError::Refused(
            "linked evaluator differs from the plan".into(),
        ));
    }
    if !binding.dependencies.is_empty() {
        return Err(RunError::Refused(
            "executor v0 provides no dependencies".into(),
        ));
    }
    let store = Store::open(dir)?;
    if store.exists("ledger.json") || store.exists("run.json") {
        return Err(RunError::AlreadyStarted);
    }
    let mut slot = Session {
        plan,
        slot_id,
        store,
        ledger: ExperimentLedger::new(ExperimentScope { plan, slot_id })?,
        state: RunState {
            started: true,
            method: Some(actual),
            ..RunState::default()
        },
    };
    slot.save()?;

    let stage = cfg.stage_root.join(format!(
        "{}-{}",
        std::process::id(),
        &refs::sha256_hex(slot_id.as_bytes())[..12]
    ));
    std::fs::create_dir_all(&stage)?;
    std::fs::write(stage.join("entry"), &method.entry_bytes)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        // Only root (before the privilege drop) or the same user reads it.
        std::fs::set_permissions(&stage, std::fs::Permissions::from_mode(0o700))?;
        std::fs::set_permissions(stage.join("entry"), std::fs::Permissions::from_mode(0o600))?;
    }
    let work = stage.join("work");
    std::fs::create_dir_all(&work)?;

    let grant = binding.budget.wall_time_ms;
    let deadline = Duration::from_millis(grant.saturating_sub(KILL_MARGIN_MS));
    let started = Instant::now();
    let mut child = match cfg
        .sandbox
        .command(&method.manifest.interpreter, &stage, &work)
        .spawn()
    {
        Ok(c) => c,
        Err(e) => {
            let _ = std::fs::remove_dir_all(&stage);
            return finish(
                &mut slot,
                cfg.environment(),
                cfg.sandbox.bounds_model_use(),
                ExecutionOutcome::Failed,
                ResourceUsage::Unknown {
                    reason: format!("the method could not be started: {e}"),
                },
            );
        }
    };
    let mut stdin = child.stdin.take().expect("piped stdin");
    let stdout = child.stdout.take().expect("piped stdout");
    let (tx, rx) = mpsc::channel::<Option<String>>();
    std::thread::spawn(move || {
        let mut reader = BufReader::new(stdout);
        loop {
            let mut line = Vec::new();
            match (&mut reader)
                .take(MAX_LINE as u64 + 1)
                .read_until(b'\n', &mut line)
            {
                Ok(0) | Err(_) => break,
                Ok(_) if line.len() > MAX_LINE => {
                    let _ = tx.send(None);
                    break;
                }
                Ok(_) => {
                    if tx
                        .send(Some(String::from_utf8_lossy(&line).into_owned()))
                        .is_err()
                    {
                        break;
                    }
                }
            }
        }
    });

    let mut outcome = None;
    loop {
        let left = deadline.saturating_sub(started.elapsed());
        match rx.recv_timeout(left) {
            Ok(Some(line)) => {
                let reply = slot.handle(line.trim_end())?;
                let mut out = serde_json::to_vec(&reply).expect("reply serializes");
                out.push(b'\n');
                if stdin.write_all(&out).and_then(|_| stdin.flush()).is_err() {
                    // The method closed its input; wait for it to exit.
                }
            }
            Ok(None) => {
                outcome = Some(ExecutionOutcome::Failed);
                let _ = child.kill();
                break;
            }
            Err(mpsc::RecvTimeoutError::Timeout) => {
                outcome = Some(ExecutionOutcome::Interrupted);
                let _ = child.kill();
                break;
            }
            Err(mpsc::RecvTimeoutError::Disconnected) => break,
        }
    }
    drop(stdin);
    let status = loop {
        if let Some(status) = child.try_wait()? {
            break status;
        }
        if started.elapsed() >= deadline {
            outcome = Some(ExecutionOutcome::Interrupted);
            let _ = child.kill();
            break child.wait()?;
        }
        std::thread::sleep(Duration::from_millis(5));
    };
    let wall = started.elapsed().as_millis() as u64;
    let _ = std::fs::remove_dir_all(&stage);
    let outcome = outcome.unwrap_or(if status.success() && slot.state.submission.is_some() {
        ExecutionOutcome::OutputGenerated
    } else {
        ExecutionOutcome::Failed
    });
    finish(
        &mut slot,
        cfg.environment(),
        cfg.sandbox.bounds_model_use(),
        outcome,
        ResourceUsage::Measured { value: wall },
    )
}
