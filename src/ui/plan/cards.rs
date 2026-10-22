//! Compact card geometry for lists of flat records.

use std::collections::BTreeMap;

use serde_json::Value;
use unicode_width::UnicodeWidthStr;

use super::super::doc::{GridBlock, GridCell};
use super::super::projection::{RecordProjection, display_value, ordered_keys};
use super::super::settings::ResolvedRenderSettings;
use super::super::text::{escape_table_line_breaks, wrap_display_width};
use crate::core::row::Row;

const CARD_GRID_GAP: usize = 4;
const CARD_FIELD_GAP: usize = 2;
const SHORT_FIELD_WIDTH_DIVISOR: usize = 2;
const UNKNOWN_CARD_WIDTH: usize = 100;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(in crate::ui) enum CompactCard {
    Entry(GridCell),
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
        .saturating_sub(settings.margin + 2)
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
    let (mut key_width, mut value_width, mut long_key_width) = (0, 0, 0);
    for field in fields
        .iter()
        .flat_map(|fields| fields.iter().skip(1))
        .filter(|field| !field.value.trim().is_empty())
    {
        if short(field) {
            key_width = key_width.max(UnicodeWidthStr::width(field.label.as_str()));
            value_width = value_width.max(longest[field.label.as_str()]);
        } else {
            long_key_width = long_key_width.max(UnicodeWidthStr::width(field.label.as_str()));
        }
    }
    key_width = key_width.max(long_key_width);
    let cell_width = key_width + CARD_FIELD_GAP + value_width;
    let columns = ((available + CARD_GRID_GAP) / (cell_width + CARD_GRID_GAP)).max(1);
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
                        column_widths: vec![cell_width; columns],
                        label_width: key_width,
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
                        label_width: key_width,
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
                        label_width: 0,
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
                        label_width: key_width,
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
