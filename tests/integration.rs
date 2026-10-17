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
