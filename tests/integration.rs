#![cfg_attr(coverage_nightly, feature(coverage_attribute))]
#![cfg(not(miri))]
#![allow(missing_docs)]

#[path = "support/temp.rs"]
mod temp_support;

#[path = "integration/app/mod.rs"]
mod app;

#[path = "integration/embedding_workflow.rs"]
mod embedding_workflow;

#[path = "integration/repl/mod.rs"]
mod repl;

#[path = "support/isolation.rs"]
mod isolation;

#[path = "support/pty.rs"]
mod pty_support;

#[path = "integration/config.rs"]
mod config;

#[path = "integration/dsl_output.rs"]
mod dsl_output;

#[cfg(test)]
#[cfg_attr(coverage_nightly, coverage(off))]
#[path = "integration/context_resolution.rs"]
mod context_resolution;

#[cfg(test)]
#[cfg_attr(coverage_nightly, coverage(off))]
#[path = "integration/snapshots.rs"]
mod snapshots;

#[cfg(test)]
#[cfg_attr(coverage_nightly, coverage(off))]
#[path = "integration/diagnostic_output.rs"]
mod diagnostic_output;

#[cfg(test)]
#[cfg_attr(coverage_nightly, coverage(off))]
#[path = "integration/context_persistence.rs"]
mod context_persistence;

#[path = "integration/transient_output.rs"]
mod transient_output;
