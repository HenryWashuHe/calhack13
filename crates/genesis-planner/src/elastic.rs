//! The planner's model as an Elastic Agent Builder agent (TECH_SPEC: "an
//! Elastic mission agent produces a structured proposal").
//!
//! The organization's mission-planner agent carries the planner's system
//! prompt as its instructions and the organization's evidence search as its
//! only tool, so it can ground proposals in recorded observations and
//! requirements. Each completion is one fresh conversation containing the
//! planner's messages. Its reply is the same untrusted JSON as any other
//! model's; the planner parses it and the kernel validates it.

use crate::llm::{LlmClient, LlmError, LlmRequest, Role};
use async_trait::async_trait;
use genesis_elastic::{AgentBuilder, AgentBuilderConfig};
use genesis_providers::ProviderError;
use tokio::sync::Mutex;

/// Categories only; provider text never reaches planner decisions.
fn map_error(e: ProviderError) -> LlmError {
    match e {
        ProviderError::Http { status, .. } => LlmError::Http {
            status,
            kind: "agent_builder".into(),
        },
        ProviderError::Timeout => LlmError::Transport("timeout".into()),
        ProviderError::Unknown(_) => LlmError::Transport("agent_builder".into()),
        ProviderError::Rejected(_) | ProviderError::NotConfigured { .. } => {
            LlmError::Malformed("agent builder refused the turn".into())
        }
    }
}

pub struct ElasticAgentLlm {
    builder: AgentBuilder,
    org_id: String,
    /// Agent ID per instruction set, created on first use.
    agent: Mutex<Option<(String, String)>>,
}

impl ElasticAgentLlm {
    pub fn new(builder: AgentBuilder, org_id: impl Into<String>) -> Self {
        Self {
            builder,
            org_id: org_id.into(),
            agent: Mutex::new(None),
        }
    }

    /// From `ELASTIC_KIBANA_URL` and `ELASTIC_MANAGEMENT_API_KEY`.
    pub fn from_env(org_id: impl Into<String>) -> Result<Self, LlmError> {
        let cfg = AgentBuilderConfig::from_env().ok_or_else(|| {
            LlmError::NotConfigured(
                "ELASTIC_KIBANA_URL and ELASTIC_MANAGEMENT_API_KEY are required for the elastic planner".into(),
            )
        })?;
        let builder = AgentBuilder::new(cfg)
            .map_err(|_| LlmError::NotConfigured("invalid Agent Builder configuration".into()))?;
        Ok(Self::new(builder, org_id))
    }

    async fn agent_for(&self, instructions: &str) -> Result<String, LlmError> {
        let mut cached = self.agent.lock().await;
        if let Some((text, id)) = cached.as_ref() {
            if text == instructions {
                return Ok(id.clone());
            }
        }
        let id = self
            .builder
            .ensure_planner_agent(&self.org_id, instructions)
            .await
            .map_err(map_error)?;
        *cached = Some((instructions.to_owned(), id.clone()));
        Ok(id)
    }
}

/// The planner's conversation as one input for a fresh agent conversation.
fn transcript(req: &LlmRequest) -> String {
    if req.messages.len() == 1 {
        return req.messages[0].content.clone();
    }
    let mut out = String::from(
        "Earlier turns of this planning task follow. Answer the last user message.\n\n",
    );
    for m in &req.messages {
        let who = match m.role {
            Role::User => "User",
            Role::Assistant => "Your earlier reply",
        };
        out.push_str(&format!("### {who}\n{}\n\n", m.content));
    }
    out
}

#[async_trait]
impl LlmClient for ElasticAgentLlm {
    fn model(&self) -> String {
        format!("elastic-agent-builder/{}", self.org_id)
    }

    async fn complete(&self, req: &LlmRequest) -> Result<String, LlmError> {
        let instructions = format!(
            "{}\n\nBefore answering, use your evidence search tool to read the organization's \
             recorded observations, requirements and responsibilities that bear on the new evidence. \
             Your final message must be only the JSON object described above.",
            req.system
        );
        let agent = self.agent_for(&instructions).await?;
        let turn = self
            .builder
            .converse(&agent, &transcript(req), None)
            .await
            .map_err(map_error)?;
        if turn.message.trim().is_empty() {
            return Err(LlmError::Malformed("empty agent reply".into()));
        }
        Ok(turn.message)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::llm::Message;

    #[test]
    fn single_message_is_sent_as_is_and_repairs_include_the_history() {
        let one = LlmRequest {
            system: "s".into(),
            messages: vec![Message::user("evidence")],
            max_tokens: 10,
        };
        assert_eq!(transcript(&one), "evidence");
        let repair = LlmRequest {
            system: "s".into(),
            messages: vec![
                Message::user("evidence"),
                Message::assistant("{bad}"),
                Message::user("fix it"),
            ],
            max_tokens: 10,
        };
        let t = transcript(&repair);
        assert!(t.contains("### User\nevidence") && t.contains("### Your earlier reply\n{bad}"));
        assert!(t.trim_end().ends_with("fix it"));
    }

    #[test]
    fn provider_errors_keep_only_categories() {
        let e = map_error(ProviderError::Http {
            status: 401,
            body: "key=secret".into(),
        });
        assert!(!e.to_string().contains("secret"));
        assert!(matches!(
            map_error(ProviderError::Rejected("x".into())),
            LlmError::Malformed(_)
        ));
    }
}
