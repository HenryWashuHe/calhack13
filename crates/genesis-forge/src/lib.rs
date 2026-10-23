//! A checkpointed capability forge, independent of organization authority.
//! Persist a prepared action's checkpoint BEFORE executing that action, then
//! persist its completion. Lost POST outcomes require reconciliation; reads can
//! resume. The kernel owns storage, leases, grants, publication and installation.
mod scaffold;
mod session;
mod source;

pub use scaffold::venue_source;
pub use session::*;
pub use source::*;
