//! The real Apify capability executor. All request bounds, exact-build checks,
//! token handling and provider provenance checks stay in `genesis-apify`.
//! The kernel owns authorization, durable ticket storage and domain verification.

use crate::{CapabilityExecutor, CapabilityRunReceipt, ProviderError};
use async_trait::async_trait;
use genesis_apify::{ApifyClient, Error, ErrorKind, RunRequest, RunTicket};
use genesis_core::model::Provider;
use genesis_verification::ExecutionEvidence;
use std::collections::BTreeMap;

#[derive(Debug)]
pub struct ApifyCapabilityExecutor {
    client: ApifyClient,
}

impl ApifyCapabilityExecutor {
    /// Accept a configured production client, or a loopback client for tests.
    pub fn new(client: ApifyClient) -> Self {
        Self { client }
    }
}

#[async_trait]
impl CapabilityExecutor for ApifyCapabilityExecutor {
    fn provider(&self) -> Provider {
        Provider::Apify
    }

    async fn submit(&self, request: &RunRequest) -> Result<CapabilityRunReceipt, ProviderError> {
        let ticket = self
            .client
            .submit(request)
            .await
            .map_err(map_submit_error)?;
        let refs = BTreeMap::from([
            ("actor_id".into(), request.artifact.actor_id.clone()),
            ("build_id".into(), request.artifact.build_id.clone()),
            ("build_number".into(), request.artifact.build_number.clone()),
            ("run_id".into(), ticket.run_id().to_owned()),
        ]);
        Ok(CapabilityRunReceipt {
            provider: Provider::Apify,
            provider_refs: refs,
            // `submit` does not poll and does not claim a native terminal status.
            raw_status: "APIFY_SUBMITTED".into(),
            ticket: Some(ticket),
        })
    }

    async fn resume(
        &self,
        request: &RunRequest,
        ticket: &RunTicket,
    ) -> Result<ExecutionEvidence, ProviderError> {
        self.client
            .resume(request, ticket)
            .await
            .map_err(map_read_error)
    }
}

/// Preserve uncertainty once a POST may have happened, even if its response
/// reports HTTP failure. A positively identified terminal failure is resolved.
fn map_submit_error(error: Error) -> ProviderError {
    if error.submission_may_have_occurred && error.kind != ErrorKind::RunFailed {
        return ProviderError::Unknown(error.to_string());
    }
    map_error(error)
}

/// Resume performs no submission: immutable provenance/output violations and
/// terminal run failure reject the run. Unresolved reads can be retried using
/// the existing ticket, never by submitting another run.
fn map_read_error(error: Error) -> ProviderError {
    match error.kind {
        ErrorKind::Transport | ErrorKind::MalformedResponse | ErrorKind::UnknownRunStatus => {
            ProviderError::Unknown(error.to_string())
        }
        _ => map_error(error),
    }
}

pub(crate) fn map_error(error: Error) -> ProviderError {
    // Display includes only enum stage/kind, never URLs, input, tokens, bodies
    // or provider IDs. Do not format the error's Debug or retain raw HTTP text.
    let message = error.to_string();
    match error.kind {
        ErrorKind::HttpStatus(status) => ProviderError::Http {
            status,
            body: message,
        },
        ErrorKind::DeadlineExceeded => ProviderError::Timeout,
        _ => ProviderError::Rejected(message),
    }
}
