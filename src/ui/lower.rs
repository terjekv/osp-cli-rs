//! Lower structured output into the UI document IR.
//!
//! This module is the semantic bridge between `crate::core::output_model`
//! and [`crate::ui::doc`]. It owns the answer to "what kind of document should
//! a given output payload become?" while deliberately staying ignorant of
//! terminal-specific rendering details.
//!
//! Keep the split here:
//!
//! - output semantics and document blocks come in from upstream modules
//! - this module chooses document structure
//! - emitters later decide spacing, borders, markdown markers, and terminal
//!   chrome

use serde_json::{Map, Value};

use super::doc::{
    Block, Doc, EntriesBlock, EntryRow, KeyValueBlock, KeyValueRow, KeyValueValue, ListBlock,
    ParagraphBlock, SectionBlock, SectionTitleChrome, TableBlock,
};
use super::plan::{FieldLayout, MregLayout, PlannedTable, RecordLayout, RenderPlan};
use super::projection::{display_labels, display_path, display_value, ordered_keys};
use super::settings::HelpLayout;
use super::text::display_width;
use super::visible_inline_text;
use crate::core::document::{DocBlock, Document};
use crate::core::output::OutputFormat;
use crate::core::output_model::{
    DisplayRule, Group, OutputDocument, OutputItems, OutputResult, output_items_to_rows,
};
use crate::core::row::Row;

pub fn lower_output(projection: &super::projection::HumanProjection, plan: &RenderPlan) -> Doc {
    let output = &projection.output;
    let is_empty = match &output.items {
        OutputItems::Rows(rows) => rows.is_empty() || rows.iter().all(Row::is_empty),
        OutputItems::Groups(groups) => groups.is_empty(),
    };
    let declared_empty_table = matches!(
        plan.format,
        OutputFormat::Table | OutputFormat::Guide | OutputFormat::Markdown
    ) && output
        .meta
        .display_columns
        .as_ref()
        .is_some_and(|columns| !columns.is_empty());
    if plan.format != OutputFormat::Json
        && is_empty
        && !declared_empty_table
        && output.meta.display_notes.is_empty()
    {
        return Doc {
            blocks: vec![Block::Paragraph(ParagraphBlock {
                text: "No results.".to_string(),
                indent: 0,
                inline_markup: false,
            })],
        };
    }

    if output.meta.document_rows
        && matches!(
            plan.format,
            OutputFormat::Auto | OutputFormat::Guide | OutputFormat::Markdown
        )
    {
        let doc = match output.document.as_ref().and_then(OutputDocument::semantic) {
            Some(document) => lower_document(document, plan),
            None => lower_document_rows(output.as_rows().unwrap_or_default(), plan.format),
        };
        return doc;
    }

    if output.meta.document_rows && plan.format == OutputFormat::Value {
        let items: Vec<String> = output
            .as_rows()
            .unwrap_or_default()
            .iter()
            .flat_map(|row| {
                if row.len() == 3
                    && let Some(description) = row.get("description").and_then(Value::as_str)
                {
                    return vec![if description.is_empty() {
                        row.get("name").map(display_value).unwrap_or_default()
                    } else {
                        description.into()
                    }];
                }
                row.iter()
                    .filter(|(key, value)| key.as_str() != "section" && !value.is_null())
                    .flat_map(|(_, value)| match value {
                        Value::Array(items) => items.iter().map(display_value).collect(),
                        value => vec![display_value(value)],
                    })
                    .collect()
            })
            .collect();
        let doc = Doc {
            blocks: vec![Block::Paragraph(ParagraphBlock {
                text: items.join("\n"),
                indent: 0,
                inline_markup: false,
            })],
        };
        return doc;
    }

    let mut doc = match plan.format {
        OutputFormat::Guide | OutputFormat::Table => lower_table_doc(output, plan),
        OutputFormat::Markdown => lower_markdown_doc(output, plan),
        OutputFormat::Value => lower_value_doc(output),
        OutputFormat::Mreg => lower_mreg_doc(output, plan),
        OutputFormat::Auto | OutputFormat::Json | OutputFormat::Csv | OutputFormat::Tsv => {
            Doc::default()
        }
    };
    if plan.settings.color && plan.format != OutputFormat::Json {
        apply_value_styles(&mut doc.blocks, &projection.source);
        apply_warning_styles(&mut doc.blocks, &projection.source, &plan.settings);
    }
    if plan.format != OutputFormat::Json {
        doc.blocks
            .extend(output.meta.display_notes.iter().map(|text| {
                Block::Paragraph(ParagraphBlock {
                    text: text.clone(),
                    indent: 0,
                    inline_markup: false,
                })
            }));
    }
    if let Some((shown, total)) = projection.shortened {
        doc.blocks.push(Block::Paragraph(ParagraphBlock {
            text: format!("Showing {shown} of {total} rows. Pipes and --json see every row."),
            indent: 0,
            inline_markup: false,
        }));
    }
    doc
}

