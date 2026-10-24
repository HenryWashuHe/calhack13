//! Durable CREAO specialist operations at the provider boundary. The kernel must
//! authorize dispatch and independently prove readiness/domain results. Native
//! creation deliberately does not implement the synchronous RoleProvisioner trait.

use crate::{PreEffectGuard, ProviderError, ProvisionRequest};
use async_trait::async_trait;
use genesis_core::ids::{
    AssignmentId, AttemptId, JobId, MissionId, OrgId, ResponsibilityId, RoleId,
};
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
use std::fmt;
use std::sync::Arc;

/// Historical, backend-collected execution provenance, not domain verification
/// or current authority. Obtain this only from a trusted runtime with the frozen
/// job and execution binding; serialized receipts are not importable proofs.
/// Callers must separately authorize any new action against current kernel state.
///
/// Execution binds the provider, agent ID and configuration digest. A binding's
/// `conversation_ref` is provisioning metadata, not an execution input: every
/// execution starts a fresh conversation. Mutable job status, result, provider
/// references and timestamps likewise are not part of the submitted request.
#[derive(Clone, PartialEq, Serialize)]
pub struct SpecialistExecutionReceipt {
    pub schema_version: u32,
    pub provider: Provider,
    pub org_id: OrgId,
    pub mission_id: MissionId,
    pub role_id: RoleId,
    pub responsibility_id: ResponsibilityId,
    pub job_id: JobId,
    pub attempt_id: AttemptId,
    pub assignment_id: AssignmentId,
    pub attempt_no: u32,
    pub epoch: u64,
    pub operation_id: String,
    pub account_namespace: String,
    pub provider_agent_id: String,
    pub binding_config_digest: String,
    pub run_id: String,
    pub conversation_id: Option<String>,
    pub workspace_id: Option<String>,
    /// Canonical hash of the submitted provider request, excluding operation
    /// ownership metadata; distinct from the full operation fingerprint below.
    pub request_sha256: String,
    /// Fingerprint of the full immutable, organization/account-bound operation.
    pub operation_fingerprint: String,
    /// Domain-separated canonical hash of the complete validated checkpoint,
    /// including its native observation and raw response. Not authentication.
    pub checkpoint_sha256: String,
    pub checkpoint_revision: u64,
    /// Native completion may have no result. Missing or malformed semantic
    /// output must be rejected by the domain consumer, not silently promoted to
    /// task success. Output retains the runtime's validated response bounds.
    pub output: Option<Value>,
    /// Canonical hash of serialized `output`; absent output hashes JSON null.
    /// The full checkpoint hash also binds the original response shape.
    pub output_sha256: String,
}

impl fmt::Debug for SpecialistExecutionReceipt {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("SpecialistExecutionReceipt")
            .field("provider", &self.provider)
            .field("org_id", &self.org_id)
            .field("job_id", &self.job_id)
            .field("attempt_id", &self.attempt_id)
            .field("operation_id", &self.operation_id)
            .field("run_id", &self.run_id)
            .field("operation_fingerprint", &self.operation_fingerprint)
            .field("checkpoint_sha256", &self.checkpoint_sha256)
            .field("checkpoint_revision", &self.checkpoint_revision)
            .field("output", &"[redacted]")
            .field("output_sha256", &self.output_sha256)
            .finish_non_exhaustive()
    }
}

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
    /// Read a completed execution bound to the authoritative frozen job and
    /// execution binding. Does not prepare, submit, poll, or infer fresh kernel
    /// authority. Implementations without bound receipts reject by default.
    fn execution_receipt(
        &self,
        _job: &Job,
        _binding: &AgentBinding,
    ) -> Result<SpecialistExecutionReceipt, ProviderError> {
        Err(rejected("specialist execution receipts are not supported"))
    }
    async fn advance(
        &self,
        operation_id: &str,
        retry_read: bool,
    ) -> Result<SpecialistProgress, ProviderError>;
    /// Check backend authority after provider queue/quota admission and before
    /// each network effect. Implementations without this guarantee reject;
    /// silently delegating to unguarded advance would drop the authority fence.
    async fn advance_guarded(
        &self,
        _operation_id: &str,
        _retry_read: bool,
        _guard: PreEffectGuard,
    ) -> Result<SpecialistProgress, ProviderError> {
        Err(rejected("guarded specialist advancement is not supported"))
    }
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

    /// Reconstruct exactly the existing submission contract. Dispatch eligibility
    /// is checked separately so historical receipts survive terminal status and
    /// expiry of the original (still immutable) deadline.
    fn execution_spec(
        &self,
        job: &Job,
        binding: &AgentBinding,
    ) -> Result<OperationSpec, ProviderError> {
        if job.org_id.as_str() != self.org_id()
            || job.provider != Some(Provider::Creao)
            || binding.provider != Provider::Creao
        {
            return Err(rejected(
                "CREAO job or binding belongs to a different provider or organization",
            ));
        }
        Ok(OperationSpec {
            schema_version: 1,
            api_version: self.runtime.api_version(),
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
                // Provisioning conversations are not execution conversations.
                conversation_id: None,
                workspace_id: None,
            }),
        })
    }
}

