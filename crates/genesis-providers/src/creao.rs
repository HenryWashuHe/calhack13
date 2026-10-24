//! Durable CREAO specialist operations at the provider boundary. The kernel must
//! authorize dispatch and independently prove readiness/domain results. Native
//! creation deliberately does not implement the synchronous RoleProvisioner trait.

use crate::{ProviderError, ProvisionRequest};
use async_trait::async_trait;
use genesis_core::jobs::Job;
use genesis_core::lifecycle::JobStatus;
use genesis_core::model::{AgentBinding, Provider};
use genesis_creao::{
    CreateAgentRequest, OperationPhase as Phase, OperationPurpose, OperationRecord,
    OperationRequest, OperationSpec, RunRequest, Runtime,
};
use genesis_verification::json_sha256;
use serde::Serialize;
use serde_json::{json, Value};
use std::sync::Arc;

/// Progress is provider evidence only. In particular, Created requires a separate
/// readiness check before the kernel can activate a role.
#[derive(Clone, Debug, Serialize)]
#[serde(tag = "state", rename_all = "snake_case")]
pub enum SpecialistProgress {
    Pending {
        operation_id: String,
        run_id: Option<String>,
        native_status: Option<String>,
    },
    Created {
        operation_id: String,
        run_id: String,
        provider_agent_id: String,
        config_digest: String,
        readiness_verified: bool,
    },
    Collected {
        operation_id: String,
        run_id: String,
        provider_agent_id: String,
        output: Option<Value>,
        domain_verified: bool,
    },
    Failed {
        operation_id: String,
        run_id: Option<String>,
    },
    NeedsReconciliation {
        operation_id: String,
        run_id: Option<String>,
    },
}
impl SpecialistProgress {
    pub fn operation_id(&self) -> &str {
        match self {
            Self::Pending { operation_id, .. }
            | Self::Created { operation_id, .. }
            | Self::Collected { operation_id, .. }
            | Self::Failed { operation_id, .. }
            | Self::NeedsReconciliation { operation_id, .. } => operation_id,
        }
    }
}

/// A split, durable provider contract. Preparing saves an immutable local intent;
/// advancing performs at most one native operation. The existing synchronous
/// provisioning saga must not pretend Pending is a ready agent binding.
#[async_trait]
pub trait SpecialistRuntime: Send + Sync {
    fn provider(&self) -> Provider;
    fn org_id(&self) -> &str;
    fn prepare_role(&self, request: &ProvisionRequest)
        -> Result<SpecialistProgress, ProviderError>;
    fn prepare_job(
        &self,
        job: &Job,
        binding: &AgentBinding,
    ) -> Result<SpecialistProgress, ProviderError>;
    fn get(&self, operation_id: &str) -> Result<SpecialistProgress, ProviderError>;
    async fn advance(
        &self,
        operation_id: &str,
        retry_read: bool,
    ) -> Result<SpecialistProgress, ProviderError>;
}

#[derive(Clone)]
pub struct CreaoSpecialist {
    runtime: Arc<Runtime>,
}
impl CreaoSpecialist {
    pub fn new(runtime: Runtime) -> Self {
        Self {
            runtime: Arc::new(runtime),
        }
    }

    /// Startup-only recovery, before accepting new commands. Ambiguous POSTs
    /// become reconciliation work; this method does not contact CREAO.
    pub async fn recover_interrupted(&self) -> Result<usize, ProviderError> {
        self.runtime
            .recover_interrupted()
            .await
            .map_err(runtime_error)
    }
}

fn rejected(message: &str) -> ProviderError {
    ProviderError::Rejected(message.into())
}
fn runtime_error(_error: impl std::fmt::Display) -> ProviderError {
    // The local operation record carries sanitized diagnostics. Do not leak
    // caller input or database details through the generic provider boundary.
    rejected("CREAO durable operation could not advance; inspect its backend checkpoint")
}
fn operation_id(kind: &str, value: Value) -> String {
    format!("creao-{kind}-{}", json_sha256(&value))
}

#[async_trait]
impl SpecialistRuntime for CreaoSpecialist {
    fn provider(&self) -> Provider {
        Provider::Creao
    }
    fn org_id(&self) -> &str {
        self.runtime.org_id()
    }

    fn prepare_role(
        &self,
        request: &ProvisionRequest,
    ) -> Result<SpecialistProgress, ProviderError> {
        if request.role.provider != Provider::Creao || request.org_id.as_str() != self.org_id() {
            return Err(rejected(
                "CREAO role operation belongs to a different provider or organization",
            ));
        }
        let context = json!({"org_id":request.org_id,"change_id":request.change_id,"role_id":request.role.role_id});
        let spec = OperationSpec {
            schema_version:1, operation_id:operation_id("role",context), org_id:request.org_id.to_string(), role_id:request.role.role_id.to_string(),
            account_namespace:self.runtime.account_namespace().to_owned(),
            purpose:OperationPurpose::Provision {change_id:request.change_id.to_string(),config_digest:request.config_digest()},
            request:OperationRequest::CreateAgent(CreateAgentRequest {
                input:format!("Create a persistent operational specialist for GENESIS using this role contract: {}. Provider creation does not grant organization authority. Configure a GENESIS readiness gateway skill that, when explicitly invoked with a readiness challenge, sends only the supplied challenge-bound readiness receipt to its supplied GENESIS endpoint. This readiness POST is the sole exception to requiring an explicit GENESIS job and scoped grant before calling a gateway; it never grants mission authority. Require an explicit GENESIS job and scoped grant for all mission work. Do not claim the skill is installed or readiness is proven without returned evidence. Do not claim task success without returned evidence.",serde_json::to_string(request).map_err(runtime_error)?),
                model:None,conversation_id:None,
            }),
        };
        progress(self.runtime.prepare(spec).map_err(runtime_error)?)
    }

