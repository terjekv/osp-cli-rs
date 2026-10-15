//! Projection keeps the selector surface while letting users narrow structure.
//!
//! The important rules here are:
//! - keepers and droppers resolve against original addresses, not already
//!   compacted output
//! - structural rebuild happens before generic compaction
//! - row fanout labels must be unambiguous
//!
//! Example:
//! - `P sections[1].entries[0].name !sections[0]` should treat the dropper
//!   against the original tree, not delete the only surviving rebuilt branch
//! - row projection `P users[].name groups[].name` should fail loudly because
//!   both fanouts want the same dynamic `name` label

#[cfg(test)]
use crate::core::output_model::Group;
use crate::core::row::Row;
#[cfg(test)]
use crate::dsl::verbs::common::map_group_rows;
use anyhow::{Result, anyhow};
use serde_json::{Map, Value};
use std::collections::{BTreeSet, HashMap};

use crate::dsl::{
    eval::{
        flatten::{coalesce_flat_row_with_fill, flatten_row},
        resolve::{AddressStep, AddressedValue, compact_sparse_arrays, sparse_hole},
    },
    verbs::common::parse_terms,
};

use super::selector;

#[derive(Debug, Clone)]
pub(crate) struct ProjectPlan {
    keepers: Vec<selector::CompiledSelector>,
    droppers: Vec<selector::CompiledSelector>,
}

impl ProjectPlan {
    pub(crate) fn project_row(&self, row: &Row) -> Result<Vec<Row>> {
        project_single_row(row, &self.keepers, &self.droppers)
    }
}

pub(crate) fn compile(spec: &str) -> Result<ProjectPlan> {
    let (keepers, droppers) = parse_patterns(spec)?;
    if keepers.is_empty() && droppers.is_empty() {
        return Err(anyhow!("P requires one or more keys"));
    }

    Ok(ProjectPlan { keepers, droppers })
}

#[cfg(test)]
/// Projects flat rows according to the keep/drop patterns in `spec`.
///
/// Fanout selectors may expand one input row into multiple output rows.
pub fn apply(rows: Vec<Row>, spec: &str) -> Result<Vec<Row>> {
    let plan = compile(spec)?;
    apply_with_plan(rows, &plan)
}

#[cfg(test)]
/// Projects the rows inside each group while preserving group metadata.
///
/// Empty member sets retain their group metadata.
pub fn apply_groups(groups: Vec<Group>, spec: &str) -> Result<Vec<Group>> {
    let plan = compile(spec)?;
    apply_groups_with_plan(groups, &plan)
}

pub(crate) fn apply_with_plan(rows: Vec<Row>, plan: &ProjectPlan) -> Result<Vec<Row>> {
    let mut out = Vec::new();
    for row in rows {
        out.extend(plan.project_row(&row)?);
    }
    Ok(out)
}

#[cfg(test)]
pub(crate) fn apply_groups_with_plan(groups: Vec<Group>, plan: &ProjectPlan) -> Result<Vec<Group>> {
    map_group_rows(groups, |rows| apply_with_plan(rows, plan))
}

fn parse_patterns(
    spec: &str,
) -> Result<(
    Vec<selector::CompiledSelector>,
    Vec<selector::CompiledSelector>,
)> {
    let trimmed = spec.trim();
    if trimmed.is_empty() {
        return Ok((Vec::new(), Vec::new()));
    }

    let mut keepers = Vec::new();
    let mut droppers = Vec::new();
    for text in parse_terms(trimmed)? {
        let drop = text.starts_with('!');
        let pattern = selector::CompiledSelector::parse(&text);

        if drop {
            droppers.push(pattern);
        } else {
            keepers.push(pattern);
        }
    }

    Ok((keepers, droppers))
}

