//! Registers the CREAO specialist runtime from the backend environment.
//!
//! When all of `GENESIS_CREAO_DB` (this organization's operation journal),
//! `GENESIS_CREAO_ACCOUNT` (account namespace shared by every organization on
//! the same CREAO account), `GENESIS_CREAO_QUOTA_DB` (the shared quota
//! database) and `CREAO_ACCOUNT_API_KEY` are set, `provider: creao` roles and
//! jobs go through the durable `CreaoSpecialist` (see ASYNC_PROVISIONING.md).
//! Startup recovery runs first and makes no provider requests.
//!
//! `GENESIS_PUBLIC_URL` additionally enables challenge-bound readiness runs and
//! the gateway verifier. Without it, created agents keep awaiting readiness.

use crate::app::AppState;
use crate::creao_readiness::CreaoReadinessProbe;
use crate::gateway_readiness::{self, GatewayConfig};
use genesis_core::model::Provider;
use genesis_creao::{
    AccountLimiter, ApiVersion, CreaoClient, Limits, QuotaLimits, Runtime, RuntimeStore,
};
use genesis_providers::CreaoSpecialist;
use std::sync::Arc;
use std::time::Duration;

const VARS: [&str; 4] = [
    "GENESIS_CREAO_DB",
    "GENESIS_CREAO_ACCOUNT",
    "GENESIS_CREAO_QUOTA_DB",
    "CREAO_ACCOUNT_API_KEY",
];

struct Configuration {
    db: String,
    account: String,
    quota_db: String,
    token: String,
    api_version: ApiVersion,
    gateway: Option<GatewayConfig>,
}

fn env_value(name: &str) -> anyhow::Result<Option<String>> {
    match std::env::var(name) {
        Ok(value) => Ok((!value.trim().is_empty()).then_some(value)),
        Err(std::env::VarError::NotPresent) => Ok(None),
        // VarError itself can retain secret values; never format it.
        Err(std::env::VarError::NotUnicode(_)) => anyhow::bail!("{name} must be valid Unicode"),
    }
}

fn configuration(
    values: [Option<String>; 4],
    public_url: Option<String>,
    version: Option<String>,
) -> anyhow::Result<Option<Configuration>> {
    if values.iter().all(Option::is_none) && public_url.is_none() && version.is_none() {
        return Ok(None);
    }
    let api_version = match version.as_deref() {
        None | Some("v1") => ApiVersion::V1,
        Some("v2") => ApiVersion::V2,
        Some(_) => anyhow::bail!("CREAO_API_VERSION must be v1 or v2"),
    };
    let missing: Vec<&str> = VARS
        .iter()
        .zip(&values)
        .filter(|(_, value)| value.is_none())
        .map(|(name, _)| *name)
        .collect();
    anyhow::ensure!(
        missing.is_empty(),
        "partial CREAO configuration; missing {}",
        missing.join(", ")
    );
    let gateway = public_url
        .as_deref()
        .map(GatewayConfig::from_public_url)
        .transpose()?;
    let [db, account, quota_db, token] = values.map(Option::unwrap_or_default);
    Ok(Some(Configuration {
        db,
        account,
        quota_db,
        token,
        api_version,
        gateway,
    }))
}

/// Attach the CREAO runtime when fully configured; otherwise return `app`
/// unchanged. A partial configuration is an error rather than a silent skip.
pub async fn attach_creao_from_env(app: AppState) -> anyhow::Result<AppState> {
    let values = [
        env_value(VARS[0])?,
        env_value(VARS[1])?,
        env_value(VARS[2])?,
        env_value(VARS[3])?,
    ];
    let Some(cfg) = configuration(
        values,
        env_value("GENESIS_PUBLIC_URL")?,
        env_value("CREAO_API_VERSION")?,
    )?
    else {
        return Ok(app);
    };
    let limiter = AccountLimiter::open(&cfg.quota_db, &cfg.account, QuotaLimits::default())?;
    let client =
        CreaoClient::new_for_version(&cfg.token, Limits::default(), limiter, cfg.api_version)?;
    let runtime = Runtime::new(
        client,
        RuntimeStore::open(&cfg.db, app.org_id.as_str())?,
        Duration::from_secs(5),
    )?;
    let recovered = runtime
        .recover_interrupted()
        .await
        .map_err(|e| anyhow::anyhow!("CREAO startup recovery failed: {e}"))?;
    let app = attach_recovered(app, runtime, cfg.gateway)?;
    tracing::info!(org = %app.org_id, recovered, gateway_readiness = app.readiness.get(Provider::Creao).is_some(), "CREAO runtime registered");
    Ok(app)
}