pub(crate) fn lower_document(document: &Document, plan: &RenderPlan) -> Doc {
    Doc {
        blocks: lower_document_blocks(
            &document.blocks,
            DocumentLayout::Direct { plan },
            false,
            false,
            &mut Vec::new(),
        ),
    }
}

fn lower_document_rows(rows: &[Row], format: OutputFormat) -> Doc {
    let mut blocks = Vec::new();
    let mut start = 0;
    while start < rows.len() {
        let section = rows[start]
            .get("section")
            .and_then(Value::as_str)
            .unwrap_or("");
        let end = start
            + rows[start..]
                .iter()
                .take_while(|row| {
                    row.get("section").and_then(Value::as_str).unwrap_or("") == section
                })
                .count();
        let mut body = Vec::new();
        for row in &rows[start..end] {
            if row.len() == 3
                && row.contains_key("description")
                && let Some(name) = row.get("name").and_then(Value::as_str)
            {
                let entry = EntryRow {
                    key: name.into(),
                    value: row
                        .get("description")
                        .and_then(Value::as_str)
                        .unwrap_or("")
                        .into(),
                    indent_hint: None,
                    gap_hint: None,
                };
                if let Some(Block::Entries(entries)) = body.last_mut() {
                    entries.rows.push(entry);
                } else {
                    body.push(Block::Entries(EntriesBlock {
                        inline_markup: true,
                        default_indent: " ".repeat(2),
                        default_gap: None,
                        rows: vec![entry],
                    }));
                }
            } else if row.len() == 2
                && let Some(text) = row.get("text").and_then(Value::as_str)
            {
                body.push(Block::Paragraph(ParagraphBlock {
                    text: text.into(),
                    indent: 0,
                    inline_markup: true,
                }));
            } else {
                let mut data = row.clone();
                data.remove("section");
                body.extend(lower_value_blocks(&Value::Object(data), None));
            }
        }
        let next = if section.is_empty() {
            body
        } else {
            vec![Block::Section(SectionBlock {
                title: Some(section.into()),
                title_chrome: if format == OutputFormat::Markdown {
                    SectionTitleChrome::Ruled
                } else {
                    SectionTitleChrome::Plain
                },
                body_indent: 0,
                inline_title_suffix: None,
                trailing_newline: false,
                blocks: body,
            })]
        };
        extend_with_blank_between(&mut blocks, next);
        start = end;
    }
    Doc { blocks }
}

#[derive(Clone, Copy)]
enum DocumentLayout<'a> {
    Direct {
        plan: &'a RenderPlan,
    },
    Help {
        plan: &'a RenderPlan,
        layout: HelpLayout,
    },
}

