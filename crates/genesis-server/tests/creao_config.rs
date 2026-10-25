//! CREAO runtime registration from the environment. One test per binary
//! because it mutates process environment variables.

use genesis_server::{build_org, creao_config::attach_creao_from_env, OrgConfig};

#[tokio::test]
async fn creao_registration_requires_complete_configuration() {
    let dir = tempfile::tempdir().unwrap();
    let cfg = OrgConfig::dev("org_a", "Org A");

    // Nothing set: unchanged.
    for v in [
        "GENESIS_CREAO_DB",
        "GENESIS_CREAO_ACCOUNT",
        "GENESIS_CREAO_QUOTA_DB",
        "CREAO_ACCOUNT_API_KEY",
        "CREAO_V2_API_KEY",
        "CREAO_API_VERSION",
        "CREAO_TRIGGER_API_KEY",
        "CREAO_TRIGGER_APP_ID",
        "CREAO_TRIGGER_RUNS_URL",
        "GENESIS_PUBLIC_URL",
    ] {
        std::env::remove_var(v);
    }
    let app = attach_creao_from_env(build_org(&cfg, None).unwrap())
        .await
        .unwrap();
    assert!(app.providers.configured_specialists().is_empty());

    // Partial: refused, naming what is missing (never the key value).
    std::env::set_var("GENESIS_CREAO_DB", dir.path().join("creao-org-a.sqlite"));
    std::env::set_var("CREAO_TRIGGER_API_KEY", "capi_testcanary123");
    let err = match attach_creao_from_env(build_org(&cfg, None).unwrap()).await {
        Ok(_) => panic!("partial configuration must be refused"),
        Err(e) => e.to_string(),
    };
    assert!(
        err.contains("GENESIS_CREAO_ACCOUNT") && err.contains("GENESIS_CREAO_QUOTA_DB"),
        "{err}"
    );
    assert!(!err.contains("capi_testcanary123"));

    // A legacy key cannot fill the missing app-scoped identity.
    std::env::set_var("CREAO_ACCOUNT_API_KEY", "cr_sk_obsolete_canary");
    std::env::set_var("CREAO_V2_API_KEY", "cr_sk_obsolete_canary");
    std::env::set_var("GENESIS_CREAO_ACCOUNT", "shared-demo-account");
    std::env::set_var("GENESIS_CREAO_QUOTA_DB", dir.path().join("quota.sqlite"));
    std::env::remove_var("CREAO_TRIGGER_API_KEY");
    let error = attach_creao_from_env(build_org(&cfg, None).unwrap())
        .await
        .err()
        .unwrap()
        .to_string();
    assert!(error.contains("CREAO_TRIGGER_API_KEY"));
    assert!(error.contains("CREAO_TRIGGER_APP_ID"));
    assert!(!error.contains("cr_sk_obsolete_canary"));
    std::env::set_var("CREAO_TRIGGER_API_KEY", "capi_testcanary123");
    // Complete: registered after offline startup recovery (no provider calls).
    std::env::set_var(
        "CREAO_TRIGGER_APP_ID",
        "00000000-0000-4000-8000-000000000001",
    );
    std::env::set_var("GENESIS_CREAO_ACCOUNT", "shared-demo-account");
    std::env::set_var("GENESIS_CREAO_QUOTA_DB", dir.path().join("quota.sqlite"));
    let app = attach_creao_from_env(build_org(&cfg, None).unwrap())
        .await
        .unwrap();
    assert_eq!(
        app.providers.configured_specialists(),
        vec![genesis_core::model::Provider::Creao]
    );
    // Never route to a URL from another app or fall back to the Developer API.
    for url in [
        "https://developer.creao.ai/v2/runs",
        "https://agent.creao.ai/api/v1/apps/00000000-0000-4000-8000-000000000002/runs",
        "https://SECRET_CANARY@example.invalid/runs",
    ] {
        std::env::set_var("CREAO_TRIGGER_RUNS_URL", url);
        let error = attach_creao_from_env(build_org(&cfg, None).unwrap())
            .await
            .err()
            .unwrap()
            .to_string();
        assert!(!error.contains("SECRET_CANARY"));
        assert!(!error.contains("capi_testcanary123"));
    }
    std::env::remove_var("CREAO_TRIGGER_RUNS_URL");
    std::env::set_var("CREAO_API_VERSION", "v2");
    assert!(attach_creao_from_env(build_org(&cfg, None).unwrap())
        .await
        .is_err());
    std::env::remove_var("CREAO_API_VERSION");
}