/// Register only after startup recovery; both adapters share one runtime gate.
fn attach_recovered(
    mut app: AppState,
    runtime: Runtime,
    gateway: Option<GatewayConfig>,
) -> anyhow::Result<AppState> {
    anyhow::ensure!(
        runtime.org_id() == app.org_id.as_str(),
        "CREAO runtime organization mismatch"
    );
    app.providers = app
        .providers
        .clone()
        .with_specialist(Arc::new(CreaoSpecialist::new(runtime.clone())));
    if let Some(cfg) = gateway {
        let probe = CreaoReadinessProbe::new(runtime, cfg.receipt_url.clone())?;
        app = gateway_readiness::attach(app, Provider::Creao, Arc::new(probe), cfg)?;
    }
    Ok(app)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{build_org, OrgConfig};

    fn values() -> [Option<String>; 4] {
        [
            "private-runtime.db",
            "account",
            "private-quota.db",
            "cr_sk_local_test",
        ]
        .map(|value| Some(value.to_owned()))
    }

    #[test]
    fn gateway_configuration_is_explicit_complete_and_errors_hide_values() {
        assert!(configuration([None, None, None, None], None, None)
            .unwrap()
            .is_none());
        assert!(configuration(values(), None, None)
            .unwrap()
            .unwrap()
            .gateway
            .is_none());
        let cfg = configuration(values(), Some("https://genesis.example".into()), None)
            .unwrap()
            .unwrap();
        assert_eq!(
            cfg.gateway.unwrap().receipt_url,
            "https://genesis.example/api/v1/gateway/readiness-receipts"
        );
        for missing in 0..4 {
            let mut partial = values();
            partial[missing] = None;
            let error = configuration(partial, Some("https://genesis.example".into()), None)
                .err()
                .unwrap()
                .to_string();
            assert!(error.contains(VARS[missing]));
            assert!(!error.contains("cr_sk_local_test"));
        }
        assert!(configuration(
            [None, None, None, None],
            Some("https://genesis.example".into()),
            None,
        )
        .is_err());
        for url in [
            "http://public.example",
            "https://user:PRIVATE_SECRET@genesis.example",
            "https://genesis.example/?PRIVATE_SECRET",
        ] {
            let error = configuration(values(), Some(url.into()), None)
                .err()
                .unwrap()
                .to_string();
            assert!(!error.contains("PRIVATE_SECRET"));
        }
        assert_eq!(
            configuration(values(), None, Some("v2".into()))
                .unwrap()
                .unwrap()
                .api_version,
            ApiVersion::V2
        );
        assert!(configuration(values(), None, Some("v3".into())).is_err());
        assert!(configuration([None, None, None, None], None, Some("v2".into())).is_err());
    }

    #[test]
    fn registration_keeps_default_inactive_and_only_adds_configured_verifier() {
        for enabled in [false, true] {
            let app = build_org(&OrgConfig::dev("org-a", "Org A"), None).unwrap();
            let runtime = Runtime::new(
                CreaoClient::for_local_testing(
                    "cr_sk_local_test",
                    Limits::default(),
                    AccountLimiter::in_memory("test-account", QuotaLimits::default()).unwrap(),
                    "http://127.0.0.1:9/v1",
                )
                .unwrap(),
                RuntimeStore::in_memory("org-a").unwrap(),
                Duration::from_secs(5),
            )
            .unwrap();
            let gateway =
                enabled.then(|| GatewayConfig::from_public_url("https://genesis.example").unwrap());
            let app = attach_recovered(app, runtime, gateway).unwrap();
            assert_eq!(app.readiness.get(Provider::Creao).is_some(), enabled);
            assert!(app.providers.specialist(Provider::Creao).is_ok());
        }
    }
}
