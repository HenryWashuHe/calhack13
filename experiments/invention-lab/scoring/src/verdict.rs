//! Compatibility re-export: the acceptance verdict now lives in
//! `genesis_core::development_verdict` so the server can recompute it from
//! retained records without depending on the lab scorer.
pub use genesis_core::development_verdict::*;
