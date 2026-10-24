//! Local backend operation tool. No command accepts an API key or provider URL.
use anyhow::{Context, Result};
use clap::{Parser, Subcommand};
use genesis_creao::{
    AccountLimiter, ApiVersion, CreaoClient, Limits, OperationRecord, OperationSpec, QuotaLimits,
    Runtime, RuntimeStore,
};
use genesis_verification::decode_json;
use serde_json::{json, Value};
use std::io::Read;
use std::path::Path;
use std::path::PathBuf;
use std::time::Duration;

#[derive(Parser)]
#[command(
    name = "genesis-creao",
    about = "Durable CREAO backend operations; native completion is not organization authority"
)]
struct Cli {
    #[command(subcommand)]
    command: Command,
}
#[derive(Subcommand)]
enum Command {
    /// Save a trusted operation spec without contacting CREAO or reading credentials.
    Prepare {
        #[arg(long)]
        db: PathBuf,
        #[arg(long)]
        org: String,
        #[arg(long)]
        spec: PathBuf,
    },
    /// Inspect a saved operation without disclosing its input or raw provider output.
    Status {
        #[arg(long)]
        db: PathBuf,
        #[arg(long)]
        org: String,
        #[arg(long)]
        operation: String,
    },
    /// Advance at most one POST/GET. Keep the quota database shared per account.
    Step {
        #[arg(long)]
        db: PathBuf,
        #[arg(long)]
        org: String,
        #[arg(long)]
        operation: String,
        #[arg(long)]
        account: String,
        #[arg(long)]
        quota_db: PathBuf,
        #[arg(long)]
        retry_read: bool,
    },
    /// Startup-only recovery; mark interrupted POSTs ambiguous, resume known GETs.
    Recover {
        #[arg(long)]
        db: PathBuf,
        #[arg(long)]
        org: String,
        #[arg(long)]
        account: String,
        #[arg(long)]
        quota_db: PathBuf,
    },
}

fn read_spec(path: &Path) -> Result<OperationSpec> {
    let file = std::fs::File::open(path).context("open trusted operation spec")?;
    let metadata = file.metadata().context("read spec metadata")?;
    anyhow::ensure!(
        metadata.is_file() && metadata.len() <= 1024 * 1024,
        "spec must be a regular file no larger than 1 MiB"
    );
    let mut bytes = Vec::new();
    file.take(1024 * 1024 + 1)
        .read_to_end(&mut bytes)
        .context("read spec")?;
    anyhow::ensure!(bytes.len() <= 1024 * 1024, "spec exceeds 1 MiB");
    let spec: OperationSpec =
        decode_json(&bytes).map_err(|_| anyhow::anyhow!("invalid or ambiguous operation JSON"))?;
    spec.validate()?;
    Ok(spec)
}

fn runtime(db: &Path, org: &str, account: &str, quota_db: &Path) -> Result<Runtime> {
    let token = std::env::var("CREAO_ACCOUNT_API_KEY").map_err(|_| {
        anyhow::anyhow!("CREAO_ACCOUNT_API_KEY is required in the backend environment")
    })?;
    let limiter = AccountLimiter::open(quota_db, account, QuotaLimits::default())?;
    let version = match std::env::var("CREAO_API_VERSION") {
        Err(std::env::VarError::NotPresent) => ApiVersion::V1,
        Ok(value) if value == "v1" => ApiVersion::V1,
        Ok(value) if value == "v2" => ApiVersion::V2,
        _ => anyhow::bail!("CREAO_API_VERSION must be v1 or v2"),
    };
    let client = CreaoClient::new_for_version(&token, Limits::default(), limiter, version)?;
    Ok(Runtime::new(
        client,
        RuntimeStore::open(db, org)?,
        Duration::from_secs(5),
    )?)
}

fn summary(record: &OperationRecord) -> Value {
    json!({"operation_id":record.spec.operation_id,"org_id":record.spec.org_id,"role_id":record.spec.role_id,
        "account_namespace":record.spec.account_namespace,"phase":record.phase,
        "run_id":record.ticket.as_ref().map(|ticket|ticket.run_id()),
        "native_status":record.observation.as_ref().map(|observation|observation.status.as_str()),
        "provider_agent_id":record.observation.as_ref().and_then(|observation|observation.agent_id.as_deref()),
        "retryable_read":record.retryable_read,"next_poll_at":record.next_poll_at,
        "failure":record.failure,"readiness_verified":false,"domain_verified":false})
}

#[tokio::main]
async fn main() -> Result<()> {
    let output = match Cli::parse().command {
        Command::Prepare { db, org, spec } => {
            let spec = read_spec(&spec)?;
            summary(&RuntimeStore::open(db, &org)?.prepare(spec)?)
        }
        Command::Status { db, org, operation } => {
            summary(&RuntimeStore::open(db, &org)?.get(&operation)?)
        }
        Command::Step {
            db,
            org,
            operation,
            account,
            quota_db,
            retry_read,
        } => summary(
            &runtime(&db, &org, &account, &quota_db)?
                .step(&operation, retry_read)
                .await?,
        ),
        Command::Recover {
            db,
            org,
            account,
            quota_db,
        } => {
            json!({"recovered":runtime(&db,&org,&account,&quota_db)?.recover_interrupted().await?,"provider_requests":0})
        }
    };
    println!("{}", serde_json::to_string_pretty(&output)?);
    Ok(())
}