    fn prepare_job(
        &self,
        job: &Job,
        binding: &AgentBinding,
    ) -> Result<SpecialistProgress, ProviderError> {
        if job.org_id.as_str() != self.org_id()
            || job.provider != Some(Provider::Creao)
            || binding.provider != Provider::Creao
        {
            return Err(rejected(
                "CREAO job or binding belongs to a different provider or organization",
            ));
        }
        if job.status != JobStatus::Queued
            || job
                .deadline
                .is_some_and(|deadline| deadline <= chrono::Utc::now())
        {
            return Err(rejected(
                "CREAO dispatch requires a queued job before its deadline",
            ));
        }
        let spec = OperationSpec {
            schema_version: 1,
            operation_id: operation_id(
                "job",
                json!({"org_id":job.org_id,"job_id":job.job_id,"attempt_id":job.attempt_id}),
            ),
            org_id: job.org_id.to_string(),
            role_id: job.role_id.to_string(),
            account_namespace: self.runtime.account_namespace().to_owned(),
            purpose: OperationPurpose::Execution {
                job_id: job.job_id.to_string(),
                assignment_id: job.assignment_id.to_string(),
                epoch: job.epoch,
            },
            request: OperationRequest::CreateRun(RunRequest {
                agent_id: binding.provider_agent_id.clone(),
                input: json!({"schema_version":1,"task":job.task,"input":job.input,
                    "context":{"org_id":job.org_id,"mission_id":job.mission_id,"role_id":job.role_id,
                        "responsibility_id":job.responsibility_id,"assignment_id":job.assignment_id,"epoch":job.epoch,
                        "job_id":job.job_id,"attempt_id":job.attempt_id,"attempt_no":job.attempt_no,
                        "binding_config_digest":binding.config_digest,"correlation_id":job.correlation_id,
                        "idempotency_key":job.idempotency_key,"capability":job.capability,"deadline":job.deadline}}),
                model: None,
                // Each event gets a fresh execution conversation. A provider's
                // creation conversation is not an execution conversation.
                conversation_id: None,
                workspace_id: None,
            }),
        };
        progress(self.runtime.prepare(spec).map_err(runtime_error)?)
    }

    fn get(&self, operation_id: &str) -> Result<SpecialistProgress, ProviderError> {
        progress(self.runtime.get(operation_id).map_err(runtime_error)?)
    }
    async fn advance(
        &self,
        operation_id: &str,
        retry_read: bool,
    ) -> Result<SpecialistProgress, ProviderError> {
        // Readiness has its own owner. Do not let the general role/job adapter
        // execute even a prepared readiness POST before rejecting its progress.
        specialist_purpose(&self.runtime.get(operation_id).map_err(runtime_error)?)?;
        progress(
            self.runtime
                .step(operation_id, retry_read)
                .await
                .map_err(runtime_error)?,
        )
    }
}

fn specialist_purpose(record: &OperationRecord) -> Result<(), ProviderError> {
    if matches!(record.spec.purpose, OperationPurpose::Readiness { .. }) {
        return Err(rejected(
            "CREAO readiness operations are not role or job progress",
        ));
    }
    Ok(())
}

fn progress(record: OperationRecord) -> Result<SpecialistProgress, ProviderError> {
    specialist_purpose(&record)?;
    let operation_id = record.spec.operation_id.clone();
    let run_id = record
        .ticket
        .as_ref()
        .map(|ticket| ticket.run_id().to_owned());
    match record.phase {
        Phase::NativeCompleted => {
            let observation = record
                .observation
                .ok_or_else(|| rejected("CREAO completed checkpoint lacks an observation"))?;
            let run_id =
                run_id.ok_or_else(|| rejected("CREAO completed checkpoint lacks its ticket"))?;
            let provider_agent_id = observation
                .agent_id
                .ok_or_else(|| rejected("CREAO completed checkpoint lacks an agent identity"))?;
            match record.spec.purpose {
                OperationPurpose::Provision { config_digest, .. } => {
                    Ok(SpecialistProgress::Created {
                        operation_id,
                        run_id,
                        provider_agent_id,
                        config_digest,
                        readiness_verified: false,
                    })
                }
                OperationPurpose::Execution { .. } => Ok(SpecialistProgress::Collected {
                    operation_id,
                    run_id,
                    provider_agent_id,
                    output: observation.result,
                    domain_verified: false,
                }),
                OperationPurpose::Readiness { .. } => Err(rejected(
                    "CREAO readiness operations are not role or job progress",
                )),
            }
        }
        Phase::Failed => Ok(SpecialistProgress::Failed {
            operation_id,
            run_id,
        }),
        Phase::NeedsReconciliation => Ok(SpecialistProgress::NeedsReconciliation {
            operation_id,
            run_id,
        }),
        _ => Ok(SpecialistProgress::Pending {
            operation_id,
            run_id,
            native_status: record
                .observation
                .as_ref()
                .map(|o| o.status.as_str().to_owned()),
        }),
    }
}
