//! Capability acceptance core. The kernel supplies trusted oracle/pin/adapter
//! observations; generated code supplies only extraction output. Acceptance is
//! limited to tested cases and does not authorize organization mutations.
mod model;
mod verify;

pub use model::*;
pub use verify::{json_sha256, verify};
