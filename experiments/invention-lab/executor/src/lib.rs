//! Henry's Claude: invention-lab development-method executor, v0 (issue #1).
//! Runs one frozen trial slot (#28) through the experiment ledger (#32) against
//! the in-process evaluator, and writes a #18 execution receipt. See README.md
//! for what is and isn't enforced.
pub mod example;
pub mod refs;
pub mod run;
pub mod sandbox;
pub mod scoring;
pub mod session;
pub mod store;
