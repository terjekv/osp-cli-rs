//! Drop-in `osp-NAME` executables, optional command metadata, and process execution.
//!
//! Search order determines the winning executable. Builtins and native commands
//! take precedence in the host. Metadata is optional and never gates execution.

pub(crate) mod catalog;
pub(crate) mod discovery;
pub(crate) mod env;
mod manager;
pub(crate) mod run;

pub(crate) use manager::{
    CommandCatalogEntry, CommandSource, DEFAULT_PLUGIN_PROCESS_TIMEOUT_MS, Plugin, PluginManager,
    PluginSource,
};

pub(crate) use manager::PluginDispatchContext;

#[cfg(test)]
#[cfg_attr(coverage_nightly, coverage(off))]
#[path = "tests/integration.rs"]
mod integration_tests;
