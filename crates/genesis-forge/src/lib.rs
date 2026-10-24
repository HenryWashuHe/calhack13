//! A checkpointed capability forge, independent of organization authority.
//! Persist a prepared action's checkpoint BEFORE executing that action, then
//! persist its completion. Lost POST outcomes require reconciliation; reads can
//! resume. The kernel owns storage, leases, grants, publication and installation.
mod acquisition;
mod scaffold;
mod session;
mod source;
pub mod synthesis;
pub mod synthesis_http;
mod synthesis_source;

pub use acquisition::*;
pub use scaffold::{venue_source, venue_source_for_actor};
pub use session::*;
pub use source::*;
pub use synthesis::*;
pub use synthesis_http::AnthropicGenerator;
pub use synthesis_source::generated_candidate;

pub mod method_synthesis;
pub mod text_generation;

pub mod method_accounting;
pub mod method_admission;
pub mod method_admitted_costs;
pub mod method_chain;
pub mod method_revision;
pub mod method_roster;
pub mod method_stage;
