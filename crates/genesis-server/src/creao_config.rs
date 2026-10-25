//! Registers the CREAO specialist runtime from the backend environment.
//!
//! When all of `GENESIS_CREAO_DB` (this organization's operation journal),
//! `GENESIS_CREAO_ACCOUNT` (account namespace shared by every organization on
//! the same CREAO account), `GENESIS_CREAO_QUOTA_DB` (the shared quota
//! database), `CREAO_TRIGGER_API_KEY` and `CREAO_TRIGGER_APP_ID` are set, roles and
//! jobs go through the durable `CreaoSpecialist` (see ASYNC_PROVISIONING.md).
//! Startup recovery runs first and makes no provider requests.
//!
//! `GENESIS_PUBLIC_URL` additionally enables challenge-bound readiness runs and
//! the gateway verifier. Without it, app-bound roles keep awaiting readiness.
//! Developer API keys are never used as a fallback.

use crate::app::AppState;
use crate::creao_readiness::CreaoReadinessProbe;
use crate::gateway_readiness::{self, GatewayConfig};
use genesis_core::model::Provider;
use genesis_creao::{AccountLimiter, CreaoClient, Limits, QuotaLimits, Runtime, RuntimeStore};
use genesis_providers::CreaoSpecialist;
use std::sync::Arc;
use std::time::Duration;

const VARS: [&str; 5] = [
    "GENESIS_CREAO_DB",
    "GENESIS_CREAO_ACCOUNT",
    "GENESIS_CREAO_QUOTA_DB",
    "CREAO_TRIGGER_API_KEY",
    "CREAO_TRIGGER_APP_ID",
];

struct Configuration {
    db: String,
    account: String,
    quota_db: String,
    token: String,
    app_id: String,
    runs_url: Option<String>,
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
    values: [Option<String>; 5],
    public_url: Option<String>,
    version: Option<String>,
    runs_url: Option<String>,
) -> anyhow::Result<Option<Configuration>> {
    if values.iter().all(Option::is_none)
        && public_url.is_none()
        && version.is_none()
        && runs_url.is_none()
    {
        return Ok(None);
    }
    anyhow::ensure!(
        version
            .as_deref()
            .is_none_or(|v| v.trim().is_empty() || v == "trigger"),
        "CREAO_API_VERSION must be trigger; Developer API execution is disabled"
    );
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
    let [db, account, quota_db, token, app_id] = values.map(Option::unwrap_or_default);
    Ok(Some(Configuration {
        db,
        account,
        quota_db,
        token,
        app_id,
        runs_url,
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
        env_value(VARS[4])?,
    ];
    let Some(cfg) = configuration(
        values,
        env_value("GENESIS_PUBLIC_URL")?,
        env_value("CREAO_API_VERSION")?,
        env_value("CREAO_TRIGGER_RUNS_URL")?,
    )?
    else {
        return Ok(app);
    };
    let limiter = AccountLimiter::open(&cfg.quota_db, &cfg.account, QuotaLimits::default())?;
    let client = CreaoClient::new_trigger(
        &cfg.token,
        &cfg.app_id,
        cfg.runs_url.as_deref(),
        Limits::default(),
        limiter,
    )?;
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

    const APP: &str = "00000000-0000-4000-8000-000000000001";
    fn values() -> [Option<String>; 5] {
        [
            "private-runtime.db",
            "account",
            "private-quota.db",
            "capi_local_test",
            APP,
        ]
        .map(|value| Some(value.to_owned()))
    }

    #[test]
    fn gateway_configuration_is_explicit_complete_and_errors_hide_values() {
        assert!(
            configuration([None, None, None, None, None], None, None, None)
                .unwrap()
                .is_none()
        );
        assert!(configuration(values(), None, None, None)
            .unwrap()
            .unwrap()
            .gateway
            .is_none());
        let cfg = configuration(values(), Some("https://genesis.example".into()), None, None)
            .unwrap()
            .unwrap();
        assert_eq!(
            cfg.gateway.unwrap().receipt_url,
            "https://genesis.example/api/v1/gateway/readiness-receipts"
        );
        assert_eq!(cfg.app_id, APP);
        for missing in 0..5 {
            let mut partial = values();
            partial[missing] = None;
            let error = configuration(partial, Some("https://genesis.example".into()), None, None)
                .err()
                .unwrap()
                .to_string();
            assert!(error.contains(VARS[missing]));
            assert!(!error.contains("capi_local_test"));
        }
        assert!(configuration(
            [None, None, None, None, None],
            Some("https://genesis.example".into()),
            None,
            None
        )
        .is_err());
        for url in [
            "http://public.example",
            "https://user:PRIVATE_SECRET@genesis.example",
            "https://genesis.example/?PRIVATE_SECRET",
        ] {
            let error = configuration(values(), Some(url.into()), None, None)
                .err()
                .unwrap()
                .to_string();
            assert!(!error.contains("PRIVATE_SECRET"));
        }
        assert!(configuration(values(), None, Some("trigger".into()), None).is_ok());
        for version in ["v1", "v2", "v3", "SECRET_CANARY"] {
            let err = configuration(values(), None, Some(version.into()), None)
                .err()
                .unwrap()
                .to_string();
            assert!(!err.contains("SECRET_CANARY"));
        }
        assert!(configuration(
            [None, None, None, None, None],
            None,
            Some("trigger".into()),
            None
        )
        .is_err());
    }

    #[test]
    fn registration_keeps_default_inactive_and_only_adds_configured_verifier() {
        for enabled in [false, true] {
            let app = build_org(&OrgConfig::dev("org-a", "Org A"), None).unwrap();
            let runtime = Runtime::new(
                CreaoClient::for_local_testing_trigger(
                    "capi_local_test",
                    APP,
                    Limits::default(),
                    AccountLimiter::in_memory("test-account", QuotaLimits::default()).unwrap(),
                    "http://127.0.0.1:9/api/v1/apps/00000000-0000-4000-8000-000000000001/runs",
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
