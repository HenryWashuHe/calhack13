use clap::Parser;
use genesis_planner::llm::{self, LlmClient};
use genesis_planner_eval::{
    load_scenarios, markdown, record_file, replay_factory, run_scenario, summarize, EvalOptions,
    Ids, Scenario,
};
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

/// Score the GENESIS planner's decisions on fixed scenarios.
///
/// Live mode uses the planner's own GENESIS_LLM_PROVIDER / GENESIS_LLM_MODEL /
/// GENESIS_LLM_API_KEY. Replay mode (--replay) makes no model calls.
#[derive(Parser)]
#[command(name = "genesis-planner-eval")]
struct Cli {
    /// Directory of scenario JSON files.
    #[arg(long, default_value = "evals/planner")]
    scenarios: PathBuf,
    /// Only these scenario ids (comma-separated).
    #[arg(long, value_delimiter = ',')]
    only: Vec<String>,
    /// Replay recorded responses from this directory instead of a live model.
    #[arg(long)]
    replay: Option<PathBuf>,
    /// Save each run's responses (IDs as placeholders) to this directory.
    #[arg(long)]
    record: Option<PathBuf>,
    /// Runs per scenario, for variance on a live model.
    #[arg(long, default_value_t = 1)]
    repeat: u32,
    /// Model calls allowed per observation, including repairs.
    #[arg(long, default_value_t = 3)]
    max_attempts: u32,
    /// Write the full JSON report here.
    #[arg(long)]
    out: Option<PathBuf>,
    /// Exit nonzero when the pass rate is below this fraction.
    #[arg(long)]
    fail_under: Option<f64>,
    /// Confirm live model calls (they cost money).
    #[arg(long)]
    live: bool,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let _ = dotenvy::dotenv();
    let cli = Cli::parse();
    let mut scenarios = load_scenarios(&cli.scenarios)?;
    if !cli.only.is_empty() {
        scenarios.retain(|s| cli.only.contains(&s.id));
        anyhow::ensure!(!scenarios.is_empty(), "no scenario matches --only");
    }
    let opts = EvalOptions {
        max_attempts: cli.max_attempts,
        commit_timeout: Duration::from_secs(15),
    };

    type Factory =
        Box<dyn Fn(&Scenario, u32, &Ids) -> anyhow::Result<Arc<dyn LlmClient>> + Send + Sync>;
    let factory: Factory = match &cli.replay {
        Some(dir) => Box::new(replay_factory(dir.clone())),
        None => {
            let client: Arc<dyn LlmClient> = llm::from_env()?
                .map(Arc::from)
                .ok_or_else(|| anyhow::anyhow!(
                    "no model configured: set GENESIS_LLM_PROVIDER, GENESIS_LLM_MODEL and GENESIS_LLM_API_KEY, or use --replay"
                ))?;
            let max_calls = scenarios.len() as u32 * cli.repeat * cli.max_attempts;
            eprintln!(
                "live model {}: {} scenarios x {} runs, at most {max_calls} model calls",
                client.model(),
                scenarios.len(),
                cli.repeat
            );
            anyhow::ensure!(cli.live, "pass --live to confirm live model calls");
            Box::new(move |_, _, _| Ok(client.clone()))
        }
    };

    let mut results = Vec::new();
    for s in &scenarios {
        for run in 0..cli.repeat {
            let r = run_scenario(s, run, factory.as_ref(), &opts).await;
            eprintln!(
                "{} #{run}: {}",
                s.id,
                if r.skipped.is_some() {
                    "skipped"
                } else if r.passed {
                    "pass"
                } else {
                    "FAIL"
                }
            );
            if let (Some(dir), None) = (&cli.record, &r.skipped) {
                std::fs::create_dir_all(dir)?;
                std::fs::write(
                    record_file(dir, &s.id, run),
                    serde_json::to_string_pretty(&r.responses)? + "\n",
                )?;
            }
            results.push(r);
        }
    }
    let summary = summarize(&results);
    println!("{}", markdown(&summary, &results));
    if let Some(out) = &cli.out {
        std::fs::write(
            out,
            serde_json::to_string_pretty(&serde_json::json!({
                "summary": summary,
                "results": results,
            }))? + "\n",
        )?;
    }
    if let Some(min) = cli.fail_under {
        anyhow::ensure!(
            summary.pass_rate >= min,
            "pass rate {:.2} below {min}",
            summary.pass_rate
        );
    }
    Ok(())
}
