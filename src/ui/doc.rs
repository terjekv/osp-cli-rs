//! Human-facing document IR used between UI lowering and emitters.
//!
//! `ui` does not render directly from rows or semantic payloads. Lowering first
//! converts semantic output into this smaller document model so terminal and
//! markdown emitters can share one structural contract.
//!
//! In practice:
//!
//! - lowering owns how rows, documents, and key/value output become blocks
//! - emitters own how those blocks become final text in a concrete format
//! - this module should stay dumb data, not a second rendering engine

use crate::core::output_model::ColumnAlignment;

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Doc {
    pub blocks: Vec<Block>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Block {
    Blank,
    Rule,
    Paragraph(ParagraphBlock),
    Section(SectionBlock),
    Table(TableBlock),
    Entries(EntriesBlock),
    KeyValue(KeyValueBlock),
    List(ListBlock),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParagraphBlock {
    pub text: String,
    pub indent: usize,
    pub inline_markup: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SectionBlock {
    pub title: Option<String>,
    pub title_chrome: SectionTitleChrome,
    pub body_indent: usize,
    pub inline_title_suffix: Option<String>,
    pub trailing_newline: bool,
    pub blocks: Vec<Block>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum SectionTitleChrome {
    #[default]
    Plain,
    Ruled,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TableBlock {
    pub cell_styles: Vec<Vec<Option<String>>>,
    pub summary: Vec<KeyValueRow>,
    pub headers: Vec<String>,
    pub rows: Vec<Vec<String>>,
    pub column_align: Vec<ColumnAlignment>,
    /// Drawn inside a record (`key (n):`); uses `ui.table.nested_border`.
    pub nested: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
/// Named text and its presentation hints, shared by every emitter.
///
/// Keeping entries typed gives Markdown and terminal rendering one owner for
/// labels and descriptions.
pub struct EntriesBlock {
    pub inline_markup: bool,
    pub default_indent: String,
    pub default_gap: Option<String>,
    pub rows: Vec<EntryRow>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EntryRow {
    pub key: String,
    pub value: String,
    pub indent_hint: Option<String>,
    pub gap_hint: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct KeyValueBlock {
    pub rows: Vec<KeyValueRow>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct KeyValueRow {
    pub value_style: Option<crate::ui::style::StyleToken>,
    pub key: String,
    pub value: KeyValueValue,
    pub indent: Option<String>,
    pub gap: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum KeyValueValue {
    Empty,
    Scalar(String),
    Array(Vec<KeyValueValue>),
    Object(Vec<KeyValueRow>),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ListBlock {
    pub items: Vec<String>,
    pub indent: usize,
    pub inline_markup: bool,
    pub auto_grid: bool,
}