fn lower_document_blocks(
    blocks: &[DocBlock],
    layout: DocumentLayout<'_>,
    inside: bool,
    long_help: bool,
    path: &mut Vec<usize>,
) -> Vec<Block> {
    let (format, help, plan) = match layout {
        DocumentLayout::Direct { plan } => (plan.format, None, plan),
        DocumentLayout::Help { plan, layout } => (OutputFormat::Auto, Some((plan, layout)), plan),
    };
    let mut lowered = Vec::new();
    for (index, block) in blocks.iter().enumerate() {
        path.push(index);
        let next = match block {
            DocBlock::Section { title, blocks, .. } => {
                let suffix = match blocks.as_slice() {
                    [DocBlock::Paragraph { text }]
                        if help.is_some() && title == "Usage" && text.lines().count() == 1 =>
                    {
                        Some(visible_inline_text(text))
                    }
                    _ => None,
                };
                let body = if suffix.is_some() {
                    Vec::new()
                } else {
                    lower_document_blocks(
                        blocks,
                        layout,
                        true,
                        long_help
                            && !matches!(
                                title.as_str(),
                                "Common Invocation Options" | "Product options"
                            ),
                        path,
                    )
                };
                vec![Block::Section(SectionBlock {
                    title: (help.is_none() || !title.is_empty()).then(|| title.clone()),
                    title_chrome: if suffix.is_some() {
                        SectionTitleChrome::Plain
                    } else if let Some((_, layout)) = help {
                        help_layout_title_chrome(layout)
                    } else if matches!(
                        format,
                        OutputFormat::Auto | OutputFormat::Guide | OutputFormat::Markdown
                    ) {
                        SectionTitleChrome::Ruled
                    } else {
                        SectionTitleChrome::Plain
                    },
                    body_indent: if format == OutputFormat::Markdown {
                        0
                    } else {
                        2
                    },
                    inline_title_suffix: suffix,
                    trailing_newline: help.is_none() && format != OutputFormat::Markdown,
                    blocks: body,
                })]
            }
            DocBlock::Paragraph { text } => vec![Block::Paragraph(ParagraphBlock {
                text: text.clone(),
                indent: 0,
                inline_markup: true,
            })],
            DocBlock::Entries { entries } => {
                let mut block =
                    entries_block_with_defaults(entries, if inside { 0 } else { 2 }, None);
                if let Some((plan, _)) = help {
                    block.inline_markup =
                        !long_help || !entries.iter().all(|entry| entry.name.starts_with('-'));
                    let width = entries
                        .iter()
                        .map(|entry| display_width(&entry.name))
                        .max()
                        .unwrap_or(0);
                    for row in &mut block.rows {
                        if long_help && row.key.starts_with('-') {
                            row.value = row
                                .value
                                .lines()
                                .filter(|line| !line.is_empty())
                                .collect::<Vec<_>>()
                                .join("\n");
                            row.indent_hint =
                                Some(" ".repeat(if row.key.starts_with("--") { 4 } else { 0 }));
                            row.gap_hint = Some("\n        ".into());
                        } else {
                            row.gap_hint = plan.document_entry_gap.map(|gap| {
                                " ".repeat(width.saturating_sub(display_width(&row.key)) + gap)
                            });
                        }
                    }
                }
                vec![Block::Entries(block)]
            }
            DocBlock::List { items } => vec![Block::List(ListBlock {
                items: if format == OutputFormat::Markdown {
                    items.clone()
                } else {
                    items.iter().map(|item| format!("- {item}")).collect()
                },
                indent: 0,
                inline_markup: true,
                auto_grid: false,
            })],
            DocBlock::Data { value } => match help {
                Some(_) => help_layout_blocks_from_value(
                    value,
                    if inside { 0 } else { 2 },
                    plan.document_tables.get(path),
                ),
                None => lower_value_blocks(value, plan.document_tables.get(path)),
            },
        };
        path.pop();
        if help.is_some() {
            let section_break = matches!(block, DocBlock::Section { .. })
                || matches!(lowered.last(), Some(Block::Section(_)));
            if section_break {
                push_blank_lines(&mut lowered, 1);
            } else if !lowered.is_empty() && !matches!(block, DocBlock::Paragraph { .. }) {
                lowered.push(Block::Blank);
            }
            lowered.extend(next);
        } else {
            extend_with_blank_between(&mut lowered, next);
        }
    }
    lowered
}

/// `key (N):` for several rows; a single row needs no count.
fn section_title(key: &str, count: usize) -> String {
    if count == 1 {
        format!("{key}:")
    } else {
        format!("{key} ({count}):")
    }
}

