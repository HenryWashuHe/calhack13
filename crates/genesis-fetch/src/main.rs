//! Trusted backend proof tool. It is not a public kernel authorization endpoint.
use anyhow::{Context, Result};
use clap::{Args, Parser, Subcommand};
use genesis_fetch::{
    client::{BridgeClient, ClientLimits},
    discovery::{DiscoveryClient, SearchRequest},
    protocol::{Message, MAX_MESSAGE_BYTES, PROTOCOL_DIGEST},
    runtime::{Contract, RuntimeStore},
};
use serde_json::{json, Value};
use std::{io::Read, path::PathBuf};

#[derive(Parser)]
#[command(
    name = "genesis-fetch",
    about = "Discover and exchange durable Fetch specialist evidence; no kernel authority is granted"
)]
struct Cli {
    #[command(subcommand)]
    command: Command,
}
#[derive(Args)]
struct Local {
    #[arg(long)]
    db: PathBuf,
    #[arg(long)]
    org: String,
    #[arg(long)]
    bridge_address: String,
}
impl Local {
    fn store(&self) -> Result<RuntimeStore> {
        Ok(RuntimeStore::open(
            &self.db,
            &self.org,
            &self.bridge_address,
        )?)
    }
}
#[derive(Subcommand)]
enum Command {
    /// Search advertised native protocol compatibility. Results are candidates only.
    Discover {
        #[arg(long)]
        query: String,
        #[arg(long, default_value_t = 10)]
        limit: u32,
        #[arg(long, default_value_t = 0)]
        offset: u64,
    },
    /// Persist a trusted outgoing intent and local contract transition, offline.
    Prepare {
        #[command(flatten)]
        local: Local,
        #[arg(long)]
        peer: String,
        #[arg(long)]
        message: PathBuf,
    },
    /// Summarize native evidence without exposing task input/output or terms.
    Status {
        #[command(flatten)]
        local: Local,
        #[arg(long)]
        contract: String,
    },
    /// Ingest one private bridge page and submit currently eligible intents.
    /// Exact queue retries are idempotent; transport does not accept a task.
    Exchange {
        #[command(flatten)]
        local: Local,
        #[arg(long)]
        bridge_url: String,
    },
}
fn read_message(path: &PathBuf) -> Result<Message> {
    let file = std::fs::File::open(path).context("open trusted message file")?;
    let meta = file.metadata()?;
    anyhow::ensure!(
        meta.is_file() && meta.len() <= MAX_MESSAGE_BYTES as u64,
        "message must be a regular file within 64 KiB"
    );
    let mut bytes = Vec::new();
    file.take(MAX_MESSAGE_BYTES as u64 + 1)
        .read_to_end(&mut bytes)?;
    Ok(Message::decode(&bytes)?)
}
fn summary(c: &Contract) -> Value {
    json!({"contract_id":c.recruit.contract_id,"org_id":c.recruit.org_id,"peer":c.peer,
        "native_release_acknowledged":c.release_ack.is_some(),"phase":c.phase,"revision":c.revision,"lease_expires_at":c.lease(),
        "recruit_message_id":c.recruit.message_id,
        "offer_message_id":c.offer.as_ref().map(|m|&m.message_id),
        "offer_id":c.offer.as_ref().and_then(|m|if let genesis_fetch::protocol::Body::Offer{offer_id,..}=&m.body{Some(offer_id)}else{None}),
        "tasks":c.tasks.iter().map(|(id,t)|json!({"request_message_id":id,"phase":t.phase,"evidence_message_id":t.evidence.as_ref().map(|m|&m.message_id)})).collect::<Vec<_>>(),
        "kernel_role_active":false,"domain_verified":false})
}
#[tokio::main]
async fn main() -> Result<()> {
    let output = match Cli::parse().command {
        Command::Discover {
            query,
            limit,
            offset,
        } => {
            let token = std::env::var("AGENTVERSE_API_KEY")
                .context("AGENTVERSE_API_KEY must be set in the backend environment")?;
            let page = DiscoveryClient::new(&token, ClientLimits::default())?
                .search(&SearchRequest {
                    search_text: query,
                    protocol_digest: PROTOCOL_DIGEST.into(),
                    states: vec!["active".into()],
                    agent_types: vec!["uagent".into()],
                    offset,
                    limit,
                    search_id: None,
                })
                .await?;
            serde_json::to_value(page)?
        }
        Command::Prepare {
            local,
            peer,
            message,
        } => summary(&local.store()?.prepare(
            &peer,
            read_message(&message)?,
            chrono::Utc::now(),
        )?),
        Command::Status { local, contract } => summary(&local.store()?.get(&contract)?),
        Command::Exchange { local, bridge_url } => {
            let token = std::env::var("GENESIS_FETCH_BRIDGE_TOKEN")
                .context("GENESIS_FETCH_BRIDGE_TOKEN must be set in the backend environment")?;
            let client = BridgeClient::new(
                &bridge_url,
                &token,
                &local.bridge_address,
                PROTOCOL_DIGEST,
                ClientLimits::default(),
            )?;
            client.identity().await?;
            let store = local.store()?;
            let page = client.inbox(store.cursor()?, 50).await?;
            let received = store.ingest(&page, chrono::Utc::now())?;
            let mut queued = Vec::new();
            for request in store.pending(chrono::Utc::now())? {
                // A previous bridge await is an opportunity for another trusted
                // process to release the contract. Recheck immediately before I/O.
                if !store
                    .pending(chrono::Utc::now())?
                    .iter()
                    .any(|r| r.message.message_id == request.message.message_id)
                {
                    continue;
                }
                let receipt = client.send(&request).await?;
                store.acknowledge(&receipt)?;
                queued.push(request.message.message_id);
            }
            json!({"received":received,"queued_message_ids":queued,"cursor":store.cursor()?,"kernel_role_active":false,"domain_verified":false})
        }
    };
    println!("{}", serde_json::to_string_pretty(&output)?);
    Ok(())
}
