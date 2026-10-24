//! Runs one frozen trial slot as a process: identity checks, spawn, request
//! loop, wall-time kill, receipt. Ledger transitions live in `session`.
use crate::refs;
use crate::sandbox::Mode;
use crate::session::{finish, Finished, RunError, RunState, Session};
use crate::store::{SlotClaim, Store};
use genesis_core::development_evaluation::EvaluationError;
use genesis_core::development_experiment::{
    ExperimentLedger, ExperimentOutcome, ExperimentScope, ExperimentSettlement,
};
use genesis_core::development_method::{ExactContentRef, ExecutionOutcome, ResourceUsage};
use genesis_core::development_package::MethodPackage;
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
/// Replies waiting for a method that isn't reading them; past this the run
/// fails instead of blocking.
pub const MAX_QUEUED_REPLIES: usize = 64;

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
    /// Where package files are staged; must not be under a hidden directory.
    pub stage_root: PathBuf,
}

impl Config {
    pub fn environment(&self) -> ExactContentRef {
        refs::environment(self.sandbox, &self.interpreter_version)
    }
    /// The runtime this host provides; a package must name exactly this.
    pub fn runtime(&self) -> ExactContentRef {
        refs::runtime(&self.interpreter_version)
    }
}

/// The interpreter behind [`refs::runtime`].
pub const INTERPRETER: &str = "python3";

/// Recovers a slot whose executor stopped mid-run: a pending call is settled
/// unknown (never resubmitted), the ledger is sealed and the receipt says
/// `unknown`. The method is not started again.
pub fn recover(
    plan: &TrialPlan,
    slot_id: &str,
    dir: &Path,
    cfg: &Config,
) -> Result<Finished, RunError> {
    let _claim = claim(dir)?;
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
        let settled = slot.ledger.settle(
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
        );
        match settled {
            Ok(()) => {}
            // Stopped after sealing but before the receipt: the sealed
            // transcript keeps the call pending, which is already unknown.
            Err(EvaluationError::Invalid("sealed experiment ledger")) => {}
            Err(e) => return Err(e.into()),
        }
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

/// Exclusive ownership of the slot; another runner holding it means the slot
/// is already started.
fn claim(dir: &Path) -> Result<SlotClaim, RunError> {
    SlotClaim::acquire(dir).map_err(|e| {
        if crate::store::is_busy(&e) {
            RunError::AlreadyStarted
        } else {
            RunError::Store(e)
        }
    })
}

/// Runs the slot once from the exact package bytes the plan names. Identity
/// checks happen before anything is spawned.
pub fn execute(
    plan: &TrialPlan,
    slot_id: &str,
    package_bytes: &[u8],
    dir: &Path,
    cfg: &Config,
) -> Result<Finished, RunError> {
    let binding = plan.expected_binding(slot_id)?;
    // Digest, canonical bytes, labels, dependencies, and the host-selected
    // runtime and protocol, all against the frozen binding (#36).
    let package =
        MethodPackage::load_for(package_bytes, &binding, &cfg.runtime(), &refs::protocol())
            .map_err(|e| RunError::Refused(format!("method package: {e}")))?;
    let actual = binding.method.clone();
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
    // Held until this function returns: admission, every transition and
    // finalization belong to one runner.
    let _claim = claim(dir)?;
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
    let files = stage.join("files");
    std::fs::create_dir_all(&files)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        // Only root (before the privilege drop) or the same user reads it.
        std::fs::set_permissions(&stage, std::fs::Permissions::from_mode(0o700))?;
    }
    // Paths are validated relative paths with no `..`; files are created
    // here, never linked.
    for file in package.files() {
        let path = files.join(&file.path);
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        std::fs::write(&path, file.content.as_bytes())?;
    }

    let grant = binding.budget.wall_time_ms;
    let deadline = Duration::from_millis(grant.saturating_sub(KILL_MARGIN_MS));
    let started = Instant::now();
    let mut child = match cfg
        .sandbox
        .command(INTERPRETER, package.entrypoint(), &stage)
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
    let stdin = child.stdin.take().expect("piped stdin");
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
    // Replies go through a bounded queue to a writer thread, so a method that
    // never reads can't block this loop past its deadline.
    let (reply_tx, reply_rx) = mpsc::sync_channel::<Vec<u8>>(MAX_QUEUED_REPLIES);
    std::thread::spawn(move || {
        let mut stdin = stdin;
        for out in reply_rx {
            if stdin.write_all(&out).and_then(|_| stdin.flush()).is_err() {
                break;
            }
        }
    });

    let mut outcome = None;
    let mut fatal = None;
    loop {
        // Checked before every message: queued requests never extend the run.
        let Some(left) = deadline.checked_sub(started.elapsed()) else {
            outcome = Some(ExecutionOutcome::Interrupted);
            break;
        };
        match rx.recv_timeout(left) {
            Ok(Some(line)) => {
                let reply = match slot.handle(line.trim_end()) {
                    Ok(reply) => reply,
                    Err(e) => {
                        fatal = Some(e);
                        break;
                    }
                };
                let mut out = serde_json::to_vec(&reply).expect("reply serializes");
                out.push(b'\n');
                match reply_tx.try_send(out) {
                    Ok(()) | Err(mpsc::TrySendError::Disconnected(_)) => {}
                    Err(mpsc::TrySendError::Full(_)) => {
                        // The method keeps sending without reading replies.
                        outcome = Some(ExecutionOutcome::Failed);
                        break;
                    }
                }
            }
            Ok(None) => {
                outcome = Some(ExecutionOutcome::Failed);
                break;
            }
            Err(mpsc::RecvTimeoutError::Timeout) => {
                outcome = Some(ExecutionOutcome::Interrupted);
                break;
            }
            Err(mpsc::RecvTimeoutError::Disconnected) => break,
        }
    }
    drop(reply_tx);
    // Unconditional cleanup: the child is stopped before any evidence is
    // finalized, on every path out of the loop.
    if outcome.is_some() || fatal.is_some() {
        let _ = child.kill();
    }
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
    if let Some(e) = fatal {
        // The store or ledger failed mid-run; leave the slot to `recover`
        // (which settles any pending call as unknown) rather than guess.
        let _ = std::fs::remove_dir_all(&stage);
        return Err(e);
    }
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
