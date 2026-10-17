#![cfg_attr(
    not(test),
    warn(clippy::expect_used, clippy::panic, clippy::unwrap_used)
)]
#![deny(missing_docs)]

//! `osp-cli` is the library behind the `osp` CLI and REPL.
//!
//! Use it when you want one of these jobs:
//!
//! - run the full `osp` host in-process
//! - build a wrapper crate with site-specific native commands and defaults
//! - render rows or run DSL pipelines in-process
//!
//! Most readers only need one of those lanes. You do not need to understand
//! the whole crate before using it.
//!
//! The crate also keeps the full `osp` product surface in one place so the
//! main concerns stay visible together: host orchestration, config resolution,
//! rendering, REPL integration, completion, plugins, and the pipeline DSL.
//! That makes rustdoc a useful architecture map after you have picked the
//! smallest surface that fits your job.
//!
//! Quick starts for the three most common library shapes:
//!
//! Full `osp`-style host with captured output:
//!
//! ```
//! use osp_cli::App;
//! use osp_cli::app::BufferedUiSink;
//!
//! let mut sink = BufferedUiSink::default();
//! let exit = App::new().run_with_sink(["osp", "--help"], &mut sink)?;
//!
//! assert_eq!(exit, 0);
//! assert!(!sink.stdout.is_empty());
//! assert!(sink.stderr.is_empty());
//! # Ok::<(), miette::Report>(())
//! ```
//!
//! Rendering existing rows without bootstrapping the full host:
//!
//! ```
//! use osp_cli::core::OutputFormat;
//! use osp_cli::row;
//! use osp_cli::ui::{RenderSettings, render};
//!
//! let output = osp_cli::core::OutputResult::from_rows(vec![
//!     row! { "uid" => "alice", "mail" => "alice@example.com" },
//! ]);
//! let settings = RenderSettings::test_plain(OutputFormat::Json);
//! let rendered = render(&output, &osp_cli::ui::RenderRequest::new(&settings));
//!
//! assert!(rendered.contains("\"uid\": \"alice\""));
//! assert!(rendered.contains("\"mail\": \"alice@example.com\""));
//! ```
//!
//! Building a product-specific wrapper crate:
//!
//! - keep site-specific auth, policy, and domain integrations in the wrapper
//!   crate
//! - Extend the command surface with [`App::with_native_commands`].
//! - keep runtime config bootstrap aligned with
//!   [`config::RuntimeDefaults`], [`config::RuntimeConfigPaths`], and
//!   [`config::build_runtime_pipeline`]
//! - expose a thin product-level `run_process` or builder API on top of this
//!   crate instead of forking generic host behavior
//!
//! Minimal wrapper shape:
//!
//! ```
//! use std::ffi::OsString;
//!
//! use anyhow::Result;
//! use clap::Command;
//! use osp_cli::app::BufferedUiSink;
//! use osp_cli::config::ConfigLayer;
//! use osp_cli::{
//!     App, NativeCommand, NativeCommandContext, NativeCommandOutcome,
//!     NativeCommandRegistry,
//! };
//!
//! struct SiteStatusCommand;
//!
//! impl NativeCommand for SiteStatusCommand {
//!     fn command(&self) -> Command {
//!         Command::new("site-status").about("Show site-specific status")
//!     }
//!
//!     fn execute(
//!         &self,
//!         _args: &[String],
//!         _context: &NativeCommandContext<'_>,
//!     ) -> Result<NativeCommandOutcome> {
//!         Ok(NativeCommandOutcome::Exit(0))
//!     }
//! }
//!
//! fn site_registry() -> NativeCommandRegistry {
//!     NativeCommandRegistry::new().with_command(SiteStatusCommand)
//! }
//!
//! fn site_defaults() -> ConfigLayer {
//!     let mut defaults = ConfigLayer::default();
//!     defaults.set("extensions.site.enabled", true);
//!     defaults
//! }
//!
//! #[derive(Clone)]
//! struct SiteApp {
//!     inner: App,
//! }
//!
//! impl SiteApp {
//!     fn builder() -> App {
//!         App::new()
//!             .with_native_commands(site_registry())
//!             .with_product_defaults(site_defaults())
//!     }
//!
//!     fn new() -> Self {
//!         Self {
//!             inner: Self::builder(),
//!         }
//!     }
//!
//!     fn run_process<I, T>(&self, args: I) -> i32
//!     where
//!         I: IntoIterator<Item = T>,
//!         T: Into<OsString> + Clone,
//!     {
//!         self.inner.run_process(args)
//!     }
//! }
//!
//! let app = SiteApp::new();
//! let mut sink = BufferedUiSink::default();
//! let exit = app.inner.run_process_with_sink(["osp", "--help"], &mut sink);
//!
//! assert_eq!(exit, 0);
//! assert!(sink.stdout.contains("site-status"));
//! assert_eq!(app.run_process(["osp", "--help"]), 0);
//! ```
//!
//! If you are new here, start with one of these:
//!
//! - wrapper crate / downstream product →
//!   [embedding guide](https://github.com/unioslo/osp-cli-rs/blob/main/docs/EMBEDDING.md)
//!   and [`App::new`]
//! - full in-process host → [`app`]
//! - rendering / formatting only → [`ui`]
//!
//! Start here depending on what you need:
//!
//! - [`app`] exists to turn the lower-level pieces into a running CLI or REPL
//!   process.
//! - [`cli`] exists to model the public command-line grammar.
//! - [`config`] exists to answer what values are legal, where they came from,
//!   and what finally wins.
//! - [`completion`] exists to rank suggestions without depending on terminal
//!   state or editor code.
//! - [`repl`] exists to own the interactive shell boundary.
//! - [`dsl`] exists to transform canonical rows and explicit group metadata.
//! - [`ui`] exists to lower structured output into terminal-facing text.
//! - `plugin` exists to treat external command providers as part of the same
//!   command surface.
//!
//! At runtime, data flows roughly like this:
//!
//! ```text
//! argv / REPL line
//!      │
//!      ▼ [ cli ]     parse grammar and flags
//!      ▼ [ config ]  resolve layered settings (builtin → file → env → cli)
//!      ▼ [ app ]     dispatch to plugin or native command  ──►  OutputResult
//!      ▼ [ dsl ]     transform rows and explicit groups   ──►  OutputResult
//!      ▼ [ ui ]      render structured output to terminal or UiSink
//! ```
//!
//! Commands normalize service collections into rows before DSL execution.
//! Raw response documents are available for unstaged rendering; a staged result
//! describes the selected rows rather than stale service totals or cursors.
//! Document output retains its layout only through stages that preserve its shape.
//! Group keys and aggregates are explicit metadata, not inferred from row fields.
//!
//! Architecture contracts worth keeping stable:
//!
//! - lower-level modules should not depend on [`app`]
//! - [`completion`] stays pure and should not start doing network, plugin
//!   discovery, or terminal I/O
//! - [`ui`] renders structured input but should not become a config-resolver or
//!   service-execution layer
//! - [`cli`] describes the grammar of the program but does not execute it
//! - [`config`] owns precedence and legality rules so callers do not invent
//!   their own merge semantics
//!
//! Public API shape:
//!
//! - semantic payloads such as [`core::Document`] and most of [`completion`]
//!   stay intentionally cheap to compose and inspect
//! - host machinery such as [`app::App`] and runtime
//!   state is guided through constructors/builders/accessors rather than
//!   compatibility shims or open-ended assembly
//! - each public concept should have one canonical home; duplicate aliases and
//!   mirrored module paths are treated as API debt
//!
//! For embedders, choose the smallest surface that solves the problem you
//! actually have:
//!
//! - "I want a full `osp`-style binary or custom `main`" →
//!   [`app::App::new`] or
//!   [`app::App::run_from`]
//! - "I want to capture rendered stdout/stderr in tests or another host" →
//!   [`app::App::run_with_sink`] or [`app::App::run_process_with_sink`]
//! - "I already have rows and only want pipeline transforms" →
//!   [`dsl::apply_pipeline`] or [`dsl::apply_output_pipeline`]
//! - "I need plugin discovery and catalog/policy integration" →
//!   [`App`] owns discovery; use [`app::AccessPolicy`] for product policy and
//!   [`core::plugin`] for the wire protocol
//! - "I need the configured host" → [`App::new`]
//!   and apply launch settings through its `with_*` methods
//! - "I need interactive native-command hooks" → [`repl`] and [`NativeCommand`]
//! - "I need semantic payload generation for help/completion surfaces" →
//!   [`core::Document`] and [`completion::CompletionTreeBuilder`]
//!
//! The root crate module tree is the only supported code path. Older mirrored
//! layouts have been removed so rustdoc and the source tree describe the same
//! architecture.

/// Main host-facing entrypoints, runtime state, and session types.
pub mod app;
/// Command-line argument types and CLI parsing helpers.
pub mod cli;
/// Structured command and pipe completion types.
pub mod completion;
/// Layered configuration schema, loading, and resolution.
pub mod config;
/// Shared command, output, row, and protocol primitives.
pub mod core;
/// Allowlisted command-local diagnostic events.
pub mod diagnostics;
/// Canonical pipeline parsing and execution.
pub mod dsl;
/// External plugin discovery, protocol, and dispatch support.
pub(crate) mod plugin;
/// Interactive REPL editor, prompt, history, and completion surface.
pub mod repl;
mod startup_trace;
/// Rendering, theming, and structured output helpers.
pub mod ui;

pub use crate::app::{App, run_from, run_process};
pub use crate::native::{
    InvocationElevation, NativeCommand, NativeCommandContext, NativeCommandOutcome,
    NativeCommandRegistry, NativePagination, NativeProgressEvent, NativeProgressSink,
    NativePromptContextEntry, NativeSessionContext, NativeSessionContextEntry, NativeWrite,
};

mod native;
mod normalize;

#[cfg(test)]
mod tests;

/// Declared entity selection shared by commands and plugins.
pub mod context;
