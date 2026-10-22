//! Registers the CREAO specialist runtime from the backend environment.
//!
//! When all of `GENESIS_CREAO_DB` (this organization's operation journal),
//! `GENESIS_CREAO_ACCOUNT` (account namespace shared by every organization on
//! the same CREAO account), `GENESIS_CREAO_QUOTA_DB` (the shared quota
//! database) and `CREAO_ACCOUNT_API_KEY` are set, `provider: creao` roles and
//! jobs go through the durable `CreaoSpecialist` (see ASYNC_PROVISIONING.md).
//! Startup recovery runs first and makes no provider requests.
//!
//! No CREAO readiness verifier is registered here: until one exists, created
//! CREAO agents wait in `awaiting_readiness` and their roles never activate.

use crate::app::AppState;
use genesis_creao::{AccountLimiter, CreaoClient, Limits, QuotaLimits, Runtime, RuntimeStore};
use genesis_providers::CreaoSpecialist;
use std::sync::Arc;
use std::time::Duration;

const VARS: [&str; 4] = [
    "GENESIS_CREAO_DB",
    "GENESIS_CREAO_ACCOUNT",
    "GENESIS_CREAO_QUOTA_DB",
    "CREAO_ACCOUNT_API_KEY",
];

/// Attach the CREAO runtime when fully configured; otherwise return `app`
/// unchanged. A partial configuration is an error rather than a silent skip.
pub async fn attach_creao_from_env(app: AppState) -> anyhow::Result<AppState> {
    let values: Vec<Option<String>> = VARS
        .iter()
        .map(|v| std::env::var(v).ok().filter(|s| !s.trim().is_empty()))
        .collect();
    if values.iter().all(Option::is_none) {
        return Ok(app);
    }
    let missing: Vec<&str> = VARS
        .iter()
        .zip(&values)
        .filter(|(_, v)| v.is_none())
        .map(|(k, _)| *k)
        .collect();
    anyhow::ensure!(
        missing.is_empty(),
        "partial CREAO configuration; missing {}",
        missing.join(", ")
    );
    let [db, account, quota_db, token] =
        [0, 1, 2, 3].map(|i| values[i].clone().unwrap_or_default());
    let limiter = AccountLimiter::open(&quota_db, &account, QuotaLimits::default())?;
    let client = CreaoClient::new(&token, Limits::default(), limiter)?;
    let runtime = Runtime::new(
        client,
        RuntimeStore::open(&db, app.org_id.as_str())?,
        Duration::from_secs(5),
    )?;
    let specialist = CreaoSpecialist::new(runtime);
    let recovered = specialist
        .recover_interrupted()
        .await
        .map_err(|e| anyhow::anyhow!("CREAO startup recovery failed: {e}"))?;
    tracing::info!(org = %app.org_id, recovered, "CREAO specialist runtime registered (no readiness verifier: CREAO roles stay inactive)");
    let mut app = app;
    app.providers = app.providers.clone().with_specialist(Arc::new(specialist));
    Ok(app)
}
