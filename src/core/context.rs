//! Entity wire declarations shared by plugins and native command grammars.
//!
//! Runtime selection, argument resolution and persistence belong to `context`.

use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

/// Shared vocabulary for selections across native commands and plugins.
pub(crate) const KINDS: &[&str] = &[
    "account", "group", "person", "host", "netgroup", "contact", "task", "provider", "vm",
    "network", "zone",
];
/// How an omitted entity argument may use context.
#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum Fill {
    /// Only an explicit value or `.` is accepted.
    #[default]
    Explicit,
    /// An omitted read subject may be filled.
    Read,
    /// A remembered scope may narrow an explicitly named object's operation.
    Narrow,
}

/// One declared entity argument; argument names are clap IDs or flag spellings.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Consumes {
    /// Shared entity kind.
    pub kind: String,
    /// Positional ID or long flag; empty for plugin environment-only consumption.
    #[serde(default)]
    pub argument: String,
    /// Omission policy.
    #[serde(default)]
    pub fill: Fill,
    /// Other arguments that make this selection inapplicable.
    #[serde(default)]
    pub conflicts: Vec<String>,
    /// Require a value even when the grammar leaves the argument optional.
    #[serde(default)]
    pub required: bool,
}

/// Entity metadata attached with clap `add`, including from derive attributes.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ContextContract {
    /// Arguments which consume remembered entities.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub consumes: Vec<Consumes>,
    /// Kind to canonical result pointers (or `$ARG` for an acknowledged name).
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub produces: BTreeMap<String, Vec<String>>,
}
impl clap::builder::CommandExt for ContextContract {}
impl ContextContract {
    /// Declares an entity argument.
    pub fn consumes(mut self, kind: &str, argument: &str, fill: Fill) -> Self {
        self.consumes.push(Consumes {
            kind: kind.into(),
            argument: argument.into(),
            fill,
            conflicts: Vec::new(),
            required: false,
        });
        self
    }
    /// Declares a canonical entity in a singleton successful result.
    pub fn produces(mut self, kind: &str, pointer: &str) -> Self {
        self.produces
            .entry(kind.into())
            .or_default()
            .push(pointer.into());
        self
    }
}
