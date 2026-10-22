//! Format and layout decisions for the human rendering pipeline.

mod cards;
mod table;

pub(super) use cards::CompactCard;

use super::doc::{GridBlock, GridCell};
use super::emit::grid::PreparedGridList;
use super::projection::{HumanProjection, ProjectedTable, RecordProjection, ValueProjection};
use super::settings::{
    FormatProvenance, RenderSettings, RenderTarget, ResolvedRenderSettings, TableBorderStyle,
    TableOverflow, TableTooWide, resolve_settings,
};
use super::text::{escape_table_line_breaks, list_cell_lines, table_cell_lines, table_cell_width};
use crate::core::output::OutputFormat;
use crate::core::output_model::{OutputItems, OutputResult};
use std::collections::BTreeMap;

// Small nested records are useful comparison tables; wider records remain lists.
const MAX_NESTED_TABLE_COLUMNS: usize = 8;
const MAX_FALLBACK_CARD_ROWS: usize = 20;
const TABLE_HEADER_LINES: usize = 1;
const TABLE_BORDER_LINES: usize = 3;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum MregLayout {
    Table,
    Cards,
    Entries,
    Compact,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct PlannedTable {
    pub data: ProjectedTable,
    pub widths: Vec<usize>,
    pub cell_lines: Vec<Vec<Vec<String>>>,
    pub footer: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum FieldKind {
    Value,
    Record(RecordLayout),
    Table(PlannedTable),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct FieldLayout {
    pub style: Option<super::style::CellStyle>,
    pub kind: FieldKind,
}

pub(super) type RecordLayout = Vec<(String, FieldLayout)>;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RenderPlan {
    pub format: OutputFormat,
    pub provenance: FormatProvenance,
    pub settings: ResolvedRenderSettings,
    pub(super) compact_cards: Vec<CompactCard>,
    pub(super) grid_list: Option<(String, GridBlock)>,
    pub(super) markdown_record: bool,
    pub(super) mreg_layout: MregLayout,
    pub(super) tables: Vec<PlannedTable>,
    pub(super) records: Vec<RecordLayout>,
    pub(super) document_tables: BTreeMap<Vec<usize>, PlannedTable>,
    pub document_long_help: bool,
    pub document_entry_gap: Option<usize>,
}

pub fn plan_format(
    output: &OutputResult,
    settings: &RenderSettings,
    target: RenderTarget,
) -> RenderPlan {
    let (format, provenance) = settings.resolve_output_format(output);
    let mut resolved = resolve_settings(settings, target);
    resolved.command_prefixes_enabled &= output.meta.document_command_prefixes;
    resolved.document_literal_commands = output.meta.document_literal_commands.clone();
    RenderPlan {
        format,
        provenance,
        settings: resolved,
        compact_cards: Vec::new(),
        grid_list: None,
        markdown_record: false,
        mreg_layout: MregLayout::Table,
        tables: Vec::new(),
        records: Vec::new(),
        document_tables: BTreeMap::new(),
        document_entry_gap: output.meta.document_entry_gap,
        document_long_help: output.meta.document_long_help,
    }
}

fn plan_table(
    data: &ProjectedTable,
    settings: &ResolvedRenderSettings,
    indent: usize,
    actions: Option<bool>,
) -> PlannedTable {
    let natural = data
        .headers
        .iter()
        .enumerate()
        .map(|(index, header)| {
            data.rows
                .iter()
                .enumerate()
                .filter_map(|(row_index, row)| {
                    row.get(index).map(|cell| {
                        if settings.table_overflow == TableOverflow::Wrap
                            && let Some(items) = data.list_items.get(&(row_index, index))
                        {
                            items
                                .iter()
                                .map(|item| table_cell_width(item, TableOverflow::None))
                                .max()
                                .unwrap_or(0)
                        } else {
                            table_cell_width(cell, settings.table_overflow)
                        }
                    })
                })
                .fold(
                    table_cell_width(header, settings.table_overflow),
                    usize::max,
                )
        })
        .collect::<Vec<_>>();
    let widths = table::fitted_table_widths(
        &natural,
        data,
        settings
            .width
            .map(|width| width.saturating_sub(settings.margin + indent)),
        if settings.width.is_some() {
            settings.table_overflow
        } else {
            TableOverflow::None
        },
    );
    let cell_lines = data
        .rows
        .iter()
        .enumerate()
        .map(|(row_index, row)| {
            widths
                .iter()
                .enumerate()
                .map(|(column, width)| {
                    if let Some(items) = data.list_items.get(&(row_index, column)) {
                        list_cell_lines(items, *width, settings.table_overflow)
                    } else {
                        table_cell_lines(
                            row.get(column).map(String::as_str).unwrap_or_default(),
                            *width,
                            settings.table_overflow,
                        )
                    }
                })
                .collect()
        })
        .collect();
    let footer = table::fit_footer(data, &widths, settings, indent, actions);
    PlannedTable {
        footer,
        data: data.clone(),
        widths,
        cell_lines,
    }
}

fn nested_table(value: &ValueProjection) -> Option<&ProjectedTable> {
    value
        .table
        .as_ref()
        .filter(|table| !table.rows.is_empty() && table.headers.len() <= MAX_NESTED_TABLE_COLUMNS)
}

fn has_nested_table(record: &RecordProjection) -> bool {
    record
        .iter()
        .any(|(_, value)| nested_table(value).is_some() || has_nested_table(&value.fields))
}

fn plan_record(
    record: &RecordProjection,
    settings: &ResolvedRenderSettings,
    depth: usize,
) -> RecordLayout {
    record
        .iter()
        .map(|(key, value)| {
            (
                key.clone(),
                FieldLayout {
                    style: value.style.clone(),
                    kind: if let Some(table) = nested_table(value) {
                        FieldKind::Table(plan_table(table, settings, (depth + 1) * 2, None))
                    } else if has_nested_table(&value.fields) {
                        FieldKind::Record(plan_record(&value.fields, settings, depth + 1))
                    } else {
                        FieldKind::Value
                    },
                },
            )
        })
        .collect()
}

pub(super) fn plan_layout(projection: &HumanProjection, plan: &mut RenderPlan) {
    let card_alternative = !projection.output.meta.grouped
        && !projection.output.meta.document_rows
        && !projection.records.is_empty();
    let actions = (!projection.output.meta.document_rows).then_some(card_alternative);
    plan.tables = projection
        .tables
        .iter()
        .map(|table| plan_table(table, &plan.settings, 0, actions))
        .collect();
    plan.mreg_layout = if plan.provenance == FormatProvenance::Explicit
        || (matches!(projection.output.items, OutputItems::Rows(_))
            && (projection.records.len() <= 1 || projection.records.iter().all(has_nested_table)))
    {
        MregLayout::Cards
    } else {
        MregLayout::Table
    };
    let output = &projection.output;
    if !output.meta.grouped
        && !output.meta.document_rows
        && let OutputItems::Rows(rows) = &output.items
        && let Some(first) = rows.first()
        && rows.len() > 1
        && first.len() == 1
        && let Some(key) = first.keys().next()
        && rows.iter().all(|row| {
            row.len() == 1
                && row
                    .get(key)
                    .is_some_and(|value| !value.is_array() && !value.is_object())
        })
    {
        if plan.format == OutputFormat::Mreg && plan.provenance == FormatProvenance::Explicit {
            plan.mreg_layout = MregLayout::Entries;
        } else if matches!(
            plan.format,
            OutputFormat::Table | OutputFormat::Mreg | OutputFormat::Guide
        ) && plan.provenance != FormatProvenance::Explicit
        {
            let values = rows
                .iter()
                .map(|row| escape_table_line_breaks(&super::projection::display_value(&row[key])))
                .collect::<Vec<_>>();
            let grid = PreparedGridList::from_items(
                &values,
                plan.settings
                    .width
                    .unwrap_or(usize::MAX)
                    .saturating_sub(plan.settings.margin + 2)
                    .max(1),
            );
            plan.grid_list = Some((
                format!(
                    "{key} ({}):",
                    projection.source_row_count.unwrap_or(rows.len())
                ),
                GridBlock {
                    rows: grid
                        .rows
                        .iter()
                        .enumerate()
                        .map(|(row_index, row)| {
                            row.iter()
                                .enumerate()
                                .map(|(column, value)| GridCell {
                                    label: String::new(),
                                    value: value.clone(),
                                    value_style: grid
                                        .source_indices
                                        .get(row_index)
                                        .and_then(|row| row.get(column))
                                        .copied()
                                        .flatten()
                                        .and_then(|index| projection.records.get(index))
                                        .and_then(|record| record.first())
                                        .and_then(|(_, field)| field.style.clone()),
                                })
                                .collect()
                        })
                        .collect(),
                    column_widths: grid.column_widths,
                    label_widths: Vec::new(),
                    gap: grid.gap,
                },
            ));
        }
    }

    plan.markdown_record = matches!(&output.items, OutputItems::Rows(rows)
        if rows.len() == 1 && rows[0].values().any(|value| value.is_array() || value.is_object()));
    plan.records = projection
        .records
        .iter()
        .map(|record| plan_record(record, &plan.settings, 0))
        .collect();
    let compare_cards = can_compare_cards(projection, plan);
    if compare_cards || (plan.format == OutputFormat::Mreg && plan.mreg_layout == MregLayout::Cards)
    {
        let rows = crate::core::output_model::output_items_to_rows(&output.items);
        if rows.len() > 1 && rows.iter().all(cards::is_flat) {
            plan.compact_cards = cards::plan_compact_cards(
                &rows,
                &projection.records,
                &output.meta.key_index,
                &output.meta.display_rules,
                &plan.settings,
            );
            if plan.mreg_layout == MregLayout::Cards {
                plan.mreg_layout = MregLayout::Compact;
            }
        }
    }
    plan.document_tables = projection
        .document_tables
        .iter()
        .map(|(path, table)| {
            (
                path.clone(),
                plan_table(
                    table,
                    &plan.settings,
                    path.len().saturating_sub(1) * 2,
                    None,
                ),
            )
        })
        .collect();
    if compare_cards && let [table] = plan.tables.as_slice() {
        let row_lines = table
            .cell_lines
            .iter()
            .map(|row| row.iter().map(Vec::len).max().unwrap_or(1))
            .sum::<usize>();
        let table_lines = TABLE_HEADER_LINES
            + row_lines
            + if plan.settings.table_border == TableBorderStyle::None {
                0
            } else {
                TABLE_BORDER_LINES
            }
            + table.footer.len();
        let row_count = table.data.rows.len();
        let table_values = row_count * table.widths.len();
        let card_values = row_count
            * table
                .data
                .headers
                .iter()
                .enumerate()
                .filter(|(index, _)| {
                    table.data.rows.iter().any(|row| {
                        row.get(*index)
                            .is_some_and(|value| !value.trim().is_empty())
                    })
                })
                .count();
        let mut cards = plan.clone();
        cards.format = OutputFormat::Mreg;
        cards.mreg_layout = if cards.compact_cards.is_empty() {
            MregLayout::Cards
        } else {
            MregLayout::Compact
        };
        // Measure the actual candidate, including classic nested tables/lists,
        // rather than duplicating their line geometry in a density estimate.
        let card_doc = super::lower::lower_mreg_doc(output, &cards);
        let card_lines = super::emit::emit_doc(&card_doc, OutputFormat::Mreg, &cards.settings)
            .lines()
            .count();
        if card_values * table_lines > table_values * card_lines {
            plan.format = cards.format;
            plan.mreg_layout = cards.mreg_layout;
        }
    }
}

fn can_compare_cards(projection: &HumanProjection, plan: &RenderPlan) -> bool {
    let output = &projection.output;
    plan.grid_list.is_none()
        && plan.format == OutputFormat::Table
        && matches!(
            plan.provenance,
            FormatProvenance::Inferred | FormatProvenance::Recommended
        )
        && plan.settings.table_too_wide == TableTooWide::Mreg
        && plan.settings.display_limits
        && plan.settings.width.is_some()
        && plan.settings.table_overflow != TableOverflow::None
        && !output.meta.grouped
        && !output.meta.document_rows
        && projection
            .source_row_count
            .is_some_and(|count| (1..=MAX_FALLBACK_CARD_ROWS).contains(&count))
        && plan.tables.len() == 1
}
