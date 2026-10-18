//! Completion exists to turn a partially typed line plus cursor position into a
//! ranked suggestion set.
//!
//! This module stays deliberately free of terminal state, network access, and
//! REPL/editor concerns. The core flow is:
//!
//! - `tree`: build a static command/config completion tree
//! - `parse`: tokenize and analyze a partially typed command line
//! - `suggest`: rank and shape suggestions from the parsed cursor context
//!
//! Outer layers such as [`crate::cli`] and [`crate::repl`] inject dynamic
//! command catalogs, shell scope, alias expansion, and live prompt behavior on
//! top of this pure engine.
//!
//! Contract:
//!
//! - completion logic may depend on structured command metadata and cursor
//!   state
//! - it should not depend on terminal painting, network I/O, plugin process
//!   execution, or interactive host state
//!
//! Public API shape:
//!
//! - `tree` exposes builder/factory-style entrypoints such as
//!   [`crate::completion::CompletionTreeBuilder`] and
//!   [`crate::completion::CompletionNode`]
//! - `model` stays mostly plain semantic data so parsers, suggesters, and
//!   embedders can exchange completion state without hauling builder objects
//!   around
//! - terminal/editor integration belongs in outer layers like [`crate::repl`]

mod context;
/// High-level orchestration that combines parsing, context resolution, and suggestion ranking.
mod engine;
/// Shared completion data structures passed between the parser and suggester.
pub(crate) mod paths;
pub use paths::path_candidates;
mod model;
/// Tokenization and cursor-aware command-line parsing.
mod parse;
/// Suggestion ranking and output shaping from parsed cursor state.
mod suggest;
/// Declarative completion-tree builders derived from command and config metadata.
mod tree;

pub use engine::CompletionEngine;
pub use model::{
    ArgNode, CommandLine, CommandWordResolver, CompletionNode, CompletionTree, ContextScope,
    CursorState, DynamicCompletion, FlagCompletions, FlagHints, FlagNode, FlagOccurrence,
    QuoteStyle, Suggestion, SuggestionEntry, SuggestionOutput, TailItem, ValueCatalog, ValueType,
};
pub use parse::CommandLineParser;
pub use suggest::closest_values;
pub use tree::{CompletionTreeBuildError, CompletionTreeBuilder};

pub(crate) use model::{CompletionAnalysis, CompletionRequest};
pub(crate) use parse::TokenSpan;
pub(crate) use tree::ConfigKeySpec;

#[cfg(test)]
#[path = "tests/integration.rs"]
mod integration_tests;
