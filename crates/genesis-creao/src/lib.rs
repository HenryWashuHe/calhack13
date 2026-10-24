//! Bounded CREAO Account API requests and durable polling receipts.
//!
//! Submit exactly once, durably save the returned ticket, then poll explicitly.
//! A lost POST response is ambiguous; neither this client nor a run ID supplies
//! provider-side idempotency. Provider completion is not mission acceptance.

mod client;
mod model;
mod rate_limiter;
pub mod runtime;

pub use client::CreaoClient;
pub use model::{
    AgentObservation, CreateAgentRequest, Error, ErrorKind, Limits, NativeRunStatus,
    OperationRequest, RunObservation, RunRequest, RunTicket, Stage, WriteAutonomy,
};
pub use rate_limiter::{AccountLimiter, LimiterError, QuotaLimits, RequestKind};
pub use runtime::{
    OperationPhase, OperationPurpose, OperationRecord, OperationSpec, Runtime, RuntimeError,
    RuntimeFailure, RuntimeStore,
};