fn apply_warning_styles(
    blocks: &mut [Block],
    output: &OutputResult,
    settings: &crate::ui::settings::ResolvedRenderSettings,
) {
    use crate::core::DisplayRule;
    use crate::ui::style::{StyleToken, style_spec};
    let warns = |key: &str| {
        output
            .meta
            .display_rules
            .iter()
            .any(|rule| matches!(rule, DisplayRule::Warning { field } if field == key))
    };
    for block in blocks {
        match block {
            Block::Section(section) => apply_warning_styles(&mut section.blocks, output, settings),
            Block::KeyValue(block) => {
                for row in &mut block.rows {
                    if warns(&row.key) {
                        row.value_style = Some(StyleToken::Warning);
                    }
                }
            }
            Block::Table(table) => {
                for (index, key) in table.headers.iter().enumerate() {
                    if warns(key) {
                        table.cell_styles.resize_with(table.rows.len(), Vec::new);
                        for styles in &mut table.cell_styles {
                            styles.resize(table.headers.len(), None);
                            styles[index] = Some(
                                style_spec(
                                    &settings.theme,
                                    &settings.style_overrides,
                                    StyleToken::Warning,
                                )
                                .into(),
                            );
                        }
                    }
                }
            }
            _ => {}
        }
    }
}

fn apply_value_styles(blocks: &mut [Block], output: &OutputResult) {
    use crate::core::output_model::DisplayRule;
    let columns = output
        .meta
        .display_columns
        .as_deref()
        .unwrap_or(&output.meta.key_index);
    let labels = display_labels(output, columns);
    let style = |key: &str, value: &str, row_index: usize| {
        output.meta.display_rules.iter().find_map(|rule| {
            let DisplayRule::ValueStyle { field, bold } = rule else {
                return None;
            };
            let label = columns
                .iter()
                .position(|column| column == field)
                .and_then(|index| labels.get(index))
                .unwrap_or(field);
            if key != label {
                return None;
            }
            let original = output
                .as_rows()
                .and_then(|rows| rows.get(row_index))
                .and_then(|row| display_path(&Value::Object(row.clone()), field).cloned());
            let value = original.as_ref().and_then(Value::as_str).unwrap_or(value);
            crate::core::style::is_valid_style_spec(value).then(|| {
                if *bold {
                    format!("bold {value}")
                } else {
                    value.to_owned()
                }
            })
        })
    };
    for block in blocks {
        match block {
            Block::Section(section) => apply_value_styles(&mut section.blocks, output),
            Block::Table(table) => {
                table.cell_styles = table
                    .rows
                    .iter()
                    .enumerate()
                    .map(|(row_index, row)| {
                        row.iter()
                            .zip(&table.headers)
                            .map(|(value, key)| style(key, value, row_index))
                            .collect()
                    })
                    .collect();
            }
            _ => {}
        }
    }
}

fn lower_value_doc(output: &OutputResult) -> Doc {
    let values = output_items_to_rows(&output.items)
        .iter()
        .flat_map(|row| row_value_displays(row, &output.meta.key_index))
        .collect::<Vec<_>>();

    let blocks = if values.is_empty() {
        Vec::new()
    } else {
        vec![Block::Paragraph(ParagraphBlock {
            text: values.join("\n"),
            indent: 0,
            inline_markup: false,
        })]
    };

    Doc { blocks }
}

pub(crate) fn lower_document_help_layout(
    document: &Document,
    plan: &RenderPlan,
    layout: HelpLayout,
    footer: bool,
) -> Doc {
    let mut blocks = lower_document_blocks(
        &document.blocks,
        DocumentLayout::Help { plan, layout },
        false,
        plan.document_long_help,
        &mut Vec::new(),
    );
    if footer
        && blocks
            .iter()
            .any(|block| matches!(block, Block::Section(_)))
    {
        blocks.push(Block::Rule);
    }
    Doc { blocks }
}

pub(crate) fn json_value(output: &OutputResult) -> Value {
    let data = if let Some(document) = &output.document
        && (output.meta.document_rows || matches!(document, OutputDocument::Json(_)))
    {
        document.json_value()
    } else {
        match &output.items {
            OutputItems::Rows(rows) => {
                Value::Array(rows.iter().cloned().map(Value::Object).collect())
            }
            OutputItems::Groups(groups) => {
                Value::Array(groups.iter().map(json_group_value).collect())
            }
        }
    };
    if output.meta.context.is_empty() {
        data
    } else {
        serde_json::json!({"data": data, "meta": {"context": output.meta.context}})
    }
}

