mod storage;

use genesis_apify::{ApifyClient, RunLimits};
use genesis_forge::{venue_source, CandidateSpec, ForgeSession, Phase};
use genesis_verification::OracleSuite;
use serde::Deserialize;
use serde_json::json;
use std::collections::BTreeMap;
use std::ffi::OsString;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::ExitCode;
use storage::{CheckpointLock, MAX_EXTRACTOR_BYTES};

const HELP: &str = "Usage:
  genesis-forge prepare --candidate metadata.json --extractor parser.ts --suite suite.json --checkpoint session.json
  genesis-forge step --suite suite.json --checkpoint session.json [--retry-read]
  genesis-forge status --suite suite.json --checkpoint session.json
  genesis-forge export --suite suite.json --checkpoint session.json

prepare is offline and refuses to overwrite a checkpoint. step performs one
checkpointed action against https://api.apify.com using APIFY_TOKEN from the
environment. The token is never accepted as an argument or stored in a checkpoint.
Each step saves pending intent before any provider request. Pending POSTs after a
restart require reconciliation; this CLI never replays them. Retryable read failures
require an explicit --retry-read. No action loop or automatic retry is provided.
export emits a reverified manifest draft; it grants no publication or installation
authority. Checkpoints contain private source and outputs; do not give them to Actors.
Exit codes: 0 success/progress, 1 rejected/failed/reconciliation required, 2 invalid input or local error.
";

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct CandidateMetadata {
    operation_id: String,
    capability_id: String,
    capability_version: String,
    actor_name: String,
    source_version: String,
}

struct Arguments {
    command: String,
    paths: BTreeMap<String, PathBuf>,
    retry_read: bool,
}

impl Arguments {
    fn parse(mut args: impl Iterator<Item = OsString>) -> Result<Option<Self>, String> {
        let first = args.next().ok_or_else(|| HELP.to_string())?;
        if first == "--help" || first == "-h" {
            if args.next().is_some() {
                return Err("help does not accept additional arguments".into());
            }
            return Ok(None);
        }
        let command = first
            .to_str()
            .ok_or_else(|| "invalid command".to_string())?
            .to_string();
        let allowed: &[&str] = match command.as_str() {
            "prepare" => &["--candidate", "--extractor", "--suite", "--checkpoint"],
            "step" | "status" | "export" => &["--suite", "--checkpoint"],
            _ => return Err(format!("unknown command\n{HELP}")),
        };
        let mut paths = BTreeMap::new();
        let mut retry_read = false;
        while let Some(raw) = args.next() {
            let flag = raw.to_str().ok_or_else(|| "invalid argument".to_string())?;
            if flag == "--retry-read" && command == "step" {
                if retry_read {
                    return Err("duplicate --retry-read".into());
                }
                retry_read = true;
                continue;
            }
            if !allowed.contains(&flag) {
                return Err("unknown argument for command".into());
            }
            let path = args
                .next()
                .ok_or_else(|| format!("missing path for {flag}"))?;
            if path.is_empty() || path.to_string_lossy().starts_with("--") {
                return Err(format!("missing path for {flag}"));
            }
            if paths
                .insert(flag.to_string(), PathBuf::from(path))
                .is_some()
            {
                return Err(format!("duplicate {flag}"));
            }
        }
        for flag in allowed {
            if !paths.contains_key(*flag) {
                return Err(format!("missing {flag}"));
            }
        }
        Ok(Some(Self {
            command,
            paths,
            retry_read,
        }))
    }

    fn path(&self, flag: &str) -> Result<&Path, String> {
        self.paths
            .get(flag)
            .map(PathBuf::as_path)
            .ok_or_else(|| format!("missing {flag}"))
    }
}

#[tokio::main(flavor = "current_thread")]
async fn main() -> ExitCode {
    match run().await {
        Ok(code) => ExitCode::from(code),
        Err(error) => {
            eprintln!("genesis-forge: {error}");
            ExitCode::from(2)
        }
    }
}

