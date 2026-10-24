use anyhow::Context;
use clap::{Args, Parser, Subcommand};
use genesis_apify::{ApifyClient, RunLimits};
use genesis_core::ids::OrgId;
use genesis_core::model::Constitution;
use genesis_providers::{ApifyCapabilityExecutor, DevStub, ProviderSet};
use genesis_server::auth::Tokens;
use genesis_server::capability_acquisition_store::AcquisitionPolicy;
use genesis_server::capability_gap_policy::GapAcquisitionPolicy;
use genesis_server::capability_scenario::{
    ScenarioDriver, ScenarioEndpoint, ScenarioLimits, ScenarioSpec,
};
use genesis_server::registry::RegistryState;
use genesis_server::{
    build_org, parse_publishers, spawn_org, spawn_registry, AcquisitionConfig, OrgConfig,
};
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
    Org(Box<OrgArgs>),
    /// Demonstrate acquisition and independent inheritance across two fresh organizations.
    /// Operator credentials are read only from GENESIS_ORG_A_OPERATOR_TOKEN and
    /// GENESIS_ORG_B_OPERATOR_TOKEN; provider credentials remain on each server.
    Scenario {
        #[arg(long, value_name = "PATH")]
        spec: PathBuf,
        #[arg(long, value_name = "URL")]
        org_a: String,
        #[arg(long, value_name = "URL")]
        org_b: String,
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

#[derive(Args)]
struct OrgArgs {
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
    /// Trusted JSON model/count-quota policy enabling capability acquisition.
    /// Provider keys come only from ANTHROPIC_API_KEY and APIFY_TOKEN.
    #[arg(long, value_name = "PATH")]
    acquisition_policy: Option<PathBuf>,
    /// Automatically advance existing and new explicitly approved acquisition intents.
    /// This does not create acquisition intents from requirements.
    #[arg(long, requires = "acquisition_policy")]
    auto_acquire: bool,
    /// Trusted JSON rules permitting committed capability gaps to create acquisition intents.
    /// Contains fixed public examples; requires automatic acquisition and its quota policy.
    #[arg(long, value_name = "FILE", requires_all = ["auto_acquire", "acquisition_policy"])]
    gap_acquisition_policy: Option<PathBuf>,
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
const MAX_GAP_POLICY_BYTES: u64 = 4 * 1024 * 1024;

/// Startup files are backend-trusted context, not claims accepted through an API.
/// Bound reads themselves, then reject duplicate keys and malformed typed JSON.
fn load_trusted_json<T: DeserializeOwned>(path: &Path) -> anyhow::Result<T> {
    load_bounded_trusted_json(path, MAX_CONFIG_BYTES)
}

fn load_gap_policy(path: &Path) -> anyhow::Result<GapAcquisitionPolicy> {
    let policy: GapAcquisitionPolicy = load_bounded_trusted_json(path, MAX_GAP_POLICY_BYTES)?;
    policy.validate()?;
    Ok(policy)
}

fn load_bounded_trusted_json<T: DeserializeOwned>(
    path: &Path,
    max_bytes: u64,
) -> anyhow::Result<T> {
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
        metadata.len() <= max_bytes,
        "{} exceeds {} MiB",
        path.display(),
        max_bytes / (1024 * 1024)
    );
    let mut bytes = Vec::new();
    file.take(max_bytes + 1)
        .read_to_end(&mut bytes)
        .with_context(|| format!("reading {}", path.display()))?;
    anyhow::ensure!(
        bytes.len() as u64 <= max_bytes,
        "{} exceeds {} MiB",
        path.display(),
        max_bytes / (1024 * 1024)
    );
    genesis_verification::decode_json(&bytes)
        .with_context(|| format!("decoding {}", path.display()))
}

fn apify_token_from_env() -> anyhow::Result<Option<String>> {
    secret_from_env("APIFY_TOKEN")
}

fn secret_from_env(name: &str) -> anyhow::Result<Option<String>> {
    match std::env::var(name) {
        Ok(token) => Ok(Some(token)),
        Err(std::env::VarError::NotPresent) => Ok(None),
        // VarError may retain the value; never include it in an error chain.
        Err(std::env::VarError::NotUnicode(_)) => {
            anyhow::bail!("{name} must be valid Unicode")
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
        Command::Scenario { spec, org_a, org_b } => {
            let spec: ScenarioSpec = load_trusted_json(&spec)?;
            let token_a = secret_from_env("GENESIS_ORG_A_OPERATOR_TOKEN")?
                .context("GENESIS_ORG_A_OPERATOR_TOKEN is required")?;
            let token_b = secret_from_env("GENESIS_ORG_B_OPERATOR_TOKEN")?
                .context("GENESIS_ORG_B_OPERATOR_TOKEN is required")?;
            let driver = ScenarioDriver::new(
                ScenarioEndpoint::new(&org_a, &token_a)?,
                ScenarioEndpoint::new(&org_b, &token_b)?,
                ScenarioLimits::default(),
            )?;
            let report = driver.run(&spec).await?;
            println!("{}", serde_json::to_string_pretty(&report)?);
            Ok(())
        }
        Command::Org(args) => {
            let OrgArgs {
                org_id,
                name,
                db,
                bind,
                constitution,
                oracle_suite,
                forge_checkpoint,
                acquisition_policy,
                auto_acquire,
                gap_acquisition_policy,
                registry_url,
                registry_token,
                operator_token,
                agent_token,
                adapter_token,
                dev_stub,
            } = *args;
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
            let acquisition = acquisition_policy
                .as_ref()
                .map(|path| load_trusted_json::<AcquisitionPolicy>(path))
                .transpose()?
                .map(AcquisitionConfig::offline);
            let gap_acquisition_policy = gap_acquisition_policy
                .as_deref()
                .map(load_gap_policy)
                .transpose()?;
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
                acquisition,
                gap_acquisition_policy,
            };
            if dev_stub {
                tracing::warn!(
                    "development stub enabled: provider bindings will be labeled devstub-*"
                );
            }
            let mut cfg = cfg;
            let token = apify_token_from_env()?;
            let providers = org_providers(dev_stub, cfg.stub_delay, token.as_deref())?;
            if let Some(config) = &mut cfg.acquisition {
                config.model = secret_from_env("ANTHROPIC_API_KEY")?
                    .map(|key| genesis_forge::AnthropicGenerator::new(&key).map(Arc::new))
                    .transpose()
                    .context("configuring the acquisition model client")?;
                config.apify = token
                    .as_deref()
                    .map(|key| ApifyClient::new(key, RunLimits::default()).map(Arc::new))
                    .transpose()
                    .context("configuring the acquisition Apify client")?;
                config.automatic = auto_acquire;
            }
            let app = genesis_server::attach_elastic_from_env(build_org(&cfg, Some(providers))?)?;
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
            let state = RegistryState::open(db, publishers, readers)?.with_elastic_from_env()?;
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
            let reg = RegistryState::open(data_dir.join("registry.db"), publishers, vec![])?
                .with_elastic_from_env()?;
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
                let app = genesis_server::attach_elastic_from_env(build_org(&cfg, None)?)?;
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
    fn scenario_cli_requires_explicit_endpoints_and_keeps_credentials_out_of_arguments() {
        let args = [
            "genesis",
            "scenario",
            "--spec",
            "scenario.json",
            "--org-a",
            "http://127.0.0.1:7101",
            "--org-b",
            "http://127.0.0.1:7102",
        ];
        assert!(matches!(Cli::try_parse_from(args).unwrap().command,
            Command::Scenario { spec, org_a, org_b }
                if spec == Path::new("scenario.json")
                    && org_a == "http://127.0.0.1:7101"
                    && org_b == "http://127.0.0.1:7102"));
        assert!(Cli::try_parse_from(&args[..6]).is_err());
        for forbidden in [
            "--operator-token",
            "--org-a-operator-token",
            "--apify-token",
            "--anthropic-api-key",
        ] {
            let mut with_secret = args.to_vec();
            with_secret.extend([forbidden, "synthetic-cli-secret"]);
            assert!(Cli::try_parse_from(with_secret).is_err());
        }
    }

    #[test]
    fn example_scenario_is_strict_and_requires_explicit_provider_access_configuration() {
        let mut file = tempfile::NamedTempFile::new().unwrap();
        file.write_all(include_bytes!("../examples/scenario/mission.json"))
            .unwrap();
        let spec: ScenarioSpec = load_trusted_json(file.path()).unwrap();
        assert_ne!(spec.suite_a_id, spec.suite_b_id);
        assert!(spec.allow_development_roles);
        assert!(!spec.provider_access_acknowledged);
        assert_eq!(spec.invocation_input["attendees"], 150);

        let mut with_oracle = serde_json::to_value(spec).unwrap();
        with_oracle["expected"] = serde_json::json!({"forbidden":"oracle"});
        assert!(serde_json::from_value::<ScenarioSpec>(with_oracle).is_err());
    }

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
        let Command::Org(args) = cli.command else {
            panic!("expected org command")
        };
        let OrgArgs {
            oracle_suite,
            forge_checkpoint,
            ..
        } = *args;
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

    #[test]
    fn acquisition_cli_accepts_only_a_policy_path_not_provider_credentials() {
        let args = [
            "genesis",
            "org",
            "--org-id",
            "test-org",
            "--name",
            "Test",
            "--acquisition-policy",
            "policy.json",
        ];
        let cli = Cli::try_parse_from(args).unwrap();
        assert!(matches!(cli.command, Command::Org(args)
            if args.acquisition_policy.as_deref() == Some(Path::new("policy.json")) && !args.auto_acquire));
        for automatic in [false, true] {
            for forbidden in ["--anthropic-api-key", "--model-endpoint", "--apify-token"] {
                let mut args = args.to_vec();
                if automatic {
                    args.push("--auto-acquire");
                }
                args.extend([forbidden, "synthetic-never-live"]);
                assert!(Cli::try_parse_from(args).is_err());
            }
        }
    }

    #[test]
    fn automatic_acquisition_is_explicit_and_requires_a_policy() {
        let base = ["genesis", "org", "--org-id", "test-org", "--name", "Test"];
        assert!(
            matches!(Cli::try_parse_from(base).unwrap().command, Command::Org(args)
            if !args.auto_acquire && args.acquisition_policy.is_none())
        );
        let mut missing_policy = base.to_vec();
        missing_policy.push("--auto-acquire");
        assert!(Cli::try_parse_from(missing_policy.clone()).is_err());
        missing_policy.extend(["--acquisition-policy", "policy.json"]);
        assert!(
            matches!(Cli::try_parse_from(missing_policy).unwrap().command, Command::Org(args)
            if args.auto_acquire && args.acquisition_policy.as_deref() == Some(Path::new("policy.json")))
        );
        assert!(Cli::try_parse_from(["genesis", "dev", "--auto-acquire"]).is_err());
    }

    #[test]
    fn gap_acquisition_requires_both_explicit_acquisition_options() {
        let base = ["genesis", "org", "--org-id", "test-org", "--name", "Test"];
        assert!(
            matches!(Cli::try_parse_from(base).unwrap().command, Command::Org(args)
            if args.gap_acquisition_policy.is_none())
        );
        for extra in [
            vec![],
            vec!["--auto-acquire"],
            vec!["--acquisition-policy", "acquisition.json"],
        ] {
            let mut args = base.to_vec();
            args.extend(["--gap-acquisition-policy", "gaps.json"]);
            args.extend(extra);
            assert!(Cli::try_parse_from(args).is_err());
        }
        let mut args = base.to_vec();
        args.extend([
            "--gap-acquisition-policy",
            "gaps.json",
            "--auto-acquire",
            "--acquisition-policy",
            "acquisition.json",
        ]);
        assert!(
            matches!(Cli::try_parse_from(args).unwrap().command, Command::Org(args)
            if args.auto_acquire && args.gap_acquisition_policy.as_deref() == Some(Path::new("gaps.json")))
        );
        assert!(
            Cli::try_parse_from(["genesis", "dev", "--gap-acquisition-policy", "gaps.json"])
                .is_err()
        );
    }

    fn gap_policy_value() -> Value {
        serde_json::json!({"rules": [{
            "capability_id": "venue-reader", "suite_id": "trusted-suite",
            "actor_name_prefix": "genesis-venue",
            "allowed_source_urls": ["https://venues.example/a"],
            "examples": [{"url":"https://venues.example/a", "html":"<html>private-debug-marker</html>"}],
            "repair": true,
        }]})
    }

    #[test]
    fn gap_policy_loader_bounds_files_and_sanitizes_schema_errors() {
        let mut file = tempfile::NamedTempFile::new().unwrap();
        file.write_all(&serde_json::to_vec(&gap_policy_value()).unwrap())
            .unwrap();
        assert_eq!(load_gap_policy(file.path()).unwrap().rules.len(), 1);
        let large = tempfile::NamedTempFile::new().unwrap();
        large.as_file().set_len(MAX_GAP_POLICY_BYTES + 1).unwrap();
        assert!(load_gap_policy(large.path())
            .err()
            .unwrap()
            .to_string()
            .contains("exceeds 4 MiB"));
        assert!(load_gap_policy(tempfile::tempdir().unwrap().path()).is_err());

        let mut unknown = gap_policy_value();
        unknown["private-field-marker"] = serde_json::json!("private-value-marker");
        let mut invalid_type = gap_policy_value();
        invalid_type["rules"][0]["repair"] = serde_json::json!("private-value-marker");
        let mut invalid_policy = gap_policy_value();
        invalid_policy["rules"][0]["actor_name_prefix"] =
            serde_json::json!("private-value-marker!");
        for value in [unknown, invalid_type, invalid_policy] {
            let mut file = tempfile::NamedTempFile::new().unwrap();
            file.write_all(&serde_json::to_vec(&value).unwrap())
                .unwrap();
            let error = format!("{:#}", load_gap_policy(file.path()).err().unwrap());
            assert!(!error.contains("private-field-marker"));
            assert!(!error.contains("private-value-marker"));
            assert!(!error.contains("private-debug-marker"));
        }
        let mut duplicate = tempfile::NamedTempFile::new().unwrap();
        duplicate
            .write_all(
                br#"{"rules": [], "rules": [], "private-field-marker": "private-value-marker"}"#,
            )
            .unwrap();
        let error = format!("{:#}", load_gap_policy(duplicate.path()).err().unwrap());
        assert!(!error.contains("private-"));
    }

    #[test]
    fn gap_configuration_requires_automatic_clients_before_opening_the_store() {
        let mut config = OrgConfig::dev("gap-startup", "Gap startup");
        config.gap_acquisition_policy = Some(serde_json::from_value(gap_policy_value()).unwrap());
        let directory = tempfile::tempdir().unwrap();
        let db = directory.path().join("should-not-exist.sqlite");
        config.db = Some(db.clone());
        let valid_policy = config.gap_acquisition_policy.take().unwrap();
        config.gap_acquisition_policy = Some(GapAcquisitionPolicy { rules: vec![] });
        assert!(build_org(&config, None)
            .err()
            .unwrap()
            .to_string()
            .contains("invalid trusted gap acquisition policy"));
        assert!(!db.exists());
        config.gap_acquisition_policy = Some(valid_policy);
        assert!(build_org(&config, None)
            .err()
            .unwrap()
            .to_string()
            .contains("requires automatic acquisition"));
        assert!(!db.exists());
        config.acquisition = Some(AcquisitionConfig::offline(AcquisitionPolicy {
            model: genesis_forge::ModelConfig {
                model: "test-model".into(),
                max_tokens: 1024,
            },
            quotas: Default::default(),
            max_duration_seconds: 3600,
        }));
        assert!(build_org(&config, None)
            .err()
            .unwrap()
            .to_string()
            .contains("requires automatic acquisition"));
        config.acquisition.as_mut().unwrap().automatic = true;
        assert!(build_org(&config, None)
            .err()
            .unwrap()
            .to_string()
            .contains("requires configured model and Apify clients"));
        assert!(!db.exists());
        let acquisition = config.acquisition.as_mut().unwrap();
        acquisition.model = Some(Arc::new(
            genesis_forge::AnthropicGenerator::new("synthetic-no-network-model").unwrap(),
        ));
        acquisition.apify = Some(Arc::new(
            ApifyClient::new("synthetic-no-network-apify", RunLimits::default()).unwrap(),
        ));
        let app = build_org(&config, None).unwrap();
        assert!(app.automatic_acquisition);
        assert!(app.gap_acquisition_policy.is_some());
        assert!(db.exists());
    }

    #[test]
    fn org_config_debug_redacts_gap_examples_and_default_stays_disabled() {
        let mut config = OrgConfig::dev("gap-debug", "Gap debug");
        assert!(config.gap_acquisition_policy.is_none());
        assert!(build_org(&config, None)
            .unwrap()
            .gap_acquisition_policy
            .is_none());
        config.gap_acquisition_policy = Some(serde_json::from_value(gap_policy_value()).unwrap());
        let debug = format!("{config:?}");
        assert!(debug.contains("gap_acquisition_configured: true"));
        assert!(!debug.contains("private-debug-marker"));
        assert!(!debug.contains("<html>"));
        assert!(!debug.contains("https://venues.example/a"));
    }

    #[test]
    fn acquisition_config_debug_never_exposes_model_credentials() {
        use genesis_forge::{AnthropicGenerator, ModelConfig};
        use genesis_server::capability_acquisition_store::AcquisitionQuotas;
        let config = AcquisitionConfig {
            automatic: true,
            policy: AcquisitionPolicy {
                model: ModelConfig {
                    model: "concrete-test-model".into(),
                    max_tokens: 1024,
                },
                quotas: AcquisitionQuotas::default(),
                max_duration_seconds: 3600,
            },
            model: Some(Arc::new(
                AnthropicGenerator::new("synthetic-model-secret").unwrap(),
            )),
            apify: Some(Arc::new(
                ApifyClient::new("synthetic-apify-secret", RunLimits::default()).unwrap(),
            )),
        };
        let debug = format!("{config:?}");
        assert!(debug.contains("automatic: true"));
        assert!(debug.contains("model_configured: true"));
        assert!(debug.contains("apify_configured: true"));
        assert!(!debug.contains("synthetic-model-secret"));
        assert!(!debug.contains("synthetic-apify-secret"));
        let offline = AcquisitionConfig::offline(config.policy.clone());
        assert!(!offline.automatic);
        assert!(format!("{offline:?}").contains("automatic: false"));
    }
}
