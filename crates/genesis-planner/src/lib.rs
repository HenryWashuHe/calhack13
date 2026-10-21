//! # genesis-planner
//!
//! The mission planning agent. It watches an organization's evidence, asks a
//! language model whether the organization must change, and submits a
//! structured proposal to the kernel under the restricted agent identity.
//! The kernel decides; the planner only proposes.
//!
//! The model is pluggable ([`llm::LlmClient`]); Elastic Agent Builder can be
//! added as another implementation without changing the loop.

pub mod agent;
pub mod kernel;
pub mod llm;
pub mod output;
pub mod prompt;
pub mod runner;