fn rejected(message: &str) -> ProviderError {
    ProviderError::Rejected(message.into())
}
fn runtime_error(_error: impl std::fmt::Display) -> ProviderError {
    // The local operation record carries sanitized diagnostics. Do not leak
    // caller input or database details through the generic provider boundary.
    rejected("CREAO durable operation unavailable; inspect its backend checkpoint")
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
            schema_version:1, api_version:self.runtime.api_version(), operation_id:operation_id("role",context), org_id:request.org_id.to_string(), role_id:request.role.role_id.to_string(),
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
        let spec = self.execution_spec(job, binding)?;
        if job.status != JobStatus::Queued
            || job
                .deadline
                .is_some_and(|deadline| deadline <= chrono::Utc::now())
        {
            return Err(rejected(
                "CREAO dispatch requires a queued job before its deadline",
            ));
        }
        progress(self.runtime.prepare(spec).map_err(runtime_error)?)
    }

    fn get(&self, operation_id: &str) -> Result<SpecialistProgress, ProviderError> {
        progress(self.runtime.get(operation_id).map_err(runtime_error)?)
    }

    fn execution_receipt(
        &self,
        job: &Job,
        binding: &AgentBinding,
    ) -> Result<SpecialistExecutionReceipt, ProviderError> {
        let expected = self.execution_spec(job, binding)?;
        // get() validates the bounded persisted checkpoint, ticket, observation,
        // raw response and runtime's organization/account context. It does not
        // create a missing operation or contact the provider.
        let record = self
            .runtime
            .get(&expected.operation_id)
            .map_err(runtime_error)?;
        if record.spec != expected
            || record.fingerprint != expected.fingerprint().map_err(runtime_error)?
        {
            return Err(rejected(
                "CREAO execution checkpoint does not match the frozen job and binding",
            ));
        }
        if record.phase != Phase::NativeCompleted {
            return Err(rejected("CREAO execution has not completed natively"));
        }
        let ticket = record
            .ticket
            .as_ref()
            .ok_or_else(|| rejected("CREAO completed checkpoint lacks its ticket"))?;
        let observation = record
            .observation
            .as_ref()
            .ok_or_else(|| rejected("CREAO completed checkpoint lacks an observation"))?;
        if ticket.run_id() != observation.run_id
            || ticket.agent_id() != Some(binding.provider_agent_id.as_str())
            || observation.agent_id.as_deref() != Some(binding.provider_agent_id.as_str())
        {
            return Err(rejected(
                "CREAO completed checkpoint does not match the execution agent and run",
            ));
        }
        let checkpoint_sha256 = json_sha256(&json!({
            "domain": "genesis-specialist-execution-checkpoint-v1",
            "record": serde_json::to_value(&record).map_err(runtime_error)?,
        }));
        let output_sha256 =
            json_sha256(&serde_json::to_value(&observation.result).map_err(runtime_error)?);
        Ok(SpecialistExecutionReceipt {
            schema_version: 1,
            provider: Provider::Creao,
            org_id: job.org_id.clone(),
            mission_id: job.mission_id.clone(),
            role_id: job.role_id.clone(),
            responsibility_id: job.responsibility_id.clone(),
            job_id: job.job_id.clone(),
            attempt_id: job.attempt_id.clone(),
            assignment_id: job.assignment_id.clone(),
            attempt_no: job.attempt_no,
            epoch: job.epoch,
            operation_id: record.spec.operation_id.clone(),
            account_namespace: record.spec.account_namespace.clone(),
            provider_agent_id: binding.provider_agent_id.clone(),
            binding_config_digest: binding.config_digest.clone(),
            run_id: ticket.run_id().to_owned(),
            conversation_id: observation.conversation_id.clone(),
            workspace_id: observation.workspace_id.clone(),
            request_sha256: record.spec.request_sha256().map_err(runtime_error)?,
            operation_fingerprint: record.fingerprint.clone(),
            checkpoint_sha256,
            checkpoint_revision: record.revision,
            output: observation.result.clone(),
            output_sha256,
        })
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

    async fn advance_guarded(
        &self,
        operation_id: &str,
        retry_read: bool,
        guard: PreEffectGuard,
    ) -> Result<SpecialistProgress, ProviderError> {
        specialist_purpose(&self.runtime.get(operation_id).map_err(runtime_error)?)?;
        progress(
            self.runtime
                .step_guarded(operation_id, retry_read, guard)
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
