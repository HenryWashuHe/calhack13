use anyhow::Context;
use clap::{Parser, Subcommand};
use genesis_core::ids::OrgId;
use genesis_core::model::Constitution;
use genesis_server::auth::Tokens;
use genesis_server::registry::RegistryState;
use genesis_server::{build_org, parse_publishers, spawn_org, spawn_registry, OrgConfig};
use std::path::PathBuf;
use std::time::Duration;

#[derive(Parser)]
#[command(name = "genesis", about = "GENESIS organization kernel")]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Serve one organization instance.
    Org {
        #[arg(long, env = "GENESIS_ORG_ID")]
        org_id: String,
        #[arg(long, env = "GENESIS_ORG_NAME")]
        name: String,
        #[arg(long, env = "GENESIS_DB", default_value = "data/org.db")]
        db: PathBuf,
        #[arg(long, env = "GENESIS_BIND", default_value = "127.0.0.1:7101")]
        bind: String,
        /// JSON file with the organization constitution. Defaults to the demo constitution.
        #[arg(long, env = "GENESIS_CONSTITUTION")]
        constitution: Option<PathBuf>,
        #[arg(long, env = "GENESIS_REGISTRY_URL")]
        registry_url: Option<String>,
        #[arg(long, env = "GENESIS_REGISTRY_TOKEN")]
        registry_token: Option<String>,
        #[arg(long, env = "GENESIS_OPERATOR_TOKEN")]
        operator_token: Option<String>,
        #[arg(long, env = "GENESIS_AGENT_TOKEN")]
        agent_token: Option<String>,
        #[arg(long, env = "GENESIS_ADAPTER_TOKEN")]
        adapter_token: Option<String>,
        /// Use the labeled development stub for providers without an adapter.
        #[arg(long, env = "GENESIS_DEV_STUB", default_value_t = false)]
        dev_stub: bool,
    },
    /// Serve the shared capability registry.
    Registry {
        #[arg(long, env = "GENESIS_REGISTRY_DB", default_value = "data/registry.db")]
        db: PathBuf,
        #[arg(long, env = "GENESIS_REGISTRY_BIND", default_value = "127.0.0.1:7100")]
        bind: String,
        /// `org_a:token,org_b:token` publisher tokens.
        #[arg(long, env = "GENESIS_REGISTRY_PUBLISHERS")]
        publishers: String,
        /// Extra read-only tokens, comma separated.
        #[arg(long, env = "GENESIS_REGISTRY_READERS", default_value = "")]
        readers: String,
    },
    /// Run the registry, Org A and Org B locally with development stubs.
    Dev {
        #[arg(long, default_value = "data/dev")]
        data_dir: PathBuf,
        #[arg(long, default_value_t = 7100)]
        base_port: u16,
        /// Artificial provisioning latency so PROVISIONING is visible in the UI.
        #[arg(long, default_value_t = 1500)]
        stub_delay_ms: u64,
    },
    /// Write JSON Schemas for the shared contracts.
    ExportSchemas {
        #[arg(long, default_value = "schemas")]
        out: PathBuf,
    },
}

fn init_tracing() {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "info,tower_http=info".into()),
        )
        .init();
}

