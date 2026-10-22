//! Human-facing document IR used between UI lowering and emitters.
//!
//! Human projection supplies content and the planner chooses layout. Lowering
//! copies both into this model so terminal and Markdown emitters share one
//! structural contract. Typed machine exports bypass this IR.
//!
//! In practice:
//!
//! - lowering copies projected content and planned layout into blocks
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
    Grid(GridBlock),
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
    Card,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TableBlock {
    pub cell_lines: Vec<Vec<Vec<String>>>,
    pub footer: Vec<String>,
    pub widths: Vec<usize>,
    pub cell_styles: Vec<Vec<Option<crate::ui::style::CellStyle>>>,
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
    pub role: crate::core::document::EntryRole,
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
    /// Planned hanging list or scalar text, relative to the child level.
    pub list_grid: Option<GridBlock>,
    pub value_style: Option<crate::ui::style::CellStyle>,
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

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GridBlock {
    pub rows: Vec<Vec<GridCell>>,
    pub column_widths: Vec<usize>,
    /// Reserved label columns, including the label/value gap.
    pub label_widths: Vec<usize>,
    pub gap: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GridCell {
    pub label: String,
    pub value: String,
    pub value_style: Option<crate::ui::style::CellStyle>,
}
