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
    ] {
        std::env::remove_var(v);
    }
    let app = attach_creao_from_env(build_org(&cfg, None).unwrap())
        .await
        .unwrap();
    assert!(app.providers.configured_specialists().is_empty());

    // Partial: refused, naming what is missing (never the key value).
    std::env::set_var("GENESIS_CREAO_DB", dir.path().join("creao-org-a.sqlite"));
    std::env::set_var("CREAO_ACCOUNT_API_KEY", "cr_sk_testcanary123");
    let err = match attach_creao_from_env(build_org(&cfg, None).unwrap()).await {
        Ok(_) => panic!("partial configuration must be refused"),
        Err(e) => e.to_string(),
    };
    assert!(
        err.contains("GENESIS_CREAO_ACCOUNT") && err.contains("GENESIS_CREAO_QUOTA_DB"),
        "{err}"
    );
    assert!(!err.contains("cr_sk_testcanary123"));

    // Complete: registered after offline startup recovery (no provider calls).
    std::env::set_var("GENESIS_CREAO_ACCOUNT", "shared-demo-account");
    std::env::set_var("GENESIS_CREAO_QUOTA_DB", dir.path().join("quota.sqlite"));
    let app = attach_creao_from_env(build_org(&cfg, None).unwrap())
        .await
        .unwrap();
    assert_eq!(
        app.providers.configured_specialists(),
        vec![genesis_core::model::Provider::Creao]
    );
}
