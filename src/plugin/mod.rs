//! Drop-in `osp-NAME` executables, optional command metadata, and process execution.
//!
//! Search order determines the winning executable. Builtins and native commands
//! take precedence in the host. Metadata is optional and never gates execution.

pub(crate) mod catalog;
pub(crate) mod discovery;
pub(crate) mod env;
pub mod manager;
pub(crate) mod run;

pub use manager::{CommandCatalogEntry, CommandSource, DEFAULT_PLUGIN_PROCESS_TIMEOUT_MS,
    Plugin, PluginDispatchContext, PluginManager, PluginSource};
