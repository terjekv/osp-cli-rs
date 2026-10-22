//! Final text emitters for the UI document IR.
//!
//! All emitters consume the shared [`crate::ui::doc::Doc`] shape. That keeps
//! markdown, terminal, and JSON-ish rendering aligned on structure even when
//! their concrete text output differs.
//!
//! This layer should stay about rendering only. Format selection and semantic
//! lowering happen earlier in the UI pipeline.

pub(crate) mod delimited;
mod entries;
pub(crate) mod grid;
pub(crate) mod json;
mod key_value;
mod markdown;
pub(crate) mod shared;
mod table;
mod terminal;

use crate::core::output::OutputFormat;

use super::doc::Doc;
use super::settings::ResolvedRenderSettings;

pub fn emit_doc(doc: &Doc, format: OutputFormat, settings: &ResolvedRenderSettings) -> String {
    match format {
        OutputFormat::Markdown => markdown::emit_doc(doc),
        _ => terminal::emit_doc(doc, settings),
    }
}
