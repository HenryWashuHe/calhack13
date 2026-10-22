use anyhow::Context;
use clap::{Parser, Subcommand};
use genesis_apify::{ApifyClient, RunLimits};
use genesis_core::ids::OrgId;
use genesis_core::model::Constitution;
use genesis_providers::{ApifyCapabilityExecutor, DevStub, ProviderSet};
use genesis_server::auth::Tokens;
use genesis_server::registry::RegistryState;
use genesis_server::{build_org, parse_publishers, spawn_org, spawn_registry, OrgConfig};
use serde::de::DeserializeOwned;
use std::fs::File;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

#[derive(Parser)]
#[command(name = "genesis", about = "GENESIS organization kernel")]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Serve one organization instance. Set APIFY_TOKEN to enable real Apify execution.
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
        /// Trusted local oracle suite JSON. Repeat for multiple suites; never fetched over HTTP.
        #[arg(long, value_name = "PATH")]
        oracle_suite: Vec<PathBuf>,
        /// Trusted local forge checkpoint JSON. Repeat for multiple verified capabilities.
        #[arg(long, value_name = "PATH")]
        forge_checkpoint: Vec<PathBuf>,
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

const MAX_CONFIG_BYTES: u64 = 8 * 1024 * 1024;

/// Startup files are backend-trusted context, not claims accepted through an API.
/// Bound reads themselves, then reject duplicate keys and malformed typed JSON.
fn load_trusted_json<T: DeserializeOwned>(path: &Path) -> anyhow::Result<T> {
    let file = File::open(path).with_context(|| format!("opening {}", path.display()))?;
    let metadata = file
        .metadata()
        .with_context(|| format!("reading metadata for {}", path.display()))?;
    anyhow::ensure!(
        metadata.is_file(),
        "{} must be a regular file",
        path.display()
    );
    anyhow::ensure!(
        metadata.len() <= MAX_CONFIG_BYTES,
        "{} exceeds 8 MiB",
        path.display()
    );
    let mut bytes = Vec::new();
    file.take(MAX_CONFIG_BYTES + 1)
        .read_to_end(&mut bytes)
        .with_context(|| format!("reading {}", path.display()))?;
    anyhow::ensure!(
        bytes.len() as u64 <= MAX_CONFIG_BYTES,
        "{} exceeds 8 MiB",
        path.display()
    );
    genesis_verification::decode_json(&bytes)
        .with_context(|| format!("decoding {}", path.display()))
}

fn apify_token_from_env() -> anyhow::Result<Option<String>> {
    match std::env::var("APIFY_TOKEN") {
        Ok(token) => Ok(Some(token)),
        Err(std::env::VarError::NotPresent) => Ok(None),
        // VarError may retain the value; never include it in an error chain.
        Err(std::env::VarError::NotUnicode(_)) => {
            anyhow::bail!("APIFY_TOKEN must be valid Unicode")
        }
    }
}

fn org_providers(
    dev_stub: bool,
    delay: Duration,
    apify_token: Option<&str>,
) -> anyhow::Result<ProviderSet> {
    let mut providers = ProviderSet::new();
    if dev_stub {
        providers = providers.with_dev_stub(DevStub::with_delay(delay));
    }
    if let Some(token) = apify_token {
        // Production origin is fixed inside ApifyClient::new. Tokens never
        // enter CLI arguments, configuration Debug output, or tracing fields.
        let client = ApifyClient::new(token, RunLimits::default())
            .context("configuring the Apify capability executor")?;
        providers = providers.with_executor(Arc::new(ApifyCapabilityExecutor::new(client)));
    }
    Ok(providers)
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
            oracle_suite,
            forge_checkpoint,
            registry_url,
            registry_token,
            operator_token,
            agent_token,
            adapter_token,
            dev_stub,
        } => {
            let constitution = match constitution {
                Some(path) => load_trusted_json::<Constitution>(&path)?,
                None => genesis_server::demo_constitution(),
            };
            let oracle_suites = oracle_suite
                .iter()
                .map(|path| load_trusted_json(path))
                .collect::<anyhow::Result<Vec<_>>>()?;
            let forge_checkpoints = forge_checkpoint
                .iter()
                .map(|path| load_trusted_json(path))
                .collect::<anyhow::Result<Vec<_>>>()?;
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
                oracle_suites,
                forge_checkpoints,
            };
            if dev_stub {
                tracing::warn!(
                    "development stub enabled: provider bindings will be labeled devstub-*"
                );
            }
            let token = apify_token_from_env()?;
            let providers = org_providers(dev_stub, cfg.stub_delay, token.as_deref())?;
            let app = build_org(&cfg, Some(providers))?;
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

