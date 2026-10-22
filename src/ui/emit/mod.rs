//! Final text emitters for the UI document IR.
//!
//! Human emitters consume the shared [`crate::ui::doc::Doc`] shape. JSON and
//! delimited machine writers consume typed canonical data directly and never
//! use the human IR.
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
