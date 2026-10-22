use clap::{Parser, Subcommand};
use genesis_core::ids::ObservationId;
use genesis_planner::agent::Planner;
use genesis_planner::kernel::KernelClient;
use genesis_planner::llm;
use genesis_planner::prompt::KnownSpecialist;
use genesis_planner::runner::Runner;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

#[derive(Parser)]
#[command(name = "genesis-planner", about = "GENESIS mission planning agent")]
struct Cli {
    /// Organization kernel URL.
    #[arg(
        long,
        env = "GENESIS_ORG_URL",
        default_value = "http://127.0.0.1:7101",
        global = true
    )]
    org_url: String,
    /// The organization's agent token (not the operator token).
    #[arg(
        long,
        env = "GENESIS_AGENT_TOKEN",
        default_value = "org_a-agent",
        global = true
    )]
    token: String,
    /// JSON file listing discovered external specialists the planner may recruit.
    #[arg(long, env = "GENESIS_SPECIALISTS", global = true)]
    specialists: Option<PathBuf>,
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Watch the event log and plan for each new observation.
    Run {
        #[arg(long, default_value_t = 1000)]
        poll_ms: u64,
        #[arg(long, default_value_t = 3)]
        max_attempts: u32,
    },
    /// Plan once for a specific observation.
    Once {
        #[arg(long)]
        observation: String,
    },
    /// Print the prompt for an observation. Works without a model configured.
    Prompt {
        #[arg(long)]
        observation: String,
    },
}

fn specialists(path: &Option<PathBuf>) -> anyhow::Result<Vec<KnownSpecialist>> {
    match path {
        None => Ok(vec![]),
        Some(p) => Ok(serde_json::from_str(&std::fs::read_to_string(p)?)?),
    }
}

/// A client used only to preview prompts when no model is configured.
struct Pending;

#[async_trait::async_trait]
impl llm::LlmClient for Pending {
    fn model(&self) -> String {
        "pending".into()
    }
    async fn complete(&self, _: &llm::LlmRequest) -> Result<String, llm::LlmError> {
        Err(llm::LlmError::NotConfigured(
            "set GENESIS_LLM_PROVIDER, GENESIS_LLM_MODEL and GENESIS_LLM_API_KEY".into(),
        ))
    }
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let _ = dotenvy::dotenv();
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env().unwrap_or_else(|_| "info".into()),
        )
        .init();
    let cli = Cli::parse();
    let kernel = KernelClient::new(&cli.org_url, &cli.token);
    let model: Option<Arc<dyn llm::LlmClient>> = llm::from_env()?.map(Arc::from);

    match cli.command {
        Command::Prompt { observation } => {
            let mut planner = Planner::new(kernel, Arc::new(Pending));
            planner.specialists = specialists(&cli.specialists)?;
            let (system, user) = planner.preview(&ObservationId::new(observation)).await?;
            println!("===== SYSTEM =====\n{system}\n===== USER =====\n{user}");
            Ok(())
        }
        Command::Once { observation } => {
            let model = model.ok_or_else(|| anyhow::anyhow!("model is pending: set GENESIS_LLM_PROVIDER, GENESIS_LLM_MODEL and GENESIS_LLM_API_KEY (see docs/PLANNER.md)"))?;
            let mut planner = Planner::new(kernel, model);
            planner.specialists = specialists(&cli.specialists)?;
            let outcome = planner.plan(&ObservationId::new(observation)).await?;
            println!("{outcome:#?}");
            Ok(())
        }
        Command::Run {
            poll_ms,
            max_attempts,
        } => {
            let model = model.ok_or_else(|| anyhow::anyhow!("model is pending: set GENESIS_LLM_PROVIDER, GENESIS_LLM_MODEL and GENESIS_LLM_API_KEY (see docs/PLANNER.md)"))?;
            tracing::info!(model = %model.model(), org = %cli.org_url, "planner starting");
            let mut planner = Planner::new(kernel, model);
            planner.max_attempts = max_attempts;
            planner.specialists = specialists(&cli.specialists)?;
            Runner::new(planner, Duration::from_millis(poll_ms))
                .run_forever()
                .await
        }
    }
}
