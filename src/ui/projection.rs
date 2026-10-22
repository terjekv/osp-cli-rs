//! Pure human data projection, shared by layout planning and lowering.

use super::settings::ResolvedRenderSettings;
use super::style::{CellStyle, StyleToken};
use super::text::sanitize_human_text;
use crate::core::document::{DocBlock, Document};
use crate::core::output::OutputFormat;
use crate::core::output_model::{ColumnAlignment, OutputDocument, OutputItems, OutputResult};
use crate::core::row::Row;
use serde_json::Value;
use std::collections::{BTreeMap, BTreeSet};

pub(super) struct HumanProjection {
    pub output: OutputResult,
    pub source_row_count: Option<usize>,
    pub shortened: Option<(usize, usize)>,
    pub tables: Vec<ProjectedTable>,
    pub records: Vec<RecordProjection>,
    pub document_tables: std::collections::BTreeMap<Vec<usize>, ProjectedTable>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct ProjectedTable {
    pub list_items: BTreeMap<(usize, usize), Vec<String>>,
    pub summarized_cells: BTreeSet<(usize, usize)>,
    pub cell_styles: Vec<Vec<Option<CellStyle>>>,
    pub headers: Vec<String>,
    pub rows: Vec<Vec<String>>,
    pub column_align: Vec<ColumnAlignment>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct ValueProjection {
    pub style: Option<CellStyle>,
    pub table: Option<ProjectedTable>,
    pub fields: RecordProjection,
}

pub(super) type RecordProjection = Vec<(String, ValueProjection)>;

type FieldStyles = BTreeMap<Vec<String>, CellStyle>;

fn field_path(value: &Value, selector: &str) -> Vec<String> {
    if value.get(selector).is_some() {
        return vec![selector.into()];
    }
    if let Some(pointer) = selector.strip_prefix('/') {
        pointer
            .split('/')
            .map(|part| part.replace("~1", "/").replace("~0", "~"))
            .collect()
    } else {
        selector.split('.').map(str::to_owned).collect()
    }
}

fn swatch_spec(value: &Value) -> Option<&str> {
    let spec = value.as_str()?;
    (crate::core::style::is_valid_style_spec(spec)
        && crate::core::style::parse_style_spec(spec)
            .is_some_and(|style| style.foreground.is_some()))
    .then_some(spec)
}

fn apply_swatches(value: &mut Value, raw: &Value, rules: &[crate::core::DisplayRule]) {
    for rule in rules {
        if let crate::core::DisplayRule::Swatch { field } = rule
            && let Some(value) = display_path_mut(value, field)
        {
            *value = Value::String(
                if display_path(raw, field).and_then(swatch_spec).is_some() {
                    "██".into()
                } else {
                    String::new()
                },
            );
        }
    }
}

fn field_styles(value: &Value, rules: &[crate::core::DisplayRule]) -> FieldStyles {
    use crate::core::DisplayRule;
    let mut styles = FieldStyles::new();
    for rule in rules {
        if let DisplayRule::Command { field } = rule {
            styles.insert(
                field_path(value, field),
                CellStyle::Token(StyleToken::PromptCommand),
            );
        }
    }
    for rule in rules {
        if let DisplayRule::ValueStyle { field, bold } = rule
            && let Some(spec) = display_path(value, field).and_then(Value::as_str)
            && crate::core::style::is_valid_style_spec(spec)
        {
            styles.insert(
                field_path(value, field),
                CellStyle::Spec(if *bold {
                    format!("bold {spec}")
                } else {
                    spec.to_owned()
                }),
            );
        }
    }
    for rule in rules {
        if let DisplayRule::Swatch { field } = rule
            && let Some(spec) = display_path(value, field).and_then(swatch_spec)
        {
            styles.insert(field_path(value, field), CellStyle::Spec(spec.to_owned()));
        }
    }
    for rule in rules {
        if let DisplayRule::Warning { field } = rule {
            styles.insert(
                field_path(value, field),
                CellStyle::Token(StyleToken::Warning),
            );
        }
    }
    styles
}

fn project_record(row: &Row, path: &[String], styles: &FieldStyles) -> RecordProjection {
    row.iter()
        .map(|(key, value)| {
            let mut path = path.to_vec();
            path.push(key.clone());
            (
                key.clone(),
                ValueProjection {
                    style: styles.get(&path).cloned(),
                    table: value
                        .as_array()
                        .and_then(|items| project_array(items, &path, styles)),
                    fields: value
                        .as_object()
                        .map(|row| project_record(row, &path, styles))
                        .unwrap_or_default(),
                },
            )
        })
        .collect()
}

fn project_array(items: &[Value], path: &[String], styles: &FieldStyles) -> Option<ProjectedTable> {
    let rows = items
        .iter()
        .map(|item| item.as_object().cloned())
        .collect::<Option<Vec<_>>>()?;
    let mut table = project_table(&rows, &[], &[]);
    table.cell_styles = table
        .rows
        .iter()
        .enumerate()
        .map(|(index, _)| {
            table
                .headers
                .iter()
                .map(|key| {
                    let mut indexed_path = path.to_vec();
                    indexed_path.push(index.to_string());
                    indexed_path.push(key.clone());
                    styles
                        .get(&indexed_path)
                        .or_else(|| {
                            let mut field_path = path.to_vec();
                            field_path.push(key.clone());
                            styles.get(&field_path)
                        })
                        .cloned()
                })
                .collect()
        })
        .collect();
    Some(table)
}

fn style_table(table: &mut ProjectedTable, styles: &[FieldStyles]) {
    table.cell_styles = table
        .rows
        .iter()
        .enumerate()
        .map(|(index, _)| {
            table
                .headers
                .iter()
                .map(|header| {
                    styles
                        .get(index)
                        .and_then(|styles| styles.get(&vec![header.clone()]))
                        .cloned()
                })
                .collect()
        })
        .collect();
}

fn retain_array_styles(styles: &mut FieldStyles, path: &[String], retained: &[usize]) {
    *styles = std::mem::take(styles)
        .into_iter()
        .filter_map(|(mut address, style)| {
            if address.starts_with(path)
                && address.len() > path.len()
                && let Ok(index) = address[path.len()].parse::<usize>()
            {
                address[path.len()] = retained
                    .iter()
                    .position(|original| *original == index)?
                    .to_string();
            }
            Some((address, style))
        })
        .collect();
}

fn empty_value(value: &Value) -> bool {
    match value {
        Value::Null => true,
        Value::String(text) => text.is_empty(),
        Value::Array(items) => items.is_empty(),
        Value::Object(fields) => fields.is_empty(),
        _ => false,
    }
}

fn reduce_record(row: &mut Row, path: &[String], styles: &mut FieldStyles) {
    for (key, value) in row {
        let mut path = path.to_vec();
        path.push(key.clone());
        reduce_value(value, &path, styles);
    }
}

fn reduce_value(value: &mut Value, path: &[String], styles: &mut FieldStyles) {
    match value {
        Value::Object(row) => reduce_record(row, path, styles),
        Value::Array(items) => {
            for (index, item) in items.iter_mut().enumerate() {
                let mut path = path.to_vec();
                path.push(index.to_string());
                reduce_value(item, &path, styles);
            }
            if items.is_empty() || !items.iter().all(Value::is_object) {
                return;
            }
            let keys = items
                .iter()
                .filter_map(Value::as_object)
                .flat_map(|row| row.iter())
                .filter(|(_, value)| !empty_value(value))
                .map(|(key, _)| key.clone())
                .collect::<BTreeSet<_>>();
            for row in items.iter_mut().filter_map(Value::as_object_mut) {
                row.retain(|key, _| keys.contains(key));
            }
            if keys.len() == 1 {
                let key = keys.first().expect("one remaining field");
                for item in items {
                    *item = item
                        .as_object_mut()
                        .and_then(|row| row.remove(key))
                        .unwrap_or(Value::Null);
                }
                *styles = std::mem::take(styles)
                    .into_iter()
                    .map(|(mut address, style)| {
                        if let Some(suffix) = address.strip_prefix(path) {
                            let offset = if suffix
                                .first()
                                .is_some_and(|part| part.parse::<usize>().is_ok())
                            {
                                1
                            } else {
                                0
                            };
                            if suffix.get(offset) == Some(key) {
                                address.remove(path.len() + offset);
                            }
                        }
                        (address, style)
                    })
                    .collect();
            }
        }
        _ => {}
    }
}

fn reduce_output(
    output: &mut OutputResult,
    styles: &mut [FieldStyles],
    grouped_styles: &mut [Vec<FieldStyles>],
) {
    match &mut output.items {
        OutputItems::Rows(rows) => {
            for (index, row) in rows.iter_mut().enumerate() {
                reduce_record(
                    row,
                    &[],
                    styles.get_mut(index).unwrap_or(&mut FieldStyles::new()),
                );
            }
        }
        OutputItems::Groups(groups) => {
            for (index, group) in groups.iter_mut().enumerate() {
                reduce_record(&mut group.groups, &[], &mut FieldStyles::new());
                reduce_record(&mut group.aggregates, &[], &mut FieldStyles::new());
                for (row_index, row) in group.rows.iter_mut().enumerate() {
                    reduce_record(row, &[], &mut grouped_styles[index][row_index]);
                }
            }
        }
    }
    fn blocks(items: &mut [DocBlock]) {
        for block in items {
            match block {
                DocBlock::Section { blocks: body, .. } => blocks(body),
                DocBlock::Data { value } => reduce_value(value, &[], &mut FieldStyles::new()),
                _ => {}
            }
        }
    }
    if let Some(OutputDocument::Semantic(document)) = &mut output.document {
        blocks(&mut document.blocks);
    }
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
                if let Some(table) = project_array(items, &[], &FieldStyles::new()) {
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
    settings: &ResolvedRenderSettings,
) -> HumanProjection {
    let (mut display, mut styles) = if output.meta.document_rows {
        (output.clone(), Vec::new())
    } else {
        display_output(output, format, settings.color)
    };
    if format != OutputFormat::Value {
        localize_timestamps(&mut display);
    }
    let source_row_count = output.as_rows().map(<[Row]>::len);
    let mut grouped_styles = match &output.items {
        OutputItems::Groups(groups) => groups
            .iter()
            .map(|group| {
                crate::core::output_model::group_rows(group)
                    .iter()
                    .map(|row| {
                        field_styles(&Value::Object(row.clone()), &output.meta.display_rules)
                    })
                    .collect::<Vec<_>>()
            })
            .collect::<Vec<_>>(),
        _ => Vec::new(),
    };
    if settings.color
        && !matches!(format, OutputFormat::Value | OutputFormat::Markdown)
        && let OutputItems::Groups(groups) = &mut display.items
    {
        for row in groups.iter_mut().flat_map(|group| {
            std::iter::once(&mut group.groups)
                .chain(std::iter::once(&mut group.aggregates))
                .chain(group.rows.iter_mut())
        }) {
            let raw = Value::Object(std::mem::take(row));
            let mut value = raw.clone();
            apply_swatches(&mut value, &raw, &output.meta.display_rules);
            *row = value.as_object().cloned().unwrap_or_default();
        }
    }
    // Derived display values must see every row and nested item.
    let shortened = if settings.display_limits {
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
    // Style addresses follow the same sanitized field names as the data.
    for row in styles.iter_mut().chain(grouped_styles.iter_mut().flatten()) {
        *row = std::mem::take(row)
            .into_iter()
            .map(|(path, style)| {
                (
                    path.into_iter()
                        .map(|key| sanitize_human_text(&key))
                        .collect(),
                    style,
                )
            })
            .collect();
    }
    if format != OutputFormat::Value {
        reduce_output(&mut display, &mut styles, &mut grouped_styles);
    }
    let tables = match &display.items {
        OutputItems::Rows(rows) => {
            let mut table =
                project_table(rows, &display.meta.key_index, &display.meta.column_align);
            if !matches!(format, OutputFormat::Markdown | OutputFormat::Value) {
                summarize_table(&mut table, rows);
            }
            style_table(&mut table, &styles);
            vec![table]
        }
        OutputItems::Groups(groups) => groups
            .iter()
            .enumerate()
            .map(|(index, group)| {
                let rows = crate::core::output_model::group_rows(group);
                let styles = &grouped_styles[index];
                let mut table =
                    project_table(&rows, &display.meta.key_index, &display.meta.column_align);
                if !matches!(format, OutputFormat::Markdown | OutputFormat::Value) {
                    summarize_table(&mut table, &rows);
                }
                style_table(&mut table, styles);
                table
            })
            .collect(),
    };
    let records = match &display.items {
        OutputItems::Rows(rows) => rows
            .iter()
            .enumerate()
            .map(|(index, row)| {
                project_record(row, &[], styles.get(index).unwrap_or(&FieldStyles::new()))
            })
            .collect(),
        OutputItems::Groups(groups) => groups
            .iter()
            .enumerate()
            .flat_map(|(index, group)| {
                crate::core::output_model::group_rows(group)
                    .iter()
                    .enumerate()
                    .map(|(row_index, row)| {
                        project_record(row, &[], &grouped_styles[index][row_index])
                    })
                    .collect::<Vec<_>>()
            })
            .collect(),
    };
    let mut document_tables = std::collections::BTreeMap::new();
    if let Some(document) = display.document.as_ref().and_then(OutputDocument::semantic) {
        project_document_tables(&document.blocks, &mut Vec::new(), &mut document_tables);
    }
    HumanProjection {
        tables,
        records,
        document_tables,
        output: display,
        source_row_count,
        shortened,
    }
}

fn localize_timestamps(output: &mut OutputResult) {
    fn value(data: &mut Value) {
        match data {
            Value::String(text) => {
                if let Some(local) = super::format_local_timestamp(text) {
                    *text = local;
                }
            }
            Value::Array(items) => items.iter_mut().for_each(value),
            Value::Object(fields) => row(fields),
            _ => {}
        }
    }
    fn row(row: &mut Row) {
        row.values_mut().for_each(value);
    }
    fn blocks(items: &mut [DocBlock]) {
        for block in items {
            match block {
                DocBlock::Section { blocks: body, .. } => blocks(body),
                DocBlock::Data { value: data } => value(data),
                _ => {}
            }
        }
    }
    match &mut output.items {
        OutputItems::Rows(rows) => rows.iter_mut().for_each(row),
        OutputItems::Groups(groups) => {
            for group in groups {
                row(&mut group.groups);
                row(&mut group.aggregates);
                group.rows.iter_mut().for_each(row);
            }
        }
    }
    if let Some(OutputDocument::Semantic(document)) = &mut output.document {
        blocks(&mut document.blocks);
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

fn display_output(
    output: &OutputResult,
    format: OutputFormat,
    color: bool,
) -> (OutputResult, Vec<FieldStyles>) {
    let mut display = output.clone();
    let mut row_styles = Vec::new();
    if let OutputItems::Rows(rows) = &mut display.items {
        for row in rows.iter_mut() {
            let mut original = Value::Object(std::mem::take(row));
            let raw = original.clone();
            let mut styles = field_styles(&raw, &output.meta.display_rules);
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
                        let path = field_path(&original, field);
                        if let Some(Value::Array(items)) = display_path_mut(&mut original, field) {
                            let mut retained = Vec::new();
                            let mut index = 0;
                            items.retain(|item| {
                                let keep =
                                    display_path(item, when).and_then(Value::as_bool) != Some(true);
                                if keep {
                                    retained.push(index);
                                }
                                index += 1;
                                keep
                            });
                            retain_array_styles(&mut styles, &path, &retained);
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
            if color && !matches!(format, OutputFormat::Value | OutputFormat::Markdown) {
                apply_swatches(&mut original, &raw, &output.meta.display_rules);
            }
            *row = if let Some(columns) = &output.meta.display_columns {
                let labels = display_labels(output, columns);
                let mut selected_styles = FieldStyles::new();
                let row = columns
                    .iter()
                    .enumerate()
                    .map(|(index, selector)| {
                        let path = field_path(&original, selector);
                        let label = &labels[index];
                        for (address, style) in &styles {
                            if let Some(suffix) = address.strip_prefix(path.as_slice()) {
                                let mut address = vec![label.clone()];
                                address.extend_from_slice(suffix);
                                selected_styles.insert(address, style.clone());
                            }
                        }
                        (
                            label.clone(),
                            display_path(&original, selector)
                                .cloned()
                                .unwrap_or(Value::Null),
                        )
                    })
                    .collect();
                styles = selected_styles;
                row
            } else {
                original.as_object().cloned().unwrap_or_default()
            };
            row_styles.push(styles);
        }
        if let Some(columns) = &output.meta.display_columns {
            display.meta.key_index = display_labels(output, columns);
            display.meta.display_columns = Some(display.meta.key_index.clone());
        }
    }
    (display, row_styles)
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
        limited.meta.document_row_roles.truncate(limit);
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

fn summarize_table(table: &mut ProjectedTable, rows: &[Row]) {
    for (row_index, row) in rows.iter().enumerate() {
        for (column, header) in table.headers.iter().enumerate() {
            let summary = match row.get(header) {
                Some(Value::Array(items)) if items.iter().all(Value::is_object) => {
                    let fields = items
                        .iter()
                        .filter_map(Value::as_object)
                        .flat_map(|row| row.keys())
                        .collect::<BTreeSet<_>>();
                    (fields.len() > 1).then(|| {
                        format!(
                            "{} item{}",
                            items.len(),
                            if items.len() == 1 { "" } else { "s" }
                        )
                    })
                }
                Some(Value::Object(fields)) => {
                    let count = fields.values().filter(|value| !empty_value(value)).count();
                    Some(format!(
                        "{count} field{}",
                        if count == 1 { "" } else { "s" }
                    ))
                }
                _ => None,
            };
            if let Some(summary) = summary {
                table.rows[row_index][column] = summary;
                table.summarized_cells.insert((row_index, column));
            }
        }
    }
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
        summarized_cells: BTreeSet::new(),
        list_items: rows
            .iter()
            .enumerate()
            .flat_map(|(row_index, row)| {
                let all_headers = &all_headers;
                visible_columns
                    .iter()
                    .enumerate()
                    .filter_map(move |(column, original)| {
                        let items = row.get(&all_headers[*original])?.as_array()?;
                        items
                            .iter()
                            .all(|item| !item.is_array() && !item.is_object())
                            .then(|| {
                                (
                                    (row_index, column),
                                    items.iter().map(display_value).collect(),
                                )
                            })
                    })
            })
            .collect(),
        cell_styles: Vec::new(),
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
