//! Bounded CREAO API Trigger and explicit legacy Account API operations.
//! Trigger runs a prepublished app; it cannot create or inspect agents.
//!
//! Submit exactly once, durably save the returned ticket, then poll explicitly.
//! A lost POST response is ambiguous; neither this client nor a run ID supplies
//! provider-side idempotency. Provider completion is not mission acceptance.

mod client;
mod model;
mod rate_limiter;
pub mod runtime;

pub use client::{CreaoClient, PreEffectGuard};
pub use model::{
    AgentObservation, ApiVersion, CreateAgentRequest, Error, ErrorKind, Limits, NativeRunStatus,
    OperationRequest, RunObservation, RunRequest, RunTicket, Stage, WriteAutonomy,
};
pub use rate_limiter::{AccountLimiter, LimiterError, QuotaLimits, RequestKind};
pub use runtime::{
    OperationPhase, OperationPurpose, OperationRecord, OperationSpec, Runtime, RuntimeError,
    RuntimeFailure, RuntimeStore,
};
