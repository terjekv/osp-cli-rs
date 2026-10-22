//! Shared card indentation, label columns and list geometry.

use std::collections::BTreeMap;

use serde_json::Value;
use unicode_width::UnicodeWidthStr;

use super::super::doc::{GridBlock, GridCell, KeyValueRow, KeyValueValue};
use super::super::emit::key_value::{display_key, scalar_text};
use super::super::projection::{RecordProjection, display_value, ordered_keys};
use super::super::settings::ResolvedRenderSettings;
use super::super::text::{escape_table_line_breaks, wrap_display_width};
use crate::core::row::Row;

const CARD_GRID_GAP: usize = 3;
const CARD_FIELD_GAP: usize = 2;
const SHORT_FIELD_WIDTH_DIVISOR: usize = 2;
pub(in crate::ui) const CARD_INDENT: usize = 2;
const CARD_LABEL_CAP: usize = 18;

fn label_column(label: &str) -> usize {
    let width = UnicodeWidthStr::width(label) + CARD_FIELD_GAP;
    if width <= CARD_LABEL_CAP { width } else { 0 }
}

pub(super) fn plan_list_grid(
    values: &[&str],
    room: usize,
    style: Option<crate::ui::style::CellStyle>,
) -> GridBlock {
    let values = values
        .iter()
        .flat_map(|value| {
            value
                .split('\n')
                .flat_map(|line| wrap_display_width(line, room))
        })
        .collect::<Vec<_>>();
    let grid = super::super::emit::grid::PreparedGridList::from_items(&values, room);
    GridBlock {
        rows: grid
            .rows
            .into_iter()
            .map(|row| {
                row.into_iter()
                    .map(|value| GridCell {
                        label: String::new(),
                        value,
                        value_style: style.clone(),
                    })
                    .collect()
            })
            .collect(),
        column_widths: grid.column_widths,
        label_widths: Vec::new(),
        gap: grid.gap,
    }
}

pub(super) fn plan_classic_rows(
    rows: &mut [KeyValueRow],
    settings: &ResolvedRenderSettings,
    depth: usize,
) {
    let available = settings
        .width
        .unwrap_or(usize::MAX)
        .saturating_sub(settings.margin + depth * CARD_INDENT)
        .max(1);
    let inline_value = |row: &KeyValueRow| match &row.value {
        KeyValueValue::Scalar(value) if !value.is_empty() => Some(value.clone()),
        KeyValueValue::Array(items) if !items.is_empty() => items
            .iter()
            .map(scalar_text)
            .collect::<Option<Vec<_>>>()
            .map(|values| values.join(", ")),
        _ => None,
    };
    let column = rows
        .iter()
        .filter(|row| {
            inline_value(row).is_some_and(|value| match &row.value {
                KeyValueValue::Array(_) => {
                    !value.contains('\n')
                        && UnicodeWidthStr::width(value.as_str())
                            + UnicodeWidthStr::width(display_key(row).as_str())
                            + 2
                            <= available
                }
                _ => true,
            })
        })
        .map(|row| label_column(&display_key(row)))
        .max()
        .unwrap_or(0);
    for row in rows {
        let label = display_key(row);
        let prefix = column.max(UnicodeWidthStr::width(label.as_str()) + 2);
        row.gap = Some(" ".repeat(prefix - UnicodeWidthStr::width(label.as_str()) - 1));
        if let KeyValueValue::Array(items) = &row.value
            && let Some(values) = items.iter().map(scalar_text).collect::<Option<Vec<_>>>()
            && !values.is_empty()
        {
            let joined = values.join(", ");
            if !joined.contains('\n')
                && UnicodeWidthStr::width(joined.as_str()) + prefix <= available
            {
                row.key = label;
                row.value = KeyValueValue::Scalar(joined);
            } else {
                row.list_grid = Some(plan_list_grid(
                    &values,
                    available.saturating_sub(CARD_INDENT).max(1),
                    row.value_style.clone(),
                ));
            }
        }
        plan_nested_rows(&mut row.value, settings, depth + 1);
    }
}