fn lower_table_doc(output: &OutputResult, plan: &RenderPlan) -> Doc {
    match &output.items {
        OutputItems::Rows(_) => Doc {
            blocks: plan
                .tables
                .iter()
                .map(|table| Block::Table(table_from_plan(table)))
                .collect(),
        },
        OutputItems::Groups(groups) => {
            let mut blocks = Vec::new();
            for (index, group) in groups.iter().enumerate() {
                if index > 0 {
                    blocks.push(Block::Blank);
                }
                let mut table = table_from_plan(&plan.tables[index]);
                table.summary = summary_from_group(group, &output.meta.key_index);
                blocks.push(Block::Table(table));
            }
            Doc { blocks }
        }
    }
}

fn lower_markdown_doc(output: &OutputResult, plan: &RenderPlan) -> Doc {
    match &output.items {
        OutputItems::Rows(rows) if plan.markdown_record => Doc {
            blocks: vec![Block::KeyValue(key_value_from_map(
                &rows[0],
                &output.meta.key_index,
            ))],
        },
        _ => lower_table_doc(output, plan),
    }
}

fn json_group_value(group: &Group) -> Value {
    let mut item = Row::new();
    item.insert("groups".to_string(), Value::Object(group.groups.clone()));
    item.insert(
        "aggregates".to_string(),
        Value::Object(group.aggregates.clone()),
    );
    item.insert(
        "rows".to_string(),
        Value::Array(
            group
                .rows
                .iter()
                .cloned()
                .map(Value::Object)
                .collect::<Vec<_>>(),
        ),
    );
    Value::Object(item)
}

fn lower_mreg_doc(output: &OutputResult, plan: &RenderPlan) -> Doc {
    match &output.items {
        OutputItems::Rows(rows) if rows.is_empty() => Doc::default(),
        OutputItems::Rows(rows) if plan.mreg_layout == MregLayout::Cards => {
            let mut blocks = Vec::new();
            for (index, row) in rows.iter().enumerate() {
                if index > 0 {
                    push_blank_lines(&mut blocks, 1);
                }
                let mut record = row.clone();
                for rule in &output.meta.display_rules {
                    if let DisplayRule::Notice { field } = rule
                        && let Some(value) = record.shift_remove(field)
                    {
                        blocks.push(Block::Paragraph(ParagraphBlock {
                            text: display_value(&value),
                            indent: 0,
                            inline_markup: false,
                        }));
                    }
                }
                blocks.extend(mreg_record_blocks(
                    &record,
                    &output.meta.key_index,
                    &plan.records[index],
                ));
            }
            Doc { blocks }
        }
        _ => lower_table_doc(output, plan),
    }
}

fn lower_value_blocks(value: &Value, table: Option<&PlannedTable>) -> Vec<Block> {
    match value {
        Value::Null => Vec::new(),
        Value::Bool(_) | Value::Number(_) | Value::String(_) => {
            vec![Block::Paragraph(ParagraphBlock {
                text: display_value(value),
                indent: 0,
                inline_markup: false,
            })]
        }
        Value::Array(items) => {
            if let Some(table) = table {
                return vec![Block::Table(table_from_plan(table))];
            }
            vec![Block::List(ListBlock {
                items: items.iter().map(display_value).collect(),
                indent: 0,
                inline_markup: false,
                auto_grid: false,
            })]
        }
        Value::Object(map) => vec![Block::KeyValue(key_value_from_map(map, &[]))],
    }
}

fn row_value_displays(row: &Row, key_order: &[String]) -> Vec<String> {
    ordered_keys(key_order, row.keys())
        .filter_map(|key| row.get(key))
        .flat_map(|value| match value {
            Value::Array(items) => items.iter().collect::<Vec<_>>(),
            Value::Null => Vec::new(),
            value => vec![value],
        })
        .map(display_value)
        .collect()
}

fn extend_with_blank_between(blocks: &mut Vec<Block>, mut extra: Vec<Block>) {
    if extra.is_empty() {
        return;
    }
    if !blocks.is_empty() {
        blocks.push(Block::Blank);
    }
    blocks.append(&mut extra);
}

fn payload_entry_value_as_entry(value: &Value) -> Option<crate::core::document::DocumentEntry> {
    let map = value.as_object()?;
    if map
        .keys()
        .any(|key| !matches!(key.as_str(), "name" | "description"))
    {
        return None;
    }
    Some(crate::core::document::DocumentEntry {
        name: map.get("name")?.as_str()?.to_string(),
        description: map
            .get("description")
            .and_then(Value::as_str)
            .unwrap_or("")
            .to_string(),
    })
}

