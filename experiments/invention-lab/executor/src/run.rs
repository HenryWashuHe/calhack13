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
/// Replies waiting for a method that isn't reading them; past this the loop
/// waits for room, never beyond the deadline.
pub const MAX_QUEUED_REPLIES: usize = 64;
/// Requests read from the method but not yet handled; past this the reader
/// stops reading, so the method blocks on its own output instead of the host
/// buffering it (at most this many lines of up to `MAX_LINE` bytes).
pub const MAX_QUEUED_REQUESTS: usize = 64;

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

/// Executor-observed evidence that a finished slot's method process ended.
#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Termination {
    pub slot_id: String,
    /// Canonical hash of the slot's execution receipt.
    pub execution_sha256: String,
    pub how: String,
    /// True only when every process the method could have started is known
    /// to have ended (namespaces mode: the whole PID namespace). Mode none
    /// covers the reaped method process alone, so its descendants may live on.
    pub whole_tree: bool,
}

/// Canonical hash of an execution receipt, as [`Termination`] binds it.
pub fn receipt_sha256(receipt: &genesis_core::development_method::ExecutionReceipt) -> String {
    genesis_core::digest::canonical_sha256(
        &serde_json::to_value(receipt).expect("receipt serializes"),
    )
}

/// The termination evidence a finished slot's executor recorded, if any. A
/// recovered slot (or one whose run never reached a reaped process) has none.
pub fn termination(slot_id: &str, dir: &Path) -> Result<Option<Termination>, RunError> {
    let store = Store::open(dir)?;
    let state: RunState = serde_json::from_slice(&store.read("run.json")?)
        .map_err(|e| RunError::Refused(format!("run state: {e}")))?;
    let (true, Some(how)) = (state.finished, state.terminated) else {
        return Ok(None);
    };
    let receipt: genesis_core::development_method::ExecutionReceipt =
        serde_json::from_slice(&store.read("receipt.json")?)
            .map_err(|e| RunError::Refused(format!("receipt: {e}")))?;
    Ok(Some(Termination {
        slot_id: slot_id.into(),
        execution_sha256: receipt_sha256(&receipt),
        how,
        whole_tree: state.terminated_tree,
    }))
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
    let (tx, rx) = mpsc::sync_channel::<Option<String>>(MAX_QUEUED_REQUESTS);
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
                // Backpressure: while the method isn't reading, wait for room
                // in the reply queue, but never past the deadline.
                let mut pending = Some(out);
                while let Some(out) = pending.take() {
                    match reply_tx.try_send(out) {
                        Ok(()) | Err(mpsc::TrySendError::Disconnected(_)) => {}
                        Err(mpsc::TrySendError::Full(out)) => {
                            if started.elapsed() >= deadline {
                                break;
                            }
                            std::thread::sleep(Duration::from_millis(5));
                            pending = Some(out);
                        }
                    }
                }
                if started.elapsed() >= deadline {
                    outcome = Some(ExecutionOutcome::Interrupted);
                    break;
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
        cfg.sandbox.stop(&mut child);
    }
    let status = loop {
        if let Some(status) = child.try_wait()? {
            break status;
        }
        if started.elapsed() >= deadline {
            outcome = Some(ExecutionOutcome::Interrupted);
            cfg.sandbox.stop(&mut child);
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
    // The child has been reaped: record how its end was observed, as the
    // termination evidence hidden scoring requires.
    slot.state.terminated = Some(
        match cfg.sandbox {
            Mode::Namespaces => "the executor reaped the sandbox's first process; the kernel ends every process in its PID namespace with it",
            Mode::None => "the executor reaped the method process; mode none does not cover processes the method started itself",
        }
        .into(),
    );
    slot.state.terminated_tree = cfg.sandbox == Mode::Namespaces;
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
