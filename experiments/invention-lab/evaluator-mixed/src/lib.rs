//! Henry's Claude: `mixed-terrain/1`, a separately versioned research family
//! (docs/invention-lab/proposals/mixed-terrain.md). Experimental and for
//! offline calibration only. It reuses design language v1 from the v1
//! evaluator crate and never changes it; v1 results are not readable through
//! this family. Simulated results say nothing about physical machines.
pub mod family;
pub mod score;
pub mod sim;
