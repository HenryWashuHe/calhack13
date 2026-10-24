//! Executor v0 against the #32 ledger. Local processes only; the sandbox test
//! runs only as root. Methods here are tiny test scripts, never real methods.
use genesis_core::development_experiment::{ExperimentLedger, ExperimentOutcome, ExperimentScope};
use genesis_core::development_method::{ExecutionOutcome, ResourceGrant, ResourceUsage};
use genesis_core::development_trial::TrialPlan;
use genesis_lab_exec::run::{self, Config, Method};
use genesis_lab_exec::session::RunError;
use genesis_lab_exec::{example, refs, sandbox::Mode};
use std::path::{Path, PathBuf};

const CART: &str = r#"{"language":1,"payload_on":"deck","bodies":[
 {"shape":"rect","id":"deck","x":-2.0,"y":0.6,"w":3.6,"h":0.3,"material":"wood"},
 {"shape":"circle","id":"w1","x":-3.5,"y":0.45,"r":0.3,"material":"rubber"},
 {"shape":"circle","id":"w2","x":-0.5,"y":0.45,"r":0.3,"material":"rubber"}],
 "joints":[
 {"a":"deck","b":"w1","kind":"hinge","x":-3.5,"y":0.45,"motor_speed":-5.0},
 {"a":"deck","b":"w2","kind":"hinge","x":-0.5,"y":0.45,"motor_speed":-5.0}]}"#;

const PRELUDE: &str = r#"
import json, sys
def call(req):
    sys.stdout.write(json.dumps(req) + "\n"); sys.stdout.flush()
    line = sys.stdin.readline()
    return json.loads(line) if line else {}
"#;

struct Fixture {
    dir: PathBuf,
    plan: TrialPlan,
    method: Method,
    cfg: Config,
}

