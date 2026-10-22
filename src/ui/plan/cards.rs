//! Compact card geometry for lists of flat records.

use std::collections::BTreeMap;

use serde_json::Value;
use unicode_width::UnicodeWidthStr;

use super::super::doc::{GridBlock, GridCell};
use super::super::projection::{RecordProjection, display_value, ordered_keys};
use super::super::settings::ResolvedRenderSettings;
use super::super::text::{escape_table_line_breaks, wrap_display_width};
use crate::core::row::Row;

const CARD_GRID_GAP: usize = 3;
const CARD_FIELD_GAP: usize = 2;
const SHORT_FIELD_WIDTH_DIVISOR: usize = 2;
const UNKNOWN_CARD_WIDTH: usize = 100;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(in crate::ui) enum CompactCard {
    Entry(GridCell),
    Inline(GridBlock),
    Titled { title: String, body: Vec<GridBlock> },
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
    settings: &ResolvedRenderSettings,
) -> Vec<CompactCard> {
    let available = settings
        .width
        .unwrap_or(UNKNOWN_CARD_WIDTH)
        .saturating_sub(settings.margin)
        .max(1);
    let fields = rows
        .iter()
        .zip(records)
        .map(|(row, record)| {
            ordered_keys(key_order, row.keys())
                .filter(|key| !rules.iter().any(|rule| matches!(rule, crate::core::DisplayRule::Notice { field } if field == *key)))
                .filter_map(|key| {
                    row.get(key).map(|value| GridCell {
                        label: key.clone(),
                        value: display_value(value),
                        value_style: record
                            .iter()
                            .find(|(name, _)| name == key)
                            .and_then(|(_, field)| field.style.clone()),
                    })
                })
                .collect::<Vec<_>>()
        })
        .collect::<Vec<_>>();
    if let Some(cards) = plan_inline_cards(&fields, available) {
        return cards;
    }
    let available = available.saturating_sub(2).max(1);
    let mut longest = BTreeMap::<&str, usize>::new();
    for field in fields.iter().flatten() {
        let width = UnicodeWidthStr::width(escape_table_line_breaks(&field.value).as_str());
        longest
            .entry(&field.label)
            .and_modify(|longest| *longest = (*longest).max(width))
            .or_insert(width);
    }
    let short = |field: &GridCell| {
        UnicodeWidthStr::width(field.label.as_str())
            + CARD_FIELD_GAP
            + longest[field.label.as_str()]
            <= available / SHORT_FIELD_WIDTH_DIVISOR
    };
    let (mut key_width, mut long_key_width) = (0, 0);
    for field in fields
        .iter()
        .flat_map(|fields| fields.iter().skip(1))
        .filter(|field| !field.value.trim().is_empty())
    {
        if short(field) {
            key_width = key_width.max(UnicodeWidthStr::width(field.label.as_str()));
        } else {
            long_key_width = long_key_width.max(UnicodeWidthStr::width(field.label.as_str()));
        }
    }
    key_width = key_width.max(long_key_width);
    let grid_widths = |columns: usize| {
        let mut widths = vec![0; columns];
        for record in &fields {
            let mut index = 0;
            for field in record
                .iter()
                .skip(1)
                .filter(|field| !field.value.trim().is_empty())
            {
                if short(field) {
                    let width =
                        UnicodeWidthStr::width(escape_table_line_breaks(&field.value).as_str());
                    widths[index % columns] =
                        widths[index % columns].max(key_width + CARD_FIELD_GAP + width);
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
        .map(|(fields, source)| {
            let Some(title) = fields.first() else {
                return CompactCard::Titled {
                    title: String::new(),
                    body: Vec::new(),
                };
            };
            if fields
                .iter()
                .skip(1)
                .all(|field| field.value.trim().is_empty())
            {
                return CompactCard::Entry(title.clone());
            }
            let mut body = Vec::new();
            let mut run = Vec::new();
            let flush = |run: &mut Vec<GridCell>, body: &mut Vec<GridBlock>| {
                if !run.is_empty() {
                    body.push(GridBlock {
                        rows: run.chunks(columns).map(<[GridCell]>::to_vec).collect(),
                        column_widths: column_widths.clone(),
                        label_widths: vec![key_width + CARD_FIELD_GAP; columns],
                        gap: CARD_GRID_GAP,
                    });
                    run.clear();
                }
            };
            for field in fields
                .iter()
                .skip(1)
                .filter(|field| !field.value.trim().is_empty())
            {
                if short(field) {
                    let mut field = field.clone();
                    field.value = escape_table_line_breaks(&field.value);
                    run.push(field);
                    continue;
                }
                flush(&mut run, &mut body);
                let room = available.saturating_sub(key_width + CARD_FIELD_GAP).max(1);
                if let Some(Value::Array(items)) = source.get(&field.label) {
                    let values = items
                        .iter()
                        .flat_map(|item| {
                            let value = display_value(item);
                            value
                                .split('\n')
                                .flat_map(|line| wrap_display_width(line, room))
                                .collect::<Vec<_>>()
                        })
                        .collect::<Vec<_>>();
                    let grid =
                        super::super::emit::grid::PreparedGridList::from_items(&values, room);
                    body.push(GridBlock {
                        rows: vec![vec![GridCell {
                            value: String::new(),
                            ..field.clone()
                        }]],
                        column_widths: vec![available],
                        label_widths: vec![key_width + CARD_FIELD_GAP],
                        gap: 0,
                    });
                    body.push(GridBlock {
                        rows: grid
                            .rows
                            .iter()
                            .map(|row| {
                                row.iter()
                                    .map(|value| GridCell {
                                        label: String::new(),
                                        value: value.clone(),
                                        value_style: field.value_style.clone(),
                                    })
                                    .collect()
                            })
                            .collect(),
                        column_widths: grid.column_widths,
                        label_widths: Vec::new(),
                        gap: grid.gap,
                    });
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
                        label_widths: vec![key_width + CARD_FIELD_GAP],
                        gap: 0,
                    });
                }
            }
            flush(&mut run, &mut body);
            CompactCard::Titled {
                title: title.value.clone(),
                body,
            }
        })
        .collect()
}

fn plan_inline_cards(fields: &[Vec<GridCell>], available: usize) -> Option<Vec<CompactCard>> {
    let mut labels = Vec::new();
    for field in fields.iter().flat_map(|record| record.iter().skip(1)) {
        if !field.value.trim().is_empty() && !labels.contains(&field.label) {
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
            title.label.clear();
            title.value = escape_table_line_breaks(&title.value);
            std::iter::once(title)
                .chain(labels.iter().map(|label| {
                    record
                        .iter()
                        .skip(1)
                        .find(|field| &field.label == label && !field.value.trim().is_empty())
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