#[cfg(test)]
mod cli_tests {
    use super::*;
    use genesis_core::model::Provider;
    use genesis_verification::OracleSuite;
    use serde_json::Value;
    use std::io::Write;

    #[test]
    fn trusted_input_flags_are_repeatable_and_org_only() {
        let cli = Cli::try_parse_from([
            "genesis",
            "org",
            "--org-id",
            "org_a",
            "--name",
            "Org A",
            "--oracle-suite",
            "suite-a.json",
            "--oracle-suite",
            "suite-b.json",
            "--forge-checkpoint",
            "forge-a.json",
            "--forge-checkpoint",
            "forge-b.json",
        ])
        .unwrap();
        let Command::Org {
            oracle_suite,
            forge_checkpoint,
            ..
        } = cli.command
        else {
            panic!("expected org command")
        };
        assert_eq!(
            oracle_suite,
            vec![PathBuf::from("suite-a.json"), PathBuf::from("suite-b.json")]
        );
        assert_eq!(
            forge_checkpoint,
            vec![PathBuf::from("forge-a.json"), PathBuf::from("forge-b.json")]
        );
        assert!(Cli::try_parse_from(["genesis", "dev", "--oracle-suite", "suite.json"]).is_err());
        // Provider credentials must not be exposed through process arguments.
        assert!(Cli::try_parse_from([
            "genesis",
            "org",
            "--org-id",
            "org_a",
            "--name",
            "Org A",
            "--apify-token",
            "not-a-real-token"
        ])
        .is_err());
    }

    #[test]
    fn trusted_suite_file_decodes_with_exact_typed_content() {
        let mut file = tempfile::NamedTempFile::new().unwrap();
        file.write_all(include_bytes!(
            "../../genesis-verification/examples/fixtures/trusted/suite.json"
        ))
        .unwrap();
        let suite: OracleSuite = load_trusted_json(file.path()).unwrap();
        genesis_verification::validate_suite(&suite).unwrap();
        assert!(suite.cases.len() >= 3);
    }

    #[test]
    fn startup_files_reject_oversize_ambiguity_and_non_files() {
        let large = tempfile::NamedTempFile::new().unwrap();
        large.as_file().set_len(MAX_CONFIG_BYTES + 1).unwrap();
        let error = load_trusted_json::<Value>(large.path()).unwrap_err();
        assert!(error.to_string().contains("exceeds 8 MiB"));

        let mut ambiguous = tempfile::NamedTempFile::new().unwrap();
        ambiguous
            .write_all(br#"{"private-source-value":1,"private-source-value":2}"#)
            .unwrap();
        let error = load_trusted_json::<Value>(ambiguous.path()).unwrap_err();
        assert!(!format!("{error:#}").contains("private-source-value"));

        let directory = tempfile::tempdir().unwrap();
        assert!(load_trusted_json::<Value>(directory.path()).is_err());
    }

    #[test]
    fn real_executor_and_explicit_role_stubs_are_independently_configured() {
        let providers =
            org_providers(true, Duration::ZERO, Some("synthetic-cli-token-never-live")).unwrap();
        assert_eq!(providers.executor().unwrap().provider(), Provider::Apify);
        assert_eq!(
            providers.provisioner(Provider::Elastic).unwrap().provider(),
            Provider::DevStub
        );
        assert_eq!(providers.configured(), vec![Provider::Apify]);

        let providers = org_providers(
            false,
            Duration::ZERO,
            Some("synthetic-cli-token-never-live"),
        )
        .unwrap();
        assert_eq!(providers.executor().unwrap().provider(), Provider::Apify);
        assert!(providers.provisioner(Provider::Elastic).is_err());

        let providers = org_providers(false, Duration::ZERO, None).unwrap();
        assert!(providers.executor().is_err());
        let providers = org_providers(true, Duration::ZERO, None).unwrap();
        assert_eq!(providers.executor().unwrap().provider(), Provider::DevStub);
    }

    #[test]
    fn invalid_apify_token_is_never_in_configuration_error() {
        let secret = "private-cli-token\ninvalid";
        let error = org_providers(false, Duration::ZERO, Some(secret))
            .err()
            .expect("invalid token must fail");
        assert!(!format!("{error:#}").contains("private-cli-token"));
    }
}
