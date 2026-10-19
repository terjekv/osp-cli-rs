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

use std::collections::BTreeSet;

use serde_json::{Map, Value};

use super::doc::{
    Block, Doc, EntriesBlock, EntryRow, JsonBlock, KeyValueBlock, KeyValueRow, KeyValueValue,
    ListBlock, ParagraphBlock, SectionBlock, SectionTitleChrome, TableBlock,
};
use super::plan::RenderPlan;
use super::settings::HelpLayout;
use super::text::{display_width, sanitize_human_text};
use super::visible_inline_text;
use crate::core::document::{DocBlock, Document};
use crate::core::output::OutputFormat;
use crate::core::output_model::{
    ColumnAlignment, Group, OutputDocument, OutputItems, OutputResult, group_rows,
    output_items_to_rows,
};
use crate::core::row::Row;

pub fn lower_output(output: &OutputResult, plan: &RenderPlan) -> Doc {
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

    // Display limits shorten only what a person reads; JSON keeps every row.
    let limited;
    let (output, shortened) = match output.meta.display_limit {
        Some(limit) if plan.format != OutputFormat::Json => {
            match limit_for_display(output, limit) {
                Some((result, total)) => {
                    limited = result;
                    (&limited, Some((limit, total)))
                }
                None => (output, None),
            }
        }
        _ => (output, None),
    };

    if output.meta.document_rows
        && matches!(
            plan.format,
            OutputFormat::Auto | OutputFormat::Guide | OutputFormat::Markdown
        )
    {
        let mut doc = match output.document.as_ref().and_then(OutputDocument::semantic) {
            Some(document) => lower_document(document, plan.format),
            None => lower_document_rows(output.as_rows().unwrap_or_default(), plan.format),
        };
        sanitize_doc(&mut doc);
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
        let mut doc = Doc {
            blocks: vec![Block::Paragraph(ParagraphBlock {
                text: items.join("\n"),
                indent: 0,
                inline_markup: false,
            })],
        };
        sanitize_doc(&mut doc);
        return doc;
    }

    // Human projection is deliberately downstream of filtering and JSON output.
    let style_source = output;
    let display;
    let output = if plan.format != OutputFormat::Json
        && (output.meta.display_columns.is_some()
            || !output.meta.unix_timestamp_columns.is_empty()
            || !output.meta.display_rules.is_empty())
    {
        display = display_output(output);
        &display
    } else {
        output
    };

    let mut doc = match plan.format {
        OutputFormat::Json => Doc {
            blocks: vec![Block::Json(JsonBlock {
                text: serde_json::to_string_pretty(&json_value(output))
                    .unwrap_or_else(|_| "null".to_string()),
            })],
        },
        OutputFormat::Guide | OutputFormat::Table => lower_table_doc(output),
        OutputFormat::Markdown => lower_markdown_doc(output),
        OutputFormat::Value => lower_value_doc(output),
        OutputFormat::Mreg => lower_mreg_doc(output),
        OutputFormat::Auto | OutputFormat::Csv | OutputFormat::Tsv => Doc::default(),
    };
    if plan.settings.color && plan.format != OutputFormat::Json {
        apply_value_styles(&mut doc.blocks, style_source);
        apply_warning_styles(&mut doc.blocks, style_source, &plan.settings);
    }
    if plan.format != OutputFormat::Json {
        sanitize_doc(&mut doc);
    }
    if plan.format != OutputFormat::Json {
        doc.blocks
            .extend(output.meta.display_notes.iter().map(|text| {
                Block::Paragraph(ParagraphBlock {
                    text: sanitize_human_text(text),
                    indent: 0,
                    inline_markup: false,
                })
            }));
    }
    if let Some((shown, total)) = shortened {
        doc.blocks.push(Block::Paragraph(ParagraphBlock {
            text: format!("Showing {shown} of {total} rows. Pipes and --json see every row."),
            indent: 0,
            inline_markup: false,
        }));
    }
    doc
}

