//! Pure human data projection, shared by layout planning and lowering.

use super::settings::{RenderProfile, RenderSettings};
use super::text::sanitize_human_text;
use crate::core::document::{DocBlock, Document};
use crate::core::output::OutputFormat;
use crate::core::output_model::{ColumnAlignment, OutputDocument, OutputItems, OutputResult};
use crate::core::row::Row;
use serde_json::Value;
use std::collections::BTreeSet;

pub(super) struct HumanProjection {
    pub output: OutputResult,
    pub source: OutputResult,
    pub shortened: Option<(usize, usize)>,
    pub tables: Vec<ProjectedTable>,
    pub records: Vec<RecordProjection>,
    pub document_tables: std::collections::BTreeMap<Vec<usize>, ProjectedTable>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct ProjectedTable {
    pub headers: Vec<String>,
    pub rows: Vec<Vec<String>>,
    pub column_align: Vec<ColumnAlignment>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct ValueProjection {
    pub table: Option<ProjectedTable>,
    pub fields: RecordProjection,
}

pub(super) type RecordProjection = Vec<(String, ValueProjection)>;

fn project_record(row: &Row) -> RecordProjection {
    row.iter()
        .map(|(key, value)| {
            (
                key.clone(),
                ValueProjection {
                    table: value.as_array().and_then(|items| project_array(items)),
                    fields: value.as_object().map(project_record).unwrap_or_default(),
                },
            )
        })
        .collect()
}

pub(super) fn project_array(items: &[Value]) -> Option<ProjectedTable> {
    let rows = items
        .iter()
        .map(|item| item.as_object().cloned())
        .collect::<Option<Vec<_>>>()?;
    Some(project_table(&rows, &[], &[]))
}

fn project_document_tables(
    blocks: &[DocBlock],
    path: &mut Vec<usize>,
    tables: &mut std::collections::BTreeMap<Vec<usize>, ProjectedTable>,
) {
    for (index, block) in blocks.iter().enumerate() {
        path.push(index);
        match block {
            DocBlock::Section { blocks, .. } => project_document_tables(blocks, path, tables),
            DocBlock::Data {
                value: Value::Array(items),
            } => {
                if let Some(table) = project_array(items) {
                    tables.insert(path.clone(), table);
                }
            }
            _ => {}
        }
        path.pop();
    }
}

pub(super) fn project(
    output: &OutputResult,
    format: OutputFormat,
    settings: &RenderSettings,
    profile: RenderProfile,
) -> HumanProjection {
    let mut display = if output.meta.document_rows {
        output.clone()
    } else {
        display_output(output, format)
    };
    let source = output.clone();
    // Derived display values must see every row and nested item.
    let shortened = if settings.runtime.stdout_is_tty && profile == RenderProfile::Normal {
        display.meta.display_limit.and_then(|limit| {
            limit_for_display(&display, limit).map(|(limited, total)| {
                display = limited;
                (limit, total)
            })
        })
    } else {
        None
    };
    sanitize_output(&mut display);
    let tables = match &display.items {
        OutputItems::Rows(rows) => vec![project_table(
            rows,
            &display.meta.key_index,
            &display.meta.column_align,
        )],
        OutputItems::Groups(groups) => groups
            .iter()
            .map(|group| {
                project_table(
                    &crate::core::output_model::group_rows(group),
                    &display.meta.key_index,
                    &display.meta.column_align,
                )
            })
            .collect(),
    };
    let records = display
        .as_rows()
        .unwrap_or_default()
        .iter()
        .map(project_record)
        .collect();
    let mut document_tables = std::collections::BTreeMap::new();
    if let Some(document) = display.document.as_ref().and_then(OutputDocument::semantic) {
        project_document_tables(&document.blocks, &mut Vec::new(), &mut document_tables);
    }
    HumanProjection {
        tables,
        records,
        document_tables,
        output: display,
        source,
        shortened,
    }
}

fn sanitize_value(value: &mut Value) {
    match value {
        Value::String(text) => *text = sanitize_human_text(text),
        Value::Array(items) => items.iter_mut().for_each(sanitize_value),
        Value::Object(map) => sanitize_row(map),
        _ => {}
    }
}

fn sanitize_row(row: &mut Row) {
    *row = std::mem::take(row)
        .into_iter()
        .map(|(key, mut value)| {
            sanitize_value(&mut value);
            (sanitize_human_text(&key), value)
        })
        .collect();
}

fn sanitize_document(document: &mut Document) {
    fn blocks(items: &mut [DocBlock]) {
        for block in items {
            match block {
                DocBlock::Section {
                    title,
                    blocks: body,
                    ..
                } => {
                    *title = sanitize_human_text(title);
                    blocks(body);
                }
                DocBlock::Paragraph { text } => *text = sanitize_human_text(text),
                DocBlock::Entries { entries } => {
                    for entry in entries {
                        entry.name = sanitize_human_text(&entry.name);
                        entry.description = sanitize_human_text(&entry.description);
                    }
                }
                DocBlock::List { items } => {
                    for item in items {
                        *item = sanitize_human_text(item);
                    }
                }
                DocBlock::Data { value } => sanitize_value(value),
            }
        }
    }
    blocks(&mut document.blocks);
}

fn sanitize_output(output: &mut OutputResult) {
    match &mut output.items {
        OutputItems::Rows(rows) => rows.iter_mut().for_each(sanitize_row),
        OutputItems::Groups(groups) => {
            for group in groups {
                sanitize_row(&mut group.groups);
                sanitize_row(&mut group.aggregates);
                group.rows.iter_mut().for_each(sanitize_row);
            }
        }
    }
    if let Some(OutputDocument::Semantic(document)) = &mut output.document {
        sanitize_document(document);
    }
    for key in &mut output.meta.key_index {
        *key = sanitize_human_text(key);
    }
    for text in &mut output.meta.display_notes {
        *text = sanitize_human_text(text);
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

fn display_output(output: &OutputResult, format: OutputFormat) -> OutputResult {
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
                    DisplayRule::Date { field, pattern } if format != OutputFormat::Value => {
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
                    DisplayRule::RelativeTimestamp { field } if format != OutputFormat::Value => {
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
            for path in output
                .meta
                .unix_timestamp_columns
                .iter()
                .filter(|_| format != OutputFormat::Value)
            {
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
            display.meta.display_columns = Some(display.meta.key_index.clone());
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

pub(super) fn display_labels(output: &OutputResult, columns: &[String]) -> Vec<String> {
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

pub(super) fn ordered_keys<'a>(
    preferred_keys: &'a [String],
    keys: impl IntoIterator<Item = &'a String>,
) -> impl Iterator<Item = &'a String> {
    let mut seen = BTreeSet::new();
    preferred_keys
        .iter()
        .chain(keys)
        .filter(move |key| seen.insert(*key))
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

pub(super) fn project_table(
    rows: &[Row],
    key_index: &[String],
    column_align: &[ColumnAlignment],
) -> ProjectedTable {
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
    ProjectedTable {
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
    }
}

fn headers_for_rows(rows: &[Row], preferred_keys: &[String]) -> Vec<String> {
    ordered_keys(preferred_keys, rows.iter().flat_map(Row::keys))
        .cloned()
        .collect()
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

pub(super) fn display_value(value: &Value) -> String {
    match value {
        Value::Null => String::new(),
        Value::Bool(flag) => flag.to_string(),
        Value::Number(number) => number.to_string(),
        Value::String(text) => text.clone(),
        Value::Array(items) => items
            .iter()
            .map(display_value)
            .collect::<Vec<_>>()
            .join(", "),
        Value::Object(_) => serde_json::to_string(value).unwrap_or_else(|_| "null".to_string()),
    }
}
