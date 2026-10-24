//! Operator source exchange through the existing registry client; never executes methods.
use anyhow::Context;
use clap::{Args, Subcommand};
use genesis_core::ids::OrgId;
use genesis_server::registry::{
    methods::{MethodCandidate, MethodPublication, MethodSearch},
    RegistryClient,
};
use serde::{de::DeserializeOwned, Serialize};
use std::{io::Write, path::PathBuf};

#[derive(Args)]
pub(super) struct MethodArgs {
    /// Registry origin; credentials come only from GENESIS_REGISTRY_TOKEN.
    #[arg(long, env = "GENESIS_REGISTRY_URL")]
    registry_url: String,
    #[command(subcommand)]
    command: MethodCommand,
}

#[derive(Subcommand)]
enum MethodCommand {
    /// Submit public source once. Any error may follow a committed publication; never auto-retry.
    Publish {
        /// Expected token-bound publisher; checked on the response, not a write authorization.
        #[arg(long)]
        expected_publisher: String,
        /// Strict MethodPublication JSON (public source and descriptions only).
        #[arg(long)]
        input: PathBuf,
    },
    /// Return source-free candidates; no automatic selection or acceptance.
    Search {
        /// Strict MethodSearch JSON.
        #[arg(long)]
        input: PathBuf,
    },
    /// Fetch one deliberately selected candidate, checking all metadata against canonical bytes.
    Fetch {
        /// Strict MethodCandidate JSON selected from an earlier search result.
        #[arg(long)]
        input: PathBuf,
    },
}

fn read<T: DeserializeOwned>(path: &std::path::Path) -> anyhow::Result<T> {
    // Do not echo input contents or parser diagnostics: local files can be misconfigured.
    super::load_bounded_trusted_json(path, 1024 * 1024).map_err(|_| {
        anyhow::anyhow!("method input must be a regular file of strict JSON, at most 1 MiB")
    })
}

fn client(url: &str) -> anyhow::Result<RegistryClient> {
    let token = super::secret_from_env("GENESIS_REGISTRY_TOKEN")?
        .filter(|token| !token.is_empty())
        .context("GENESIS_REGISTRY_TOKEN is required")?;
    Ok(RegistryClient::new(url, token))
}

fn encode<T: Serialize>(
    result: Result<T, genesis_server::error::ApiError>,
) -> anyhow::Result<Vec<u8>> {
    // RegistryClient returns sanitized local diagnostics, never remote response prose.
    let result = result.map_err(|e| anyhow::anyhow!("{}: {}", e.error, e.message))?;
    Ok(serde_json::to_vec_pretty(&result)?)
}

impl MethodArgs {
    pub(super) async fn run(self) -> anyhow::Result<()> {
        let bytes = match self.command {
            MethodCommand::Publish {
                expected_publisher,
                input,
            } => {
                let publication: MethodPublication = read(&input)?;
                encode(
                    client(&self.registry_url)?
                        .publish_method(&OrgId::new(expected_publisher), &publication)
                        .await,
                )?
            }
            MethodCommand::Search { input } => {
                let query: MethodSearch = read(&input)?;
                encode(client(&self.registry_url)?.search_methods(&query).await)?
            }
            MethodCommand::Fetch { input } => {
                let candidate: MethodCandidate = read(&input)?;
                encode(
                    client(&self.registry_url)?
                        .fetch_method_candidate(&candidate)
                        .await,
                )?
            }
        };
        // Serialize fully before touching stdout. A later pipe error still cannot undo a write.
        let mut stdout = std::io::stdout().lock();
        stdout.write_all(&bytes)?;
        stdout.write_all(b"\n")?;
        Ok(())
    }
}
