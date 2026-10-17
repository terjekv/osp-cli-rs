//! Canonical row pipeline DSL.
//!
//! A pipeline is a command followed by zero or more transformation stages
//! separated by `|`. The stages transform the rows returned by the command:
//!
//! ```text
//! "orch task list | F status.name=running | S created_at | L 10"
//!  ─────────────────  ─────────────────────────────────
//!     command              pipeline stages
//! ```
//!
//! Data flow:
//!
//! ```text
//! raw line
//!   │
//!   ▼  parse_pipeline(line)
//!   Pipeline { command: "orch task list",
//!              stages:  ["F status.name=running", "S created_at", "L 10"] }
//!   │
//!   │  caller dispatches the command and gets rows back
//!   │
//!   ▼  apply_pipeline(rows, &stages)
//!   OutputResult  ← filtered · sorted · limited rows
//! ```
//!
//! Apply stages to existing rows with [`crate::dsl::apply_pipeline`], or use
//! [`crate::dsl::apply_output_pipeline`] to retain output metadata and semantic documents.
//! [`crate::dsl::execute_pipeline`] is also available for row-based callers. Parsing and
//! verb metadata belong to the host implementation.
//!
//! Common verbs include `F` (filter), `P` (project), `S` (sort), `G` (group),
//! `A` (aggregate), `L` (limit), `V`/`K` (quick search), `U` (unroll), and
//! `JQ` (jq expression).

pub(crate) mod compiled;
pub(crate) mod eval;
pub(crate) mod model;
pub(crate) mod parse;
pub(crate) mod verb_info;

mod engine;
#[cfg(test)]
mod value;
mod verbs;

pub use engine::{apply_output_pipeline, apply_pipeline, execute_pipeline};
pub(crate) use parse::pipeline::parse_pipeline;
pub(crate) use verb_info::{registered_verbs, verb_info};
pub(crate) use verbs::filter::parse_timestamp;

#[cfg(test)]
mod tests;