async fn run() -> Result<u8, String> {
    let Some(args) = Arguments::parse(std::env::args_os().skip(1))? else {
        print!("{HELP}");
        return Ok(0);
    };
    let suite: OracleSuite = storage::read_json(args.path("--suite")?)?;
    let lock = CheckpointLock::acquire(args.path("--checkpoint")?)?;
    if args.command == "prepare" {
        let metadata: CandidateMetadata = storage::read_json(args.path("--candidate")?)?;
        let parser = String::from_utf8(storage::read_bytes(
            args.path("--extractor")?,
            MAX_EXTRACTOR_BYTES,
        )?)
        .map_err(|_| "extractor must be UTF-8 TypeScript".to_string())?;
        let urls = suite
            .cases
            .iter()
            .map(|case| case.source.url.clone())
            .collect::<Vec<_>>();
        let source = venue_source(&parser, &urls).map_err(|error| error.to_string())?;
        let spec = CandidateSpec {
            operation_id: metadata.operation_id,
            capability_id: metadata.capability_id,
            capability_version: metadata.capability_version,
            actor_name: metadata.actor_name,
            source_version: metadata.source_version,
            source,
        };
        let session = ForgeSession::new(spec, &suite).map_err(|error| error.to_string())?;
        lock.create(&session)?;
        emit_status(&session, &suite)?;
        return Ok(0);
    }

    let mut session: ForgeSession = lock.load()?;
    session
        .validate(&suite)
        .map_err(|error| error.to_string())?;
    match args.command.as_str() {
        "status" => {
            emit_status(&session, &suite)?;
            Ok(0)
        }
        "export" => {
            let verified = session
                .verified(&suite)
                .map_err(|error| error.to_string())?;
            emit(&verified)?;
            Ok(0)
        }
        "step" => {
            if session
                .recover_interrupted_submission(&suite)
                .map_err(|error| error.to_string())?
            {
                lock.replace(&session)?;
                emit_status(&session, &suite)?;
                eprintln!("genesis-forge: pending provider submission requires reconciliation; no request was sent");
                return Ok(1);
            }
            if matches!(
                session.phase(),
                Phase::Verified | Phase::Rejected | Phase::Failed | Phase::NeedsReconciliation
            ) {
                emit_status(&session, &suite)?;
                return Ok(if session.phase() == Phase::Verified {
                    0
                } else {
                    1
                });
            }
            let retryable_read_failure = session.failure().is_some_and(|failure| failure.retryable);
            if retryable_read_failure && !args.retry_read {
                return Err("previous read failed; inspect status and explicitly use --retry-read to retry that read".into());
            }
            if args.retry_read && !retryable_read_failure {
                return Err("--retry-read requires a prior retryable read failure".into());
            }

            // Validate credentials/configuration before marking an action pending.
            // The fixed endpoint cannot be redirected to an untrusted CLI target.
            let token = std::env::var("APIFY_TOKEN")
                .map_err(|_| "APIFY_TOKEN is required for step".to_string())?;
            let client = ApifyClient::new(&token, RunLimits::default())
                .map_err(|error| error.to_string())?;
            let Some(action) = session
                .prepare_next(&suite)
                .map_err(|error| error.to_string())?
            else {
                emit_status(&session, &suite)?;
                return Ok(0);
            };
            let completion = lock
                .persist_before_effect(&session, || action.execute(&client))
                .await?;
            session
                .complete(completion, &suite)
                .map_err(|error| error.to_string())?;
            lock.replace(&session)?;
            emit_status(&session, &suite)?;
            Ok(
                if session.failure().is_some()
                    || matches!(
                        session.phase(),
                        Phase::Rejected | Phase::Failed | Phase::NeedsReconciliation
                    )
                {
                    1
                } else {
                    0
                },
            )
        }
        _ => Err("unsupported command".into()),
    }
}

fn emit_status(session: &ForgeSession, suite: &OracleSuite) -> Result<(), String> {
    emit(&json!({
        "schema_version": 1,
        "operation_id": session.spec().operation_id,
        "capability_id": session.spec().capability_id,
        "capability_version": session.spec().capability_version,
        "phase": session.phase(),
        "pending_action": session.pending_action(),
        "completed_cases": session.completed_cases(),
        "total_cases": suite.cases.len(),
        "artifact": session.artifact(),
        "failure": session.failure(),
        "authority": "none: verification and manifest drafts do not grant publication or installation"
    }))
}

fn emit(value: &impl serde::Serialize) -> Result<(), String> {
    let bytes = storage::encode_json(value)?;
    std::io::stdout()
        .lock()
        .write_all(&bytes)
        .map_err(|_| "cannot write JSON output".into())
}