fn plan_nested_rows(value: &mut KeyValueValue, settings: &ResolvedRenderSettings, depth: usize) {
    match value {
        KeyValueValue::Object(rows) => plan_classic_rows(rows, settings, depth),
        KeyValueValue::Array(items) => {
            for item in items {
                plan_nested_rows(item, settings, depth + 1);
            }
        }
        _ => {}
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(in crate::ui) enum CompactCard {
    Entry(GridCell),
    Inline(GridBlock),
    Titled {
        title: Option<String>,
        body: Vec<GridBlock>,
    },
}

pub(super) fn is_flat(row: &Row) -> bool {
    row.values().all(|value| match value {
        Value::Object(_) => false,
        Value::Array(items) => items
            .iter()
            .all(|item| !item.is_array() && !item.is_object()),
        _ => true,
    })
}

pub(super) fn plan_compact_cards(
    rows: &[Row],
    records: &[RecordProjection],
    key_order: &[String],
    rules: &[crate::core::DisplayRule],
    title_field: Option<&str>,
    settings: &ResolvedRenderSettings,
) -> Vec<CompactCard> {
    let available = settings
        .width
        .unwrap_or(usize::MAX)
        .saturating_sub(settings.margin)
        .max(1);
    let fields = rows
        .iter()
        .zip(records)
        .map(|(row, record)| {
            let mut fields = ordered_keys(key_order, row.keys())
                .filter(|key| !rules.iter().any(|rule| matches!(rule, crate::core::DisplayRule::Notice { field } if field == *key)))
                .filter_map(|key| {
                    row.get(key)
                        .filter(|value| settings.verbosity > 0 || Some(key.as_str()) == title_field || !super::super::projection::empty_value(value))
                        .map(|value| GridCell {
                        label: key.clone(),
                        value: display_value(value),
                        value_style: record
                            .iter()
                            .find(|(name, _)| name == key)
                            .and_then(|(_, field)| field.style.clone()),
                    })
                })
                .collect::<Vec<_>>();
            if let Some(index) = fields.iter().position(|field| Some(field.label.as_str()) == title_field) {
                let title = fields.remove(index);
                fields.insert(0, title);
            }
            fields
        })
        .collect::<Vec<_>>();
    let declared = fields
        .iter()
        .map(|fields| {
            fields
                .first()
                .is_some_and(|field| Some(field.label.as_str()) == title_field)
        })
        .collect::<Vec<_>>();
    let inline = plan_inline_cards(&fields, available);
    if !declared.iter().any(|declared| *declared)
        && let Some(cards) = &inline
    {
        return cards.clone();
    }
    let available = available.saturating_sub(CARD_INDENT).max(1);
    let mut longest = BTreeMap::<&str, usize>::new();
    for field in fields.iter().flatten() {
        let width = UnicodeWidthStr::width(escape_table_line_breaks(&field.value).as_str());
        longest
            .entry(&field.label)
            .and_modify(|longest| *longest = (*longest).max(width))
            .or_insert(width);
    }
    let key_width = fields
        .iter()
        .flat_map(|fields| fields.iter().skip(1))
        .map(|field| label_column(&field.label))
        .max()
        .unwrap_or(0);
    let field_prefix = |field: &GridCell| {
        key_width.max(UnicodeWidthStr::width(field.label.as_str()) + CARD_FIELD_GAP)
    };
    let short = |field: &GridCell| {
        field_prefix(field) + longest[field.label.as_str()] <= available
            && (!field.value.contains('\n')
                || !rows
                    .iter()
                    .any(|row| row.get(&field.label).is_some_and(Value::is_array)))
            && UnicodeWidthStr::width(field.label.as_str())
                + CARD_FIELD_GAP
                + longest[field.label.as_str()]
                <= available / SHORT_FIELD_WIDTH_DIVISOR
    };
    let grid_widths = |columns: usize| {
        let mut widths = vec![0; columns];
        for record in &fields {
            let mut index = 0;
            for field in record.iter().skip(1) {
                if short(field) {
                    let width =
                        UnicodeWidthStr::width(escape_table_line_breaks(&field.value).as_str());
                    widths[index % columns] =
                        widths[index % columns].max(field_prefix(field) + width);
                    index += 1;
                } else {
                    index = 0;
                }
            }
        }
        widths
    };
    let max_columns = fields
        .iter()
        .map(|record| record.len().saturating_sub(1))
        .max()
        .unwrap_or(1)
        .max(1);
    let column_widths = (1..=max_columns)
        .rev()
        .map(grid_widths)
        .find(|widths| {
            widths.iter().sum::<usize>() + CARD_GRID_GAP * (widths.len() - 1) <= available
        })
        .unwrap_or_else(|| grid_widths(1));
    let columns = column_widths.len();
    fields
        .iter()
        .zip(rows)
        .enumerate()
        .map(|(index, (fields, source))| {
            if !declared[index]
                && let Some(cards) = &inline
            {
                return cards[index].clone();
            }
            let Some(title) = fields.first() else {
                return CompactCard::Titled {
                    title: None,
                    body: Vec::new(),
                };
            };
            if fields.len() == 1 && !declared[index] {
                return CompactCard::Entry(title.clone());
            }
            let mut body = Vec::new();
            let mut run = Vec::new();
            let flush = |run: &mut Vec<GridCell>, body: &mut Vec<GridBlock>| {
                if !run.is_empty() {
                    for row in run.chunks(columns) {
                        body.push(GridBlock {
                            rows: vec![row.to_vec()],
                            column_widths: column_widths.clone(),
                            label_widths: row.iter().map(&field_prefix).collect(),
                            gap: CARD_GRID_GAP,
                        });
                    }
                    run.clear();
                }
            };
            for field in fields.iter().skip(1) {
                if short(field) {
                    let mut field = field.clone();
                    field.value = escape_table_line_breaks(&field.value);
                    run.push(field);
                    continue;
                }
                flush(&mut run, &mut body);
                let prefix = field_prefix(field);
                let room = available.saturating_sub(prefix).max(1);
                if let Some(Value::Array(items)) = source.get(&field.label)
                    && (field.value.contains('\n')
                        || UnicodeWidthStr::width(field.value.as_str()) > room)
                {
                    body.push(GridBlock {
                        rows: vec![vec![GridCell {
                            value: String::new(),
                            ..field.clone()
                        }]],
                        column_widths: vec![available],
                        label_widths: vec![prefix],
                        gap: 0,
                    });
                    let values = items.iter().map(display_value).collect::<Vec<_>>();
                    let values = values.iter().map(String::as_str).collect::<Vec<_>>();
                    let mut grid = plan_list_grid(
                        &values,
                        available.saturating_sub(CARD_INDENT).max(1),
                        field.value_style.clone(),
                    );
                    grid.label_widths = vec![CARD_INDENT];
                    if let Some(first) = grid.column_widths.first_mut() {
                        *first += CARD_INDENT;
                    }
                    body.push(grid);
                } else {
                    body.push(GridBlock {
                        rows: field
                            .value
                            .split('\n')
                            .flat_map(|line| wrap_display_width(line, room))
                            .enumerate()
                            .map(|(index, value)| {
                                vec![GridCell {
                                    label: if index == 0 {
                                        field.label.clone()
                                    } else {
                                        String::new()
                                    },
                                    value,
                                    value_style: field.value_style.clone(),
                                }]
                            })
                            .collect(),
                        column_widths: vec![available],
                        label_widths: vec![prefix],
                        gap: 0,
                    });
                }
            }
            flush(&mut run, &mut body);
            CompactCard::Titled {
                title: Some(if title.value.is_empty() && !declared[index] {
                    title.label.clone()
                } else {
                    title.value.clone()
                }),
                body,
            }
        })
        .collect()
}

fn plan_inline_cards(fields: &[Vec<GridCell>], available: usize) -> Option<Vec<CompactCard>> {
    let mut labels = Vec::new();
    for field in fields.iter().flat_map(|record| record.iter().skip(1)) {
        if !labels.contains(&field.label) {
            labels.push(field.label.clone());
        }
    }
    if labels.is_empty() {
        return None;
    }
    let rows = fields
        .iter()
        .map(|record| {
            let mut title = record.first().cloned().unwrap_or(GridCell {
                label: String::new(),
                value: String::new(),
                value_style: None,
            });
            title.value = if title.value.is_empty() && !title.label.is_empty() {
                format!("{}:", title.label)
            } else {
                escape_table_line_breaks(&title.value)
            };
            title.label.clear();
            std::iter::once(title)
                .chain(labels.iter().map(|label| {
                    record
                        .iter()
                        .skip(1)
                        .find(|field| &field.label == label)
                        .map(|field| GridCell {
                            value: escape_table_line_breaks(&field.value),
                            ..field.clone()
                        })
                        .unwrap_or(GridCell {
                            label: String::new(),
                            value: String::new(),
                            value_style: None,
                        })
                }))
                .collect::<Vec<_>>()
        })
        .collect::<Vec<_>>();
    let label_widths = std::iter::once(0)
        .chain(
            labels
                .iter()
                .map(|label| UnicodeWidthStr::width(label.as_str()) + 1),
        )
        .collect::<Vec<_>>();
    let column_widths = (0..label_widths.len())
        .map(|column| {
            label_widths[column]
                + rows
                    .iter()
                    .map(|row| UnicodeWidthStr::width(row[column].value.as_str()))
                    .max()
                    .unwrap_or(0)
                + if column == 0 {
                    2
                } else if column < labels.len() {
                    CARD_GRID_GAP
                } else {
                    0
                }
        })
        .collect::<Vec<_>>();
    if column_widths.iter().sum::<usize>() > available {
        return None;
    }
    Some(
        rows.into_iter()
            .map(|row| {
                CompactCard::Inline(GridBlock {
                    rows: vec![row],
                    column_widths: column_widths.clone(),
                    label_widths: label_widths.clone(),
                    gap: 0,
                })
            })
            .collect(),
    )
}