pub(crate) fn lower_document(document: &Document, format: OutputFormat) -> Doc {
    Doc {
        blocks: lower_document_blocks(
            &document.blocks,
            DocumentLayout::Direct { format },
            false,
            false,
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
                body.extend(lower_value_blocks(&Value::Object(data)));
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
        format: OutputFormat,
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
) -> Vec<Block> {
    let (format, help) = match layout {
        DocumentLayout::Direct { format } => (format, None),
        DocumentLayout::Help { plan, layout } => (OutputFormat::Auto, Some((plan, layout))),
    };
    let mut lowered = Vec::new();
    for block in blocks {
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
                Some(_) => help_layout_blocks_from_value(value, if inside { 0 } else { 2 }),
                None => lower_value_blocks(value),
            },
        };
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

pub(crate) fn sanitize_doc(doc: &mut Doc) {
    sanitize_blocks(&mut doc.blocks);
}

fn sanitize_blocks(blocks: &mut [Block]) {
    for block in blocks {
        match block {
            Block::Blank | Block::Rule | Block::Json(_) => {}
            Block::Paragraph(paragraph) => {
                paragraph.text = sanitize_human_text(&paragraph.text);
            }
            Block::Section(section) => {
                if let Some(title) = &mut section.title {
                    *title = sanitize_human_text(title);
                }
                if let Some(suffix) = &mut section.inline_title_suffix {
                    *suffix = sanitize_human_text(suffix);
                }
                sanitize_blocks(&mut section.blocks);
            }
            Block::Table(table) => {
                for row in &mut table.summary {
                    sanitize_key_value_row(row);
                }
                for header in &mut table.headers {
                    *header = sanitize_human_text(header);
                }
                for row in &mut table.rows {
                    for cell in row {
                        *cell = sanitize_human_text(cell);
                    }
                }
            }
            Block::Entries(entries) => {
                entries.default_indent = sanitize_human_text(&entries.default_indent);
                if let Some(gap) = &mut entries.default_gap {
                    *gap = sanitize_human_text(gap);
                }
                for row in &mut entries.rows {
                    row.key = sanitize_human_text(&row.key);
                    row.value = sanitize_human_text(&row.value);
                    if let Some(indent) = &mut row.indent_hint {
                        *indent = sanitize_human_text(indent);
                    }
                    if let Some(gap) = &mut row.gap_hint {
                        *gap = sanitize_human_text(gap);
                    }
                }
            }
            Block::KeyValue(key_value) => {
                for row in &mut key_value.rows {
                    sanitize_key_value_row(row);
                }
            }
            Block::List(list) => {
                for item in &mut list.items {
                    *item = sanitize_human_text(item);
                }
            }
        }
    }
}

/// `key (N):` for several rows; a single row needs no count.
fn section_title(key: &str, count: usize) -> String {
    if count == 1 {
        format!("{key}:")
    } else {
        format!("{key} ({count}):")
    }
}

fn sanitize_key_value_row(row: &mut KeyValueRow) {
    row.key = sanitize_human_text(&row.key);
    if let Some(indent) = &mut row.indent {
        *indent = sanitize_human_text(indent);
    }
    if let Some(gap) = &mut row.gap {
        *gap = sanitize_human_text(gap);
    }
    sanitize_key_value(&mut row.value);
}

fn sanitize_key_value(value: &mut KeyValueValue) {
    match value {
        KeyValueValue::Empty => {}
        KeyValueValue::Scalar(value) => *value = sanitize_human_text(value),
        KeyValueValue::Array(values) => {
            for value in values {
                sanitize_key_value(value);
            }
        }
        KeyValueValue::Object(rows) => {
            for row in rows {
                sanitize_key_value_row(row);
            }
        }
    }
}

/// Keep a base-2^128 carry so even a full IPv6 range has an exact human total.
fn address_range_total(ranges: &Value, start: &str, end: &str) -> Option<String> {
    use std::net::IpAddr;
    let (mut high, mut low) = (0u128, 0u128);
    for range in ranges.as_array()? {
        let first = display_path(range, start)?
            .as_str()?
            .parse::<IpAddr>()
            .ok()?;
        let last = display_path(range, end)?.as_str()?.parse::<IpAddr>().ok()?;
        let (first, last) = match (first, last) {
            (IpAddr::V4(first), IpAddr::V4(last)) => {
                (u128::from(u32::from(first)), u128::from(u32::from(last)))
            }
            (IpAddr::V6(first), IpAddr::V6(last)) => (u128::from(first), u128::from(last)),
            _ => return None,
        };
        let (sum, carry) = low.overflowing_add(last.checked_sub(first)?);
        let (sum, inclusive_carry) = sum.overflowing_add(1);
        high = high.checked_add(u128::from(carry) + u128::from(inclusive_carry))?;
        low = sum;
    }
    Some(match (high, low) {
        (0, count) => count.to_string(),
        (1, 0) => "2^128".into(),
        (1, count) => format!("2^128 + {count}"),
        (multiple, 0) => format!("{multiple} × 2^128"),
        (multiple, count) => format!("{multiple} × 2^128 + {count}"),
    })
}

fn display_output(output: &OutputResult) -> OutputResult {
    let mut display = output.clone();
    if let OutputItems::Rows(rows) = &mut display.items {
        for row in rows.iter_mut() {
            let mut original = Value::Object(std::mem::take(row));
            let raw = original.clone();
            for rule in &output.meta.display_rules {
                use crate::core::output_model::DisplayRule;
                match rule {
                    DisplayRule::AddressRangeTotal {
                        ranges,
                        start,
                        end,
                        field,
                    } => {
                        if let Some(total) = display_path(&raw, ranges)
                            .and_then(|ranges| address_range_total(ranges, start, end))
                            && let Some(map) = original.as_object_mut()
                        {
                            map.insert(field.clone(), Value::String(total));
                        }
                    }
                    DisplayRule::AddressCountRemaining {
                        network,
                        used,
                        field,
                    } => {
                        if let Some(text) = display_path(&raw, network).and_then(Value::as_str)
                            && let Some((address, prefix)) = text.split_once('/')
                            && let Ok(address) = address.parse::<std::net::IpAddr>()
                            && let Ok(prefix) = prefix.parse::<u32>()
                            && let Some(bits) =
                                (if address.is_ipv4() { 32u32 } else { 128 }).checked_sub(prefix)
                            && let Some(used) = display_path(&raw, used).and_then(Value::as_u64)
                            && let Some(map) = original.as_object_mut()
                        {
                            let remaining = if bits > 64 {
                                Some(format!("2^{bits} − {used}"))
                            } else {
                                (1u128 << bits)
                                    .checked_sub(u128::from(used))
                                    .map(|count| count.to_string())
                            };
                            if let Some(remaining) = remaining {
                                map.insert(field.clone(), Value::String(remaining));
                            }
                        }
                    }
                    DisplayRule::NullText { field, text } => {
                        if let Some(value) = display_path_mut(&mut original, field)
                            && value.is_null()
                        {
                            *value = Value::String(text.clone());
                        }
                    }
                    DisplayRule::Date { field, pattern } => {
                        if let Some(value) = display_path_mut(&mut original, field)
                            && let Some(text) = value.as_str()
                            && let Some(formatted) = format_date(text, pattern)
                        {
                            *value = Value::String(formatted);
                        }
                    }
                    DisplayRule::Decimal { field, precision } => {
                        if let Some(value) = display_path_mut(&mut original, field)
                            && let Some(number) = value.as_f64()
                        {
                            *value =
                                Value::String(format!("{:.*}", (*precision).min(4096), number));
                        }
                    }
                    DisplayRule::TextLimit { field, length } => {
                        if let Some(value) = display_path_mut(&mut original, field)
                            && !value.is_null()
                        {
                            *value =
                                Value::String(display_value(value).chars().take(*length).collect());
                        }
                    }
                    DisplayRule::Compose { field, parts } => {
                        if let Some(value) = display_path_mut(&mut original, field)
                            && (value.is_object() || value.is_array())
                        {
                            use crate::core::output_model::DisplayPart;
                            let text: Option<String> = parts
                                .iter()
                                .map(|part| match part {
                                    DisplayPart::Literal { text } => Some(text.clone()),
                                    DisplayPart::Field { field } => {
                                        display_path(value, field).map(display_value)
                                    }
                                })
                                .collect();
                            if let Some(text) = text {
                                *value = Value::String(text);
                            }
                        }
                    }
                    DisplayRule::HideArrayItemsWhenTrue { field, when } => {
                        if let Some(Value::Array(items)) = display_path_mut(&mut original, field) {
                            items.retain(|item| {
                                display_path(item, when).and_then(Value::as_bool) != Some(true)
                            });
                        }
                    }
                    DisplayRule::SuffixWhenTrue {
                        field,
                        when,
                        suffix,
                    } if display_path(&raw, when).and_then(Value::as_bool) == Some(true) => {
                        if let Some(Value::String(text)) = display_path_mut(&mut original, field) {
                            text.push_str(suffix);
                        }
                    }
                    DisplayRule::BlankWhenPresent { field, when }
                        if display_path(&raw, when).is_some_and(|value| !value.is_null()) =>
                    {
                        if let Some(value) = display_path_mut(&mut original, field) {
                            *value = Value::Null;
                        }
                    }
                    DisplayRule::Bytes { field } => {
                        if let Some(value) = display_path_mut(&mut original, field)
                            && let Some(bytes) = value.as_u64()
                        {
                            *value = Value::String(super::format_binary_bytes(bytes));
                        }
                    }
                    DisplayRule::RelativeTimestamp { field } => {
                        if let Some(value) = display_path_mut(&mut original, field) {
                            let formatted = match value {
                                Value::String(text) => super::format_relative_timestamp(text),
                                Value::Number(number) => {
                                    number.as_i64().map(super::format_relative_unix_timestamp)
                                }
                                _ => None,
                            };
                            if let Some(formatted) = formatted {
                                *value = Value::String(formatted);
                            }
                        }
                    }
                    _ => {}
                }
            }
            for path in &output.meta.unix_timestamp_columns {
                if let Some(value) = display_path_mut(&mut original, path)
                    && let Some(seconds) = value.as_i64()
                    && let Some(time) = super::format_local_unix_timestamp(seconds)
                {
                    *value = Value::String(time);
                }
            }
            *row = if let Some(columns) = &output.meta.display_columns {
                let labels = display_labels(output, columns);
                columns
                    .iter()
                    .enumerate()
                    .map(|(index, path)| {
                        let value = display_path(&original, path);
                        let label = &labels[index];
                        (label.clone(), value.cloned().unwrap_or(Value::Null))
                    })
                    .collect()
            } else {
                original.as_object().cloned().unwrap_or_default()
            };
        }
        if let Some(columns) = &output.meta.display_columns {
            display.meta.key_index = display_labels(output, columns);
        }
    }
    display
}

fn format_date(text: &str, pattern: &str) -> Option<String> {
    use chrono::{NaiveDate, NaiveDateTime};
    let date = NaiveDateTime::parse_from_str(text, "%Y-%m-%dT%H:%M:%S")
        .ok()
        .or_else(|| {
            chrono::DateTime::parse_from_rfc3339(text)
                .ok()
                .map(|date| date.naive_local())
        })
        .or_else(|| {
            NaiveDate::parse_from_str(text, "%Y-%m-%d")
                .ok()?
                .and_hms_opt(0, 0, 0)
        })?;
    let mut result = String::new();
    use std::fmt::Write;
    write!(&mut result, "{}", date.format(pattern)).ok()?;
    Some(result)
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

fn display_labels(output: &OutputResult, columns: &[String]) -> Vec<String> {
    let labels = output
        .meta
        .display_column_labels
        .as_deref()
        .filter(|labels| labels.len() == columns.len())
        .unwrap_or(columns);
    let mut used = BTreeSet::new();
    labels
        .iter()
        .zip(columns)
        .map(|(label, path)| {
            let mut heading = if labels.iter().filter(|other| *other == label).count() > 1 {
                format!("{label} ({path})")
            } else {
                label.clone()
            };
            let base = heading.clone();
            let mut count = 2;
            while !used.insert(heading.clone()) {
                heading = format!("{base} ({count})");
                count += 1;
            }
            heading
        })
        .collect()
}

pub(super) fn display_path<'a>(value: &'a Value, path: &str) -> Option<&'a Value> {
    value.get(path).or_else(|| {
        if path.starts_with('/') {
            value.pointer(path)
        } else {
            path.split('.').try_fold(value, |value, key| value.get(key))
        }
    })
}

fn display_path_mut<'a>(value: &'a mut Value, path: &str) -> Option<&'a mut Value> {
    if value.get(path).is_some() {
        value.get_mut(path)
    } else if path.starts_with('/') {
        value.pointer_mut(path)
    } else {
        path.split('.')
            .try_fold(value, |value, key| value.get_mut(key))
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

/// Shortens a long row set, or the long lists inside a one-record document,
/// to `limit` entries. Returns the shortened result and the longest original
/// length, or `None` when nothing exceeds the limit.
fn limit_for_display(output: &OutputResult, limit: usize) -> Option<(OutputResult, usize)> {
    let OutputItems::Rows(rows) = &output.items else {
        return None;
    };
    let mut limited = output.clone();
    let OutputItems::Rows(limited_rows) = &mut limited.items else {
        return None;
    };
    let mut total = 0;
    let record_document = rows.len() == 1
        && matches!(
            output.document,
            Some(OutputDocument::Json(Value::Object(_)))
        );
    if rows.len() > limit && !record_document {
        total = rows.len();
        limited_rows.truncate(limit);
    } else if let [record] = limited_rows.as_mut_slice() {
        fn shorten(value: &mut Value, limit: usize, total: &mut usize) {
            match value {
                Value::Array(items) => {
                    if items.len() > limit {
                        *total = (*total).max(items.len());
                        items.truncate(limit);
                    }
                    for item in items {
                        shorten(item, limit, total);
                    }
                }
                Value::Object(fields) => {
                    for value in fields.values_mut() {
                        shorten(value, limit, total);
                    }
                }
                _ => {}
            }
        }
        if let Some(columns) = &output.meta.display_columns {
            // Hidden sections must not contribute to the visible row count.
            let mut value = Value::Object(std::mem::take(record));
            for column in columns {
                if let Some(value) = display_path_mut(&mut value, column) {
                    shorten(value, limit, &mut total);
                }
            }
            if let Value::Object(fields) = value {
                *record = fields;
            }
        } else {
            for value in record.values_mut() {
                shorten(value, limit, &mut total);
            }
        }
    }
    (total > 0).then_some((limited, total))
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

fn lower_table_doc(output: &OutputResult) -> Doc {
    match &output.items {
        OutputItems::Rows(rows) if rows.is_empty() => Doc::default(),
        OutputItems::Rows(rows) => Doc {
            blocks: vec![Block::Table(table_from_rows(
                rows,
                &output.meta.key_index,
                &output.meta.column_align,
            ))],
        },
        OutputItems::Groups(groups) => {
            let mut blocks = Vec::new();
            for (index, group) in groups.iter().enumerate() {
                if index > 0 {
                    blocks.push(Block::Blank);
                }
                blocks.push(Block::Table(table_from_group(
                    group,
                    &output.meta.key_index,
                    &output.meta.column_align,
                )));
            }
            Doc { blocks }
        }
    }
}

fn lower_markdown_doc(output: &OutputResult) -> Doc {
    match &output.items {
        OutputItems::Rows(rows) if rows.len() == 1 && row_has_nested_values(&rows[0]) => Doc {
            blocks: vec![Block::KeyValue(key_value_from_map(
                &rows[0],
                &output.meta.key_index,
            ))],
        },
        _ => lower_table_doc(output),
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

fn lower_mreg_doc(output: &OutputResult) -> Doc {
    match &output.items {
        OutputItems::Rows(rows) if rows.is_empty() => Doc::default(),
        OutputItems::Rows(rows) if rows.len() == 1 && row_has_table_arrays(&rows[0]) => Doc {
            blocks: mreg_record_blocks(&rows[0], &output.meta.key_index),
        },
        OutputItems::Rows(rows) if rows.len() == 1 => Doc {
            blocks: vec![Block::KeyValue(key_value_from_map(
                &rows[0],
                &output.meta.key_index,
            ))],
        },
        _ => lower_table_doc(output),
    }
}

fn lower_value_blocks(value: &Value) -> Vec<Block> {
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
            if let Some(table) = table_from_value_array(items) {
                return vec![Block::Table(table)];
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

fn ordered_keys<'a>(
    preferred_keys: &'a [String],
    keys: impl IntoIterator<Item = &'a String>,
) -> impl Iterator<Item = &'a String> {
    let mut seen = BTreeSet::new();
    preferred_keys
        .iter()
        .chain(keys)
        .filter(move |key| seen.insert(*key))
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

fn help_layout_blocks_from_value(value: &Value, indent: usize) -> Vec<Block> {
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
            } else if let Some(table) = table_from_value_array(items) {
                vec![Block::Table(table)]
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

fn mreg_record_blocks(map: &Map<String, Value>, preferred_keys: &[String]) -> Vec<Block> {
    let mut builder = MregRecordBuilder::default();
    for key in ordered_keys(preferred_keys, map.keys()) {
        if let Some(value) = map.get(key) {
            visit_mreg_record_field(&mut builder, key, value, 0);
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
) {
    match value {
        Value::Array(items) => {
            if let Some(table) = table_from_nonempty_object_array(items) {
                builder.push_table(key, items.len(), table, depth);
            } else {
                builder.push_row(key, lower_key_value_value(value, false), depth);
            }
        }
        Value::Object(map) if object_has_table_arrays(map) => {
            builder.push_row(key, KeyValueValue::Empty, depth);
            for (nested_key, nested) in map {
                visit_mreg_record_field(builder, nested_key, nested, depth + 1);
            }
        }
        other => builder.push_row(key, lower_key_value_value(other, false), depth),
    }
}

fn row_has_table_arrays(row: &Row) -> bool {
    row.values().any(value_has_table_arrays)
}

fn object_has_table_arrays(map: &Map<String, Value>) -> bool {
    map.values().any(value_has_table_arrays)
}

fn value_has_table_arrays(value: &Value) -> bool {
    match value {
        Value::Array(items) => table_from_nonempty_object_array(items).is_some(),
        Value::Object(map) => object_has_table_arrays(map),
        Value::Null | Value::Bool(_) | Value::Number(_) | Value::String(_) => false,
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

fn row_has_nested_values(row: &Row) -> bool {
    row.values()
        .any(|value| matches!(value, Value::Array(_) | Value::Object(_)))
}

fn table_from_rows(
    rows: &[Row],
    key_index: &[String],
    column_align: &[ColumnAlignment],
) -> TableBlock {
    let all_headers = headers_for_rows(rows, key_index);
    let all_rows = rows
        .iter()
        .map(|row| {
            all_headers
                .iter()
                .map(|header| row.get(header).map(display_value).unwrap_or_default())
                .collect::<Vec<_>>()
        })
        .collect::<Vec<_>>();
    let mut visible_columns = (0..all_headers.len())
        .filter(|index| {
            all_rows.is_empty()
                || all_rows.iter().any(|row| {
                    row.get(*index)
                        .is_some_and(|value| !value.trim().is_empty())
                })
        })
        .collect::<Vec<_>>();
    if visible_columns.is_empty() && !all_headers.is_empty() {
        // A nonempty row still represents a result even when every value is blank.
        visible_columns.push(0);
    }
    let all_align = normalize_table_alignment(all_headers.len(), column_align);
    TableBlock {
        cell_styles: Vec::new(),
        summary: Vec::new(),
        headers: visible_columns
            .iter()
            .map(|index| all_headers[*index].clone())
            .collect(),
        rows: all_rows
            .into_iter()
            .map(|row| {
                visible_columns
                    .iter()
                    .map(|index| row[*index].clone())
                    .collect()
            })
            .collect(),
        column_align: visible_columns
            .iter()
            .map(|index| all_align[*index])
            .collect(),
        nested: false,
    }
}

fn table_from_group(
    group: &Group,
    preferred_keys: &[String],
    column_align: &[ColumnAlignment],
) -> TableBlock {
    let merged_rows = group_rows(group);
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
    let mut table = table_from_rows(&merged_rows, preferred_keys, column_align);
    table.summary = summary;
    table
}

fn headers_for_rows(rows: &[Row], preferred_keys: &[String]) -> Vec<String> {
    ordered_keys(preferred_keys, rows.iter().flat_map(Row::keys))
        .cloned()
        .collect()
}

fn table_from_value_array(items: &[Value]) -> Option<TableBlock> {
    let mut rows = Vec::new();
    for item in items {
        let Value::Object(map) = item else {
            return None;
        };
        rows.push(map.clone());
    }
    Some(table_from_rows(&rows, &[], &[]))
}

fn table_from_nonempty_object_array(items: &[Value]) -> Option<TableBlock> {
    if items.is_empty() {
        return None;
    }
    let table = table_from_value_array(items)?;
    (table.headers.len() <= 8).then_some(table)
}

fn normalize_table_alignment(
    header_count: usize,
    column_align: &[ColumnAlignment],
) -> Vec<ColumnAlignment> {
    let mut normalized = vec![ColumnAlignment::Default; header_count];
    for (index, alignment) in column_align.iter().copied().enumerate().take(header_count) {
        normalized[index] = alignment;
    }
    normalized
}

fn display_value(value: &Value) -> String {
    match value {
        Value::Null => String::new(),
        Value::Bool(flag) => flag.to_string(),
        Value::Number(number) => number.to_string(),
        Value::String(text) => super::format_local_timestamp(text).unwrap_or_else(|| text.clone()),
        Value::Array(items) => items
            .iter()
            .map(display_value)
            .collect::<Vec<_>>()
            .join(", "),
        Value::Object(_) => serde_json::to_string(value).unwrap_or_else(|_| "null".to_string()),
    }
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
