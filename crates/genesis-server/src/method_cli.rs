//! Operator source exchange through the existing registry client; never executes methods.
use anyhow::Context;
use clap::{Args, Subcommand};
use genesis_core::{development_method::ExactContentRef, digest, ids::OrgId};
use genesis_forge::{
    method_revision::{MethodRevisionInput, MethodRevisionSpec},
    method_synthesis::PublicDocument,
};
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
    /// Fetch a selected parent and emit bounded input for source-only revision preparation.
    RevisionInput {
        /// Strict MethodCandidate JSON deliberately selected by the receiver.
        #[arg(long)]
        input: PathBuf,
        /// Receiver-owned MethodRevisionSpec; runtime, task and model settings remain local choices.
        #[arg(long)]
        plan: PathBuf,
    },
}

const SELECTION_DOMAIN: &str = "genesis.method_registry_selection";

async fn revision_input(
    url: &str,
    candidate: MethodCandidate,
    mut spec: MethodRevisionSpec,
) -> anyhow::Result<Vec<u8>> {
    // Reject incompatible local requests before credentials or HTTP. Neither the
    // registry nor the selected source may choose the receiver's runtime/plan.
    spec.sha256()
        .map_err(|_| anyhow::anyhow!("invalid revision specification"))?;
    anyhow::ensure!(
        spec.parent == candidate.reference
            && spec.generation.runtime == candidate.runtime
            && spec.generation.protocol.reference == candidate.protocol
            && spec.generation.dependencies == candidate.dependencies
            && spec.generation.public_inputs.len() <= 14
            && !spec.generation.public_inputs.iter().any(|input| {
                input.reference.domain == SELECTION_DOMAIN || input.reference == spec.parent
            }),
        "revision specification is incompatible with selected parent or added public inputs"
    );
    let offer = client(url)?
        .fetch_method_candidate(&candidate)
        .await
        .map_err(|e| anyhow::anyhow!("{}: {}", e.error, e.message))?;
    // The client checked every selected field against the fetched canonical
    // bytes. Retain that selection in the ordinary immutable generation plan.
    // This local document is not a signature or proof of adopted authority.
    let selection = serde_json::json!({
        "schema": "genesis-method-selection/1", "candidate": candidate
    });
    spec.generation.public_inputs.push(PublicDocument {
        reference: ExactContentRef {
            domain: SELECTION_DOMAIN.into(),
            schema: "genesis-method-selection/1".into(),
            id: "selected-parent".into(),
            version: "1".into(),
            sha256: digest::canonical_sha256(&selection),
        },
        text: digest::canonical_json(&selection),
    });
    let input = MethodRevisionInput::new(spec, offer.publication.package_json)
        .map_err(|_| anyhow::anyhow!("selected parent cannot form a bounded revision input"))?;
    Ok(serde_json::to_vec(&input)?)
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
            MethodCommand::RevisionInput { input, plan } => {
                revision_input(&self.registry_url, read(&input)?, read(&plan)?).await?
            }
        };
        // Serialize fully before touching stdout. A later pipe error still cannot undo a write.
        let mut stdout = std::io::stdout().lock();
        stdout.write_all(&bytes)?;
        stdout.write_all(b"\n")?;
        Ok(())
    }
}