fn help_layout_blocks_from_value(
    value: &Value,
    indent: usize,
    table: Option<&PlannedTable>,
) -> Vec<Block> {
    match value {
        Value::Object(map) if should_render_as_key_value_group(map) => {
            vec![Block::KeyValue(key_value_from_document_data_map(
                map, indent,
            ))]
        }
        Value::Object(map) => vec![Block::KeyValue(key_value_from_map(map, &[]))],
        Value::Array(items) if items.is_empty() => Vec::new(),
        Value::Array(items) => {
            if let Some(entries) = items
                .iter()
                .map(payload_entry_value_as_entry)
                .collect::<Option<Vec<_>>>()
            {
                vec![Block::Entries(entries_block_with_defaults(
                    &entries, indent, None,
                ))]
            } else if items.iter().all(is_scalar_like) {
                vec![Block::List(ListBlock {
                    items: items.iter().map(display_value).collect(),
                    indent,
                    inline_markup: true,
                    auto_grid: true,
                })]
            } else if let Some(table) = table {
                vec![Block::Table(table_from_plan(table))]
            } else {
                vec![Block::List(ListBlock {
                    items: items.iter().map(display_value).collect(),
                    indent: 0,
                    inline_markup: false,
                    auto_grid: false,
                })]
            }
        }
        Value::Null => Vec::new(),
        Value::Bool(_) | Value::Number(_) | Value::String(_) => {
            vec![Block::Paragraph(ParagraphBlock {
                text: display_value(value),
                indent,
                inline_markup: true,
            })]
        }
    }
}

fn push_blank_lines(blocks: &mut Vec<Block>, count: usize) {
    if blocks.is_empty() {
        return;
    }
    blocks.extend((0..count).map(|_| Block::Blank));
}

fn key_value_from_map(map: &Map<String, Value>, preferred_keys: &[String]) -> KeyValueBlock {
    KeyValueBlock {
        rows: ordered_keys(preferred_keys, map.keys())
            .filter_map(|key| {
                map.get(key).map(|value| KeyValueRow {
                    value_style: None,
                    key: key.clone(),
                    value: lower_key_value_value(value, false),
                    indent: None,
                    gap: None,
                })
            })
            .collect(),
    }
}

#[derive(Debug, Default)]
struct MregRecordBuilder {
    blocks: Vec<Block>,
    pending_rows: Vec<KeyValueRow>,
}

impl MregRecordBuilder {
    fn push_row(&mut self, key: &str, value: KeyValueValue, depth: usize) {
        self.pending_rows.push(KeyValueRow {
            value_style: None,
            key: key.to_string(),
            value,
            indent: (depth > 0).then(|| " ".repeat(depth * 2)),
            gap: None,
        });
    }

    fn push_table(&mut self, key: &str, item_count: usize, mut table: TableBlock, depth: usize) {
        self.flush();
        table.nested = true;
        self.blocks.push(Block::Paragraph(ParagraphBlock {
            text: section_title(key, item_count),
            indent: depth * 2,
            inline_markup: false,
        }));
        self.blocks.push(Block::Section(SectionBlock {
            title: None,
            title_chrome: SectionTitleChrome::Plain,
            body_indent: (depth + 1) * 2,
            inline_title_suffix: None,
            trailing_newline: false,
            blocks: vec![Block::Table(table)],
        }));
    }

    fn flush(&mut self) {
        if self.pending_rows.is_empty() {
            return;
        }
        self.blocks.push(Block::KeyValue(KeyValueBlock {
            rows: std::mem::take(&mut self.pending_rows),
        }));
    }
}

fn mreg_record_blocks(
    map: &Map<String, Value>,
    preferred_keys: &[String],
    layout: &RecordLayout,
) -> Vec<Block> {
    let mut builder = MregRecordBuilder::default();
    for key in ordered_keys(preferred_keys, map.keys()) {
        if let Some(value) = map.get(key)
            && let Some((_, field)) = layout.iter().find(|(name, _)| name == key)
        {
            visit_mreg_record_field(&mut builder, key, value, 0, field);
        }
    }
    builder.flush();
    builder.blocks
}

