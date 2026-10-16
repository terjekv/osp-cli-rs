#![cfg(not(miri))]
#![allow(missing_docs)]

#[path = "support/temp.rs"]
mod temp_support;

#[path = "integration/app/mod.rs"]
mod app;

#[path = "integration/config/mod.rs"]
mod config;

#[path = "integration/diagnostics.rs"]
mod diagnostics;

#[path = "integration/dsl/mod.rs"]
mod dsl;

#[path = "integration/embedding_workflow.rs"]
mod embedding_workflow;

#[path = "integration/guide_ui.rs"]
mod guide_ui;

#[path = "integration/repl/mod.rs"]
mod repl;

#[path = "support/isolation.rs"]
mod isolation;

#[path = "support/pty.rs"]
mod pty_support;
