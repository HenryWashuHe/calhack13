mod acquisition_commands;
mod method_accounting_commands;
mod method_admission_commands;
mod method_revision_commands;
mod method_roster_commands;
mod method_synthesis_commands;
mod storage;
mod synthesis_commands;

use genesis_apify::{ApifyClient, RunLimits};
use genesis_forge::{venue_source_for_actor, CandidateSpec, ForgeSession, Phase};
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
  genesis-forge method-generate-prepare --plan method-plan.json --checkpoint method-generation.json
  genesis-forge method-generate-step --checkpoint method-generation.json [--admission admission.json]
  genesis-forge method-admission-prepare --roster roster.json --admission admission.json
  genesis-forge method-admission-status --admission admission.json
  genesis-forge method-generate-status --checkpoint method-generation.json
  genesis-forge method-generate-export --checkpoint method-generation.json
  genesis-forge method-cost-assess --manifest cost-evidence.json [--admission admission.json]
  genesis-forge method-roster-prepare --manifest declaration.json --roster roster.json
  genesis-forge method-roster-assess --manifest snapshots.json --roster roster.json
  genesis-forge method-revise-prepare --plan revision.json --parent parent.package.json --checkpoint generation.json
  genesis-forge method-revise-observe --plan revision.json --parent parent.package.json --checkpoint generation.json
  genesis-forge generate-prepare --request public-request.json --model-config model.json --checkpoint generation.json
  genesis-forge generate-step --checkpoint generation.json
  genesis-forge generate-status --checkpoint generation.json
  genesis-forge prepare-generated --candidate metadata.json --generation generation.json --suite suite.json --checkpoint session.json
  genesis-forge acquire-prepare --plan plan.json --suite suite.json --checkpoint acquisition.json
  genesis-forge acquire-step --suite suite.json --checkpoint acquisition.json [--retry-read]
  genesis-forge acquire-run --suite suite.json --checkpoint acquisition.json --max-steps N [--retry-read]
  genesis-forge acquire-status --suite suite.json --checkpoint acquisition.json
  genesis-forge acquire-export --suite suite.json --checkpoint acquisition.json
  genesis-forge acquire-export-forge --suite suite.json --checkpoint acquisition.json --output forge.json
  genesis-forge step --suite suite.json --checkpoint session.json [--retry-read]
  genesis-forge status --suite suite.json --checkpoint session.json
  genesis-forge export --suite suite.json --checkpoint session.json

prepare is offline and refuses to overwrite a checkpoint. step performs one
checkpointed action against https://api.apify.com using APIFY_TOKEN from the
environment. The token is never accepted as an argument or stored in a checkpoint.
Each step saves pending intent before any provider request. Pending POSTs after a
restart require reconciliation; this CLI never replays them. Retryable read failures
require an explicit --retry-read. step does not loop or automatically retry.
export emits a reverified manifest draft; it grants no publication or installation
authority. Checkpoints contain private source and outputs; do not give them to Actors.
generate-prepare is offline. generate-step makes at most one Messages API call using
ANTHROPIC_API_KEY, with the explicit model stored in generation.json. Pending calls
after interruption require reconciliation and are never automatically replayed.
Generation receives public examples only, never a verification suite. Generated
source is untrusted; prepare-generated packages it without executing it locally.
acquire-run advances one durable workflow for at most N steps (1..64), stopping
on failures. It can perform one preplanned semantic repair, never an automatic
retry of failed provider submissions. Its status output is newline-delimited JSON.
method-cost-assess is offline and read-only. Exit 0 means report construction,
including unknown usage or overruns; it is not acceptance or budget compliance.
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
    max_steps: Option<u32>,
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
            "prepare-generated" => &["--candidate", "--generation", "--suite", "--checkpoint"],
            "method-generate-prepare" => &["--plan", "--checkpoint"],
            "method-cost-assess" => &["--manifest", "--admission"],
            "method-admission-prepare" => &["--roster", "--admission"],
            "method-admission-status" => &["--admission"],
            "method-roster-prepare" | "method-roster-assess" => &["--manifest", "--roster"],
            "method-revise-prepare" | "method-revise-observe" => {
                &["--plan", "--parent", "--checkpoint"]
            }
            "method-generate-step" => &["--checkpoint", "--admission"],
            "method-generate-status" | "method-generate-export" => &["--checkpoint"],
            "generate-prepare" => &["--request", "--model-config", "--checkpoint"],
            "generate-step" | "generate-status" => &["--checkpoint"],
            "acquire-prepare" => &["--plan", "--suite", "--checkpoint"],
            "acquire-step" | "acquire-run" | "acquire-status" | "acquire-export" => {
                &["--suite", "--checkpoint"]
            }
            "acquire-export-forge" => &["--suite", "--checkpoint", "--output"],
            "step" | "status" | "export" => &["--suite", "--checkpoint"],
            _ => return Err(format!("unknown command\n{HELP}")),
        };
        let mut paths = BTreeMap::new();
        let mut retry_read = false;
        let mut max_steps = None;
        while let Some(raw) = args.next() {
            let flag = raw.to_str().ok_or_else(|| "invalid argument".to_string())?;
            if flag == "--max-steps" && command == "acquire-run" {
                if max_steps.is_some() {
                    return Err("duplicate --max-steps".into());
                }
                let count = args
                    .next()
                    .and_then(|v| v.to_str().and_then(|s| s.parse::<u32>().ok()))
                    .filter(|n| (1..=64).contains(n))
                    .ok_or("--max-steps must be an integer between 1 and 64")?;
                max_steps = Some(count);
                continue;
            }
            if flag == "--retry-read"
                && matches!(command.as_str(), "step" | "acquire-step" | "acquire-run")
            {
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
            if matches!(
                command.as_str(),
                "method-generate-step" | "method-cost-assess"
            ) && *flag == "--admission"
            {
                continue;
            }
            if !paths.contains_key(*flag) {
                return Err(format!("missing {flag}"));
            }
        }
        if command == "acquire-run" && max_steps.is_none() {
            return Err("acquire-run requires --max-steps".into());
        }
        Ok(Some(Self {
            command,
            paths,
            retry_read,
            max_steps,
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
    if args.command.starts_with("method-generate-") {
        return method_synthesis_commands::run(&args).await;
    }
    if args.command.starts_with("method-admission-") {
        return method_admission_commands::run(&args);
    }
    if args.command == "method-cost-assess" {
        return method_accounting_commands::run(&args);
    }
    if args.command.starts_with("method-roster-") {
        return method_roster_commands::run(&args);
    }
    if args.command.starts_with("method-revise-") {
        return method_revision_commands::run(&args);
    }
    if args.command.starts_with("generate-") {
        return synthesis_commands::run(&args).await;
    }
    if args.command.starts_with("acquire-") {
        return acquisition_commands::run(&args).await;
    }
    let suite: OracleSuite = storage::read_json(args.path("--suite")?)?;
    let lock = CheckpointLock::acquire(args.path("--checkpoint")?)?;
    if args.command == "prepare" || args.command == "prepare-generated" {
        let metadata: CandidateMetadata = storage::read_json(args.path("--candidate")?)?;
        let source = if args.command == "prepare-generated" {
            synthesis_commands::generated_source(&args, &metadata, &suite)?
        } else {
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
            venue_source_for_actor(
                &parser,
                &urls,
                &metadata.actor_name,
                &metadata.source_version,
            )
            .map_err(|error| error.to_string())?
        };
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