fn token_or_dev(value: Option<String>, org: &str, kind: &str, dev: bool) -> anyhow::Result<String> {
    match value {
        Some(v) if !v.is_empty() => Ok(v),
        _ if dev => {
            let t = format!("{org}-{kind}");
            tracing::warn!("{kind} token not set; using development token `{t}`");
            Ok(t)
        }
        _ => anyhow::bail!(
            "{kind} token is required (set GENESIS_{}_TOKEN)",
            kind.to_uppercase()
        ),
    }
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    init_tracing();
    match Cli::parse().command {
        Command::Org {
            org_id,
            name,
            db,
            bind,
            constitution,
            registry_url,
            registry_token,
            operator_token,
            agent_token,
            adapter_token,
            dev_stub,
        } => {
            let constitution = match constitution {
                Some(path) => {
                    let text = std::fs::read_to_string(&path)
                        .with_context(|| format!("reading {}", path.display()))?;
                    serde_json::from_str::<Constitution>(&text).context("parsing constitution")?
                }
                None => genesis_server::demo_constitution(),
            };
            let tokens = Tokens {
                operator: token_or_dev(operator_token, &org_id, "operator", dev_stub)?,
                agent: token_or_dev(agent_token, &org_id, "agent", dev_stub)?,
                adapter: token_or_dev(adapter_token, &org_id, "adapter", dev_stub)?,
            };
            let cfg = OrgConfig {
                org_id: OrgId::new(org_id),
                name,
                db: Some(db),
                constitution,
                tokens,
                registry_url,
                registry_token,
                dev_stub,
                stub_delay: Duration::from_millis(800),
                provision_timeout: Duration::from_secs(120),
            };
            if dev_stub {
                tracing::warn!(
                    "development stub enabled: provider bindings will be labeled devstub-*"
                );
            }
            let app = build_org(&cfg, None)?;
            let listener = tokio::net::TcpListener::bind(&bind).await?;
            tracing::info!(org = %cfg.org_id, %bind, "organization listening");
            genesis_server::serve_org(app, listener).await
        }
        Command::Registry {
            db,
            bind,
            publishers,
            readers,
        } => {
            let publishers = parse_publishers(&publishers);
            anyhow::ensure!(
                !publishers.is_empty(),
                "at least one publisher token is required"
            );
            let readers = readers
                .split(',')
                .map(str::trim)
                .filter(|s| !s.is_empty())
                .map(String::from)
                .collect();
            let state = RegistryState::open(db, publishers, readers)?;
            let addr = spawn_registry(state, &bind).await?;
            tracing::info!(%addr, "registry listening");
            tokio::signal::ctrl_c().await?;
            Ok(())
        }
        Command::Dev {
            data_dir,
            base_port,
            stub_delay_ms,
        } => {
            std::fs::create_dir_all(&data_dir)?;
            let reg_addr = format!("127.0.0.1:{base_port}");
            let publishers = parse_publishers("org_a:org_a-registry,org_b:org_b-registry");
            let reg = RegistryState::open(data_dir.join("registry.db"), publishers, vec![])?;
            spawn_registry(reg, &reg_addr).await?;
            let mut lines = vec![format!("registry  http://{reg_addr}")];
            for (i, (id, name)) in [
                ("org_a", "Org A · Campus Events"),
                ("org_b", "Org B · Conference Co"),
            ]
            .into_iter()
            .enumerate()
            {
                let mut cfg = OrgConfig::dev(id, name);
                cfg.db = Some(data_dir.join(format!("{id}.db")));
                cfg.registry_url = Some(format!("http://{reg_addr}"));
                cfg.registry_token = Some(format!("{id}-registry"));
                cfg.stub_delay = Duration::from_millis(stub_delay_ms);
                let addr = format!("127.0.0.1:{}", base_port + 1 + i as u16);
                let app = build_org(&cfg, None)?;
                spawn_org(app, &addr).await?;
                lines.push(format!(
                    "{id}     http://{addr}   operator token: {}",
                    cfg.tokens.operator
                ));
            }
            println!("\nGENESIS dev stack (development stubs enabled)\n");
            for l in lines {
                println!("  {l}");
            }
            println!("\nPress Ctrl+C to stop.\n");
            tokio::signal::ctrl_c().await?;
            Ok(())
        }
        Command::ExportSchemas { out } => {
            std::fs::create_dir_all(&out)?;
            for (name, schema) in genesis_core::schema::all() {
                let path = out.join(format!("{name}.schema.json"));
                std::fs::write(&path, serde_json::to_string_pretty(&schema)? + "\n")?;
                println!("wrote {}", path.display());
            }
            Ok(())
        }
    }
}
