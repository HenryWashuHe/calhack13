//! Agent Builder provisioning against the fake Kibana API.

use genesis_core::engine::RoleToProvision;
use genesis_core::ids::{ChangeId, OrgId, RoleId};
use genesis_core::model::AuthorityGrant;
use genesis_core::model::Provider;
use genesis_elastic::fake::start_kibana;
use genesis_elastic::{AgentBuilder, AgentBuilderConfig};
use genesis_providers::{ProviderError, ProvisionRequest, RoleProvisioner};
use std::time::Duration;

fn builder(url: &str, space: Option<&str>) -> AgentBuilder {
    AgentBuilder::new(AgentBuilderConfig {
        kibana_url: url.into(),
        api_key: "bWdtdC1rZXk=".into(),
        space: space.map(Into::into),
        index_prefix: "genesis".into(),
        timeout: Duration::from_secs(5),
    })
    .unwrap()
}

fn request(change: &str, charter: &str) -> ProvisionRequest {
    ProvisionRequest {
        org_id: OrgId::new("org_a"),
        change_id: ChangeId::new(change),
        role: RoleToProvision {
            key: "relocation".into(),
            role_id: RoleId::new("role_reloc"),
            name: "Relocation lead".into(),
            charter: charter.into(),
            authority: AuthorityGrant::new(["ledger.reserve_venue"], 150_000),
            provider: Provider::Elastic,
        },
    }
}

#[tokio::test]
async fn provisions_a_scoped_specialist_and_checks_readiness() {
    let (url, kib) = start_kibana().await;
    let ab = builder(&url, None);
    let req = request("chg-1", "Secure a replacement venue for 120 attendees");
    let binding = ab.provision(&req).await.unwrap();
    assert_eq!(binding.provider, Provider::Elastic);
    assert!(binding
        .provider_agent_id
        .starts_with("genesis-org_a-relocation-"));
    assert!(ab.check_ready(&binding).await.unwrap());

    let f = kib.lock().unwrap();
    let tool_id = ab.evidence_tool_id("org_a");
    let tool = &f.tools[&tool_id];
    assert_eq!(tool["type"], "index_search");
    assert_eq!(
        tool["configuration"]["pattern"],
        genesis_elastic::evidence_index_name("genesis", "org_a")
    );
    let agent = &f.agents[&binding.provider_agent_id];
    assert_eq!(
        agent["configuration"]["tools"][0]["tool_ids"][0],
        tool_id.as_str()
    );
    let instructions = agent["configuration"]["instructions"].as_str().unwrap();
    assert!(instructions.contains("Secure a replacement venue"));
    assert!(instructions.contains("ledger.reserve_venue"));
    assert!(instructions.contains("cannot change the organization"));
    assert_eq!(f.missing_xsrf, 0);
    assert!(f.auth.iter().all(|a| a == "ApiKey bWdtdC1rZXk="));
}

#[tokio::test]
async fn retries_reconcile_instead_of_creating_twice() {
    let (url, kib) = start_kibana().await;
    let ab = builder(&url, None);
    let req = request("chg-1", "Secure a replacement venue");
    let first = ab.provision(&req).await.unwrap();
    let again = ab.provision(&req).await.unwrap();
    assert_eq!(first, again);
    {
        let f = kib.lock().unwrap();
        assert_eq!(f.agents.len(), 1);
        assert_eq!(f.posts.get("agents"), Some(&1), "second attempt only reads");
    }
    // A different change gets a different agent.
    let other = ab
        .provision(&request("chg-2", "Secure a replacement venue"))
        .await
        .unwrap();
    assert_ne!(other.provider_agent_id, first.provider_agent_id);
}

#[tokio::test]
async fn conflict_after_a_lost_create_is_reconciled() {
    let (url, kib) = start_kibana().await;
    kib.lock().unwrap().conflict_after_create = true;
    let ab = builder(&url, None);
    let binding = ab.provision(&request("chg-1", "Charter")).await.unwrap();
    assert!(kib
        .lock()
        .unwrap()
        .agents
        .contains_key(&binding.provider_agent_id));
}

#[tokio::test]
async fn existing_agent_with_different_configuration_is_rejected() {
    let (url, _) = start_kibana().await;
    let ab = builder(&url, None);
    ab.provision(&request("chg-1", "Original charter"))
        .await
        .unwrap();
    let err = ab
        .provision(&request("chg-1", "Tampered charter"))
        .await
        .unwrap_err();
    assert!(matches!(err, ProviderError::Rejected(m) if m.contains("different configuration")));
}

#[tokio::test]
async fn deleted_agent_is_not_ready() {
    let (url, kib) = start_kibana().await;
    let ab = builder(&url, None);
    let binding = ab.provision(&request("chg-1", "Charter")).await.unwrap();
    kib.lock().unwrap().agents.clear();
    assert!(!ab.check_ready(&binding).await.unwrap());
}

#[tokio::test]
async fn uses_the_configured_space() {
    let (url, kib) = start_kibana().await;
    let ab = builder(&url, Some("genesis-demo"));
    ab.provision(&request("chg-1", "Charter")).await.unwrap();
    assert!(kib
        .lock()
        .unwrap()
        .paths
        .iter()
        .all(|p| p.contains("/s/genesis-demo/api/agent_builder/")));
}

#[tokio::test]
async fn converse_returns_message_and_tool_calls_and_refuses_prompts() {
    let (url, kib) = start_kibana().await;
    let ab = builder(&url, None);
    let binding = ab.provision(&request("chg-1", "Charter")).await.unwrap();
    let r = ab
        .converse(&binding.provider_agent_id, "Which venues are open?", None)
        .await
        .unwrap();
    assert_eq!(r.status, "completed");
    assert_eq!(r.message, "answer to: Which venues are open?");
    assert_eq!(r.tool_calls, vec![ab.evidence_tool_id("org_a")]);
    let r2 = ab
        .converse(
            &binding.provider_agent_id,
            "And capacity?",
            Some(&r.conversation_id),
        )
        .await
        .unwrap();
    assert_eq!(r2.conversation_id, r.conversation_id);

    kib.lock().unwrap().awaiting_prompt = true;
    let err = ab
        .converse(&binding.provider_agent_id, "Book it", None)
        .await
        .unwrap_err();
    assert!(matches!(err, ProviderError::Rejected(m) if m.contains("human prompt")));
}

#[tokio::test]
async fn http_errors_do_not_forward_provider_bodies() {
    let (url, _) = start_kibana().await;
    let ab = builder(&url, None);
    let err = ab.converse("no-such-agent", "hi", None).await.unwrap_err();
    let shown = err.to_string();
    assert!(shown.contains("404"));
    assert!(!shown.contains("secret-ish"));
}

#[tokio::test]
async fn non_elastic_roles_are_refused() {
    let (url, _) = start_kibana().await;
    let mut req = request("chg-1", "Charter");
    req.role.provider = Provider::Creao;
    let err = builder(&url, None).provision(&req).await.unwrap_err();
    assert!(matches!(err, ProviderError::Rejected(_)));
}
