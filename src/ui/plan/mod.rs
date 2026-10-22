//! Format and layout decisions for the human rendering pipeline.

mod table;

use super::projection::{HumanProjection, ProjectedTable, RecordProjection, ValueProjection};
use super::settings::{
    FormatProvenance, RenderProfile, RenderSettings, ResolvedRenderSettings, TableOverflow,
    TableTooWide, resolve_settings,
};
use super::text::table_cell_width;
use crate::core::output::OutputFormat;
use crate::core::output_model::{OutputItems, OutputResult};
use std::collections::BTreeMap;

// Small nested records are useful comparison tables; wider records remain lists.
const MAX_NESTED_TABLE_COLUMNS: usize = 8;
const MAX_FALLBACK_CARD_ROWS: usize = 20;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum MregLayout {
    Table,
    Cards,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct PlannedTable {
    pub data: ProjectedTable,
    pub widths: Vec<usize>,
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
    profile: RenderProfile,
) -> RenderPlan {
    let (format, provenance) = settings.resolve_output_format(output);
    let mut resolved = resolve_settings(settings, profile);
    resolved.command_prefixes_enabled &= output.meta.document_command_prefixes;
    resolved.document_literal_commands = output.meta.document_literal_commands.clone();
    RenderPlan {
        format,
        provenance,
        settings: resolved,
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
) -> PlannedTable {
    let natural = data
        .headers
        .iter()
        .enumerate()
        .map(|(index, header)| {
            data.rows
                .iter()
                .filter_map(|row| row.get(index))
                .map(|cell| table_cell_width(cell, settings.table_overflow))
                .fold(
                    table_cell_width(header, settings.table_overflow),
                    usize::max,
                )
        })
        .collect::<Vec<_>>();
    let widths = table::fitted_table_widths(
        &natural,
        &data.headers,
        &data.rows,
        settings
            .width
            .map(|width| width.saturating_sub(settings.margin + indent)),
        if settings.width.is_some() {
            settings.table_overflow
        } else {
            TableOverflow::None
        },
    );
    PlannedTable {
        data: data.clone(),
        widths,
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
                        FieldKind::Table(plan_table(table, settings, (depth + 1) * 2))
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

pub(super) fn plan_layout(
    projection: &HumanProjection,
    plan: &mut RenderPlan,
    settings: &RenderSettings,
    profile: RenderProfile,
) {
    plan.tables = projection
        .tables
        .iter()
        .map(|table| plan_table(table, &plan.settings, 0))
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
    plan.markdown_record = matches!(&output.items, OutputItems::Rows(rows)
        if rows.len() == 1 && rows[0].values().any(|value| value.is_array() || value.is_object()));
    if plan.format == OutputFormat::Table
        && matches!(
            plan.provenance,
            FormatProvenance::Inferred | FormatProvenance::Recommended
        )
        && settings.table_too_wide == TableTooWide::Mreg
        && settings.runtime.stdout_is_tty
        && profile == RenderProfile::Normal
        && plan.settings.width.is_some()
        && plan.settings.table_overflow != TableOverflow::None
        && !output.meta.grouped
        && !output.meta.document_rows
        && projection
            .source_row_count
            .is_some_and(|count| (1..=MAX_FALLBACK_CARD_ROWS).contains(&count))
        && let [table] = plan.tables.as_slice()
        && table.widths.len() < table.data.headers.len()
    {
        plan.format = OutputFormat::Mreg;
        plan.mreg_layout = MregLayout::Cards;
    }
    plan.records = projection
        .records
        .iter()
        .map(|record| plan_record(record, &plan.settings, 0))
        .collect();
    plan.document_tables = projection
        .document_tables
        .iter()
        .map(|(path, table)| {
            (
                path.clone(),
                plan_table(table, &plan.settings, path.len().saturating_sub(1) * 2),
            )
        })
        .collect();
}