fn project_single_row(
    row: &Row,
    keepers: &[selector::CompiledSelector],
    droppers: &[selector::CompiledSelector],
) -> Result<Vec<Row>> {
    let flattened = flatten_row(row);
    let nested = Value::Object(row.clone());

    let mut static_flat = if keepers.is_empty() {
        flattened.clone()
    } else {
        Map::new()
    };
    let mut dynamic_columns: Vec<DynamicColumn> = Vec::new();

    let drops = selector::collect_compiled_matches(&nested, droppers.iter());
    for pattern in keepers {
        if let Some(column) = pattern.collect_dynamic_column(&nested) {
            dynamic_columns.push(DynamicColumn {
                label: column.0,
                source: pattern.token().to_string(),
                values: column
                    .1
                    .into_iter()
                    .filter(|entry| {
                        !drops
                            .iter()
                            .any(|drop| entry.address.starts_with(&drop.address))
                    })
                    .collect(),
            });
            continue;
        }

        for key in pattern.matched_flat_keys(&flattened) {
            if let Some(value) = flattened.get(&key) {
                static_flat.insert(key, value.clone());
            }
        }
    }

    for pattern in droppers {
        for key in pattern.matched_flat_keys(&flattened) {
            static_flat.remove(&key);
        }
    }

    static_flat.retain(|key, _| {
        !drops.iter().any(|entry| {
            key == &entry.flat_key
                || key.starts_with(&format!("{}.", entry.flat_key))
                || key.starts_with(&format!("{}[", entry.flat_key))
        })
    });

    reject_ambiguous_dynamic_columns(&static_flat, &dynamic_columns)?;
    let mut rows = build_rows_from_dynamic(static_flat, dynamic_columns);
    if rows.is_empty() && keepers.is_empty() {
        rows.push(rebuild_row(&Map::new()));
    }
    Ok(rows)
}

#[derive(Debug, Clone)]
struct DynamicColumn {
    label: String,
    source: String,
    values: Vec<AddressedValue>,
}

fn reject_ambiguous_dynamic_columns(static_flat: &Row, columns: &[DynamicColumn]) -> Result<()> {
    let static_keys = static_flat
        .keys()
        .map(|key| (key.as_str(), key.as_str()))
        .collect::<Vec<_>>();

    let mut dynamic_keys = Vec::<(String, &str)>::new();
    for column in columns {
        let emitted_keys = column
            .values
            .iter()
            .flat_map(|entry| match &entry.value {
                Value::Object(map) => map.keys().cloned().collect(),
                _ => vec![column.label.clone()],
            })
            .collect::<BTreeSet<_>>();

        for key in &emitted_keys {
            if let Some((_, source)) = static_keys
                .iter()
                .find(|(existing, _)| output_keys_overlap(existing, key))
            {
                return Err(ambiguous_projection_key(key, source, &column.source));
            }
            if let Some((_, source)) = dynamic_keys
                .iter()
                .find(|(existing, _)| output_keys_overlap(existing, key))
            {
                return Err(ambiguous_projection_key(key, source, &column.source));
            }
        }
        dynamic_keys.extend(
            emitted_keys
                .into_iter()
                .map(|key| (key, column.source.as_str())),
        );
    }

    Ok(())
}

fn output_keys_overlap(left: &str, right: &str) -> bool {
    fn is_parent(parent: &str, child: &str) -> bool {
        child
            .strip_prefix(parent)
            .is_some_and(|suffix| suffix.starts_with('.') || suffix.starts_with('['))
    }

    left == right || is_parent(left, right) || is_parent(right, left)
}

fn ambiguous_projection_key(key: &str, first: &str, second: &str) -> anyhow::Error {
    anyhow!("ambiguous dynamic projection label `{key}` from selectors: {first}, {second}")
}

fn rebuild_row(flat: &Row) -> Row {
    let mut value = Value::Object(coalesce_flat_row_with_fill(flat, &sparse_hole()));
    compact_sparse_arrays(&mut value);
    value.as_object().cloned().unwrap_or_default()
}

fn build_rows_from_dynamic(static_flat: Row, dynamic_columns: Vec<DynamicColumn>) -> Vec<Row> {
    if dynamic_columns.is_empty() {
        if static_flat.is_empty() {
            return Vec::new();
        }
        return vec![rebuild_row(&static_flat)];
    }

    let mut rows = Vec::new();
    let mut index_by_address = HashMap::new();
    for column in dynamic_columns {
        for mut entry in column.values {
            if let Some(index) = entry
                .address
                .iter()
                .rposition(|step| matches!(step, AddressStep::Index(_)))
            {
                entry.address.truncate(index + 1);
            }
            let index = *index_by_address.entry(entry.address).or_insert_with(|| {
                let index = rows.len();
                rows.push(static_flat.clone());
                index
            });
            let flat = &mut rows[index];
            match entry.value {
                Value::Object(map) => flat.extend(map),
                scalar => {
                    flat.insert(column.label.clone(), scalar);
                }
            }
        }
    }
    if rows.is_empty() && !static_flat.is_empty() {
        rows.push(static_flat);
    }

    rows.into_iter()
        .map(|flat| rebuild_row(&flat))
        .filter(|row| !row.is_empty())
        .collect()
}

#[cfg(test)]
mod tests;