fn visit_mreg_record_field(
    builder: &mut MregRecordBuilder,
    key: &str,
    value: &Value,
    depth: usize,
    layout: &FieldLayout,
) {
    match layout {
        FieldLayout::Table(table) => {
            builder.push_table(key, table.data.rows.len(), table_from_plan(table), depth)
        }
        FieldLayout::Record(fields) => {
            builder.push_row(key, KeyValueValue::Empty, depth);
            for (nested_key, layout) in fields {
                if let Some(nested) = value.get(nested_key) {
                    visit_mreg_record_field(builder, nested_key, nested, depth + 1, layout);
                }
            }
        }
        FieldLayout::Value => builder.push_row(key, lower_key_value_value(value, false), depth),
    }
}

fn lower_key_value_value(value: &Value, null_as_empty: bool) -> KeyValueValue {
    match value {
        Value::Null if null_as_empty => KeyValueValue::Empty,
        Value::Null => KeyValueValue::Scalar("null".to_string()),
        Value::Bool(_) | Value::Number(_) | Value::String(_) => {
            KeyValueValue::Scalar(display_value(value))
        }
        Value::Array(items) => KeyValueValue::Array(
            items
                .iter()
                .map(|item| lower_key_value_value(item, false))
                .collect(),
        ),
        Value::Object(map) => KeyValueValue::Object(key_value_from_map(map, &[]).rows),
    }
}

fn summary_from_group(group: &Group, preferred_keys: &[String]) -> Vec<KeyValueRow> {
    let mut summary = Vec::new();
    for key in ordered_keys(
        preferred_keys,
        group.groups.keys().chain(group.aggregates.keys()),
    ) {
        if let Some(value) = group.groups.get(key).or_else(|| group.aggregates.get(key)) {
            summary.push(KeyValueRow {
                value_style: None,
                key: key.clone(),
                value: lower_key_value_value(value, false),
                indent: None,
                gap: None,
            });
        }
    }
    summary
}

fn key_value_from_document_data_map(map: &Map<String, Value>, indent: usize) -> KeyValueBlock {
    KeyValueBlock {
        rows: map
            .iter()
            .map(|(key, value)| KeyValueRow {
                value_style: None,
                key: key.clone(),
                value: lower_key_value_value(value, true),
                indent: Some(" ".repeat(indent)),
                gap: None,
            })
            .collect(),
    }
}

fn entries_block_with_defaults(
    entries: &[crate::core::document::DocumentEntry],
    entry_indent: usize,
    entry_gap: Option<usize>,
) -> EntriesBlock {
    EntriesBlock {
        inline_markup: true,
        default_indent: " ".repeat(entry_indent),
        default_gap: entry_gap.map(|value| " ".repeat(value)),
        rows: entries.iter().map(entry_row).collect(),
    }
}

fn help_layout_title_chrome(layout: HelpLayout) -> SectionTitleChrome {
    match layout {
        HelpLayout::Full => SectionTitleChrome::Ruled,
        HelpLayout::Compact | HelpLayout::Minimal => SectionTitleChrome::Plain,
    }
}

fn entry_row(entry: &crate::core::document::DocumentEntry) -> EntryRow {
    EntryRow {
        key: entry.name.clone(),
        value: entry.description.clone(),
        indent_hint: None,
        gap_hint: None,
    }
}

fn should_render_as_key_value_group(map: &Map<String, Value>) -> bool {
    !map.is_empty()
        && map.values().all(|value| match value {
            Value::Null | Value::Bool(_) | Value::Number(_) | Value::String(_) => true,
            Value::Array(items) => items.iter().all(is_scalar_like),
            Value::Object(_) => false,
        })
}

fn is_scalar_like(value: &Value) -> bool {
    matches!(
        value,
        Value::Null | Value::Bool(_) | Value::Number(_) | Value::String(_)
    )
}

fn table_from_plan(table: &PlannedTable) -> TableBlock {
    TableBlock {
        widths: table.widths.clone(),
        headers: table.data.headers.clone(),
        rows: table.data.rows.clone(),
        column_align: table.data.column_align.clone(),
        cell_styles: Vec::new(),
        summary: Vec::new(),
        nested: false,
    }
}
