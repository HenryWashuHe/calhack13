//! # genesis-core
//!
//! Domain model and pure organization engine for GENESIS.
//!
//! - [`ids`]: newtype identifiers.
//! - [`lifecycle`]: explicit state machines with transition tables.
//! - [`model`]: entities and the [`model::OrgState`] aggregate.
//! - [`change`]: the organization-change proposal vocabulary.
//! - [`engine`]: validation, commit, dispatch authorization and fencing.
//! - [`capability`]: manifests, verification evidence, publication rules.
//! - [`events`]: domain event envelope and kinds.
//! - [`jobs`]: job records and normalized job lifecycle.
//! - [`audit`]: structural invariants of a state, checked independently.
//!
//! This crate has no I/O. The server crate persists state and talks to
//! providers.

pub mod audit;
pub mod capability;
pub mod change;
pub mod development_catalogue;
pub mod development_evaluation;
pub mod development_experiment;
pub mod development_method;
pub mod development_package;
pub mod development_trial;
pub mod digest;
pub mod engine;
pub mod events;
pub mod ids;
pub mod jobs;
pub mod lifecycle;
pub mod model;
pub mod schema;

pub use change::{OrgChangeProposal, OrgOp};
pub use engine::{Outcome, ValidationCode, ValidationError};
pub use model::OrgState;