fn fixture(name: &str, body: &str, mode: Mode, steps: u64, wall_ms: u64) -> Fixture {
    let base = if mode == Mode::Namespaces {
        PathBuf::from("/var/tmp")
    } else {
        std::env::temp_dir()
    };
    let dir = base.join(format!("lab-exec-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(dir.join("method")).unwrap();
    let script = format!("{PRELUDE}\nCART = json.loads('''{CART}''')\n{body}\n");
    std::fs::write(dir.join("method/m.py"), script).unwrap();
    std::fs::write(
        dir.join("method/method.json"),
        format!(r#"{{"id":"test-{name}","version":"1","label":"control","interpreter":"python3","entry":"m.py"}}"#),
    )
    .unwrap();
    let method = Method::load(&dir.join("method/method.json")).unwrap();
    // The control slot only needs a distinct identity; it never runs here.
    let control = refs::content("lab-method/1", "test-control", "1", b"control");
    let version = run::interpreter_version("python3");
    let plan = example::acceptance_plan(
        &example::ExampleMethods {
            candidate: method.reference().unwrap(),
            control,
        },
        mode,
        &version,
        3,
        ResourceGrant {
            model_calls: 0,
            input_tokens: 0,
            output_tokens: 0,
            wall_time_ms: wall_ms,
            simulation_steps: steps,
        },
    );
    let cfg = Config {
        sandbox: mode,
        interpreter_version: version,
        stage_root: if mode == Mode::Namespaces {
            PathBuf::from("/run/genesis-lab-test")
        } else {
            dir.join("stage")
        },
    };
    Fixture {
        dir,
        plan,
        method,
        cfg,
    }
}

impl Fixture {
    fn run(&self) -> Result<genesis_lab_exec::session::Finished, RunError> {
        run::execute(
            &self.plan,
            "candidate",
            &self.method,
            &self.dir.join("slot"),
            &self.cfg,
        )
    }
    fn ledger(&self) -> ExperimentLedger {
        serde_json::from_slice(&std::fs::read(self.dir.join("slot/ledger.json")).unwrap()).unwrap()
    }
    fn scope(&self) -> ExperimentScope<'_> {
        ExperimentScope {
            plan: &self.plan,
            slot_id: "candidate",
        }
    }
}

#[test]
fn a_method_whose_digest_differs_from_the_plan_never_runs() {
    let mut f = fixture(
        "digest",
        "call({'op':'submit','design':CART})",
        Mode::None,
        7200,
        20_000,
    );
    f.method
        .entry_bytes
        .extend_from_slice(b"\n# edited after the plan froze\n");
    assert!(matches!(f.run(), Err(RunError::Refused(_))));
    assert!(!f.dir.join("slot/ledger.json").exists());
}

#[test]
fn the_wall_time_limit_interrupts_and_keeps_the_submission() {
    let body = "call({'op':'submit','design':CART})\nimport time\ntime.sleep(60)";
    let f = fixture("wall", body, Mode::None, 7200, 1500);
    let done = f.run().unwrap();
    assert_eq!(done.receipt.outcome, ExecutionOutcome::Interrupted);
    assert_eq!(done.receipt.outputs.len(), 1);
    match done.receipt.usage.wall_time_ms {
        ResourceUsage::Measured { value } => assert!(value <= 1500, "{value}"),
        other => panic!("{other:?}"),
    }
}

#[test]
fn only_one_output_is_retained_and_oversized_requests_fail_the_run() {
    let body = r#"
assert call({'op':'submit','design':CART})['ok']
r = call({'op':'submit','design':CART})
assert not r['ok'] and 'already' in r['refused'], r
"#;
    let f = fixture("twice", body, Mode::None, 7200, 20_000);
    let done = f.run().unwrap();
    assert_eq!(done.receipt.outcome, ExecutionOutcome::OutputGenerated);
    assert_eq!(done.receipt.outputs.len(), 1);

    let body = "sys.stdout.write('x' * (2 << 20)); sys.stdout.flush()\nimport time\ntime.sleep(5)";
    let f = fixture("huge", body, Mode::None, 7200, 20_000);
    assert_eq!(f.run().unwrap().receipt.outcome, ExecutionOutcome::Failed);
}

#[test]
fn a_restarted_executor_settles_the_pending_call_unknown_and_never_restarts() {
    let f = fixture(
        "recover",
        "call({'op':'submit','design':CART})",
        Mode::None,
        7200,
        20_000,
    );
    // Simulate a crash right after the pending call was made durable.
    let slot = f.dir.join("slot");
    std::fs::create_dir_all(&slot).unwrap();
    let mut ledger = ExperimentLedger::new(f.scope()).unwrap();
    let design = refs::content("lab-design/1", "submission", "1", CART.as_bytes());
    ledger
        .reserve(
            f.scope(),
            f.plan.development_instances[0].clone(),
            design,
            2400,
        )
        .unwrap();
    std::fs::write(
        slot.join("ledger.json"),
        serde_json::to_vec(&ledger).unwrap(),
    )
    .unwrap();
    let state = genesis_lab_exec::session::RunState {
        started: true,
        method: Some(f.method.reference().unwrap()),
        ..Default::default()
    };
    std::fs::write(slot.join("run.json"), serde_json::to_vec(&state).unwrap()).unwrap();
    assert!(matches!(f.run(), Err(RunError::AlreadyStarted)));
    let done = run::recover(&f.plan, "candidate", &slot, &f.cfg).unwrap();
    assert_eq!(done.receipt.outcome, ExecutionOutcome::Unknown);
    assert!(matches!(
        done.receipt.usage.simulation_steps,
        ResourceUsage::Unknown { .. }
    ));
    let ledger = f.ledger();
    assert_eq!(ledger.records().len(), 1);
    assert!(matches!(
        ledger.records()[0].settlement.as_ref().unwrap().outcome,
        ExperimentOutcome::Unknown { .. }
    ));
    ledger.validate_execution(f.scope(), &done.receipt).unwrap();
    assert!(matches!(
        run::recover(&f.plan, "candidate", &slot, &f.cfg),
        Err(RunError::AlreadyStarted)
    ));
    assert!(matches!(f.run(), Err(RunError::AlreadyStarted)));
}

#[test]
fn the_namespace_sandbox_hides_files_network_and_processes() {
    if unsafe_euid() != 0 {
        eprintln!("skipped: the namespace sandbox needs root");
        return;
    }
    let body = r#"
import os, socket
problems = []
if os.getuid() != 65534: problems.append('uid')
try:
    socket.create_connection(('1.1.1.1', 443), 2); problems.append('network')
except OSError: pass
if os.listdir('/home'): problems.append('home visible')
if any(p != 'work' for p in os.listdir('/tmp')): problems.append('tmp visible')
pids = [p for p in os.listdir('/proc') if p.isdigit()]
if len(pids) > 4: problems.append('pids %d' % len(pids))
if os.listdir('/run') or os.listdir('/var/tmp'): problems.append('run or var/tmp visible')
try:
    open('/var/lib/lab-exec-probe/escape', 'w').write('x'); problems.append('wrote the root filesystem')
except OSError: pass
open('/var/tmp/lab-exec-escape', 'w').write('private tmpfs only')
if problems: sys.exit(1)
call({'op':'submit','design':CART})
"#;
    // A host directory anyone may write: the read-only root must still refuse.
    std::fs::create_dir_all("/var/lib/lab-exec-probe").unwrap();
    std::fs::set_permissions(
        "/var/lib/lab-exec-probe",
        std::os::unix::fs::PermissionsExt::from_mode(0o777),
    )
    .unwrap();
    let _ = std::fs::remove_file("/var/tmp/lab-exec-escape");
    let _ = std::fs::remove_file("/var/lib/lab-exec-probe/escape");
    let f = fixture("sandbox", body, Mode::Namespaces, 7200, 20_000);
    let done = f.run().unwrap();
    assert!(
        !Path::new("/var/tmp/lab-exec-escape").exists(),
        "a write left the sandbox"
    );
    assert!(!Path::new("/var/lib/lab-exec-probe/escape").exists());
    assert_eq!(done.receipt.outcome, ExecutionOutcome::OutputGenerated);
    assert_eq!(
        done.receipt.usage.model_calls,
        ResourceUsage::Measured { value: 0 }
    );
}

fn unsafe_euid() -> u32 {
    // Read from /proc to avoid a libc dependency.
    let status = std::fs::read_to_string("/proc/self/status").unwrap_or_default();
    status
        .lines()
        .find(|l| l.starts_with("Uid:"))
        .and_then(|l| l.split_whitespace().nth(2))
        .and_then(|v| v.parse().ok())
        .unwrap_or(u32::MAX)
}

#[test]
fn a_stop_after_sealing_still_recovers_with_the_call_pending() {
    let f = fixture(
        "sealed",
        "call({'op':'submit','design':CART})",
        Mode::None,
        7200,
        20_000,
    );
    let slot = f.dir.join("slot");
    std::fs::create_dir_all(&slot).unwrap();
    let mut ledger = ExperimentLedger::new(f.scope()).unwrap();
    let design = refs::content("lab-design/1", "submission", "1", CART.as_bytes());
    ledger
        .reserve(
            f.scope(),
            f.plan.development_instances[0].clone(),
            design,
            2400,
        )
        .unwrap();
    ledger.seal(f.scope()).unwrap();
    let state = genesis_lab_exec::session::RunState {
        started: true,
        method: Some(f.method.reference().unwrap()),
        ..Default::default()
    };
    std::fs::write(slot.join("run.json"), serde_json::to_vec(&state).unwrap()).unwrap();
    std::fs::write(
        slot.join("ledger.json"),
        serde_json::to_vec(&ledger).unwrap(),
    )
    .unwrap();
    let done = run::recover(&f.plan, "candidate", &slot, &f.cfg).unwrap();
    assert_eq!(done.receipt.outcome, ExecutionOutcome::Unknown);
    let ledger = f.ledger();
    assert!(
        ledger.records()[0].settlement.is_none(),
        "kept pending, not invented"
    );
    assert!(matches!(
        done.receipt.usage.simulation_steps,
        ResourceUsage::Unknown { .. }
    ));
    ledger.validate_execution(f.scope(), &done.receipt).unwrap();
}
