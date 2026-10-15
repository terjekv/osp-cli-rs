#[cfg(test)]
use crate::core::output_model::OutputItems;
use std::{cmp::Ordering, net::IpAddr};

use crate::core::{output_model::Group, row::Row};
use anyhow::{Result, anyhow};
use serde_json::Value;

use crate::dsl::{
    eval::resolve::resolve_first_value, parse::key_spec::KeySpec, verbs::common::parse_stage_words,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SortCast {
    Auto,
    Num,
    Str,
    Ip,
}

#[derive(Debug, Clone)]
struct SortKeySpec {
    key_spec: KeySpec,
    descending: bool,
    cast: SortCast,
}

#[derive(Debug, Clone)]
pub(crate) struct SortPlan {
    keys: Vec<SortKeySpec>,
}

pub(crate) fn compile(spec: &str) -> Result<SortPlan> {
    Ok(SortPlan {
        keys: parse_sort_spec(spec)?,
    })
}

pub(crate) fn apply_set(
    mut set: crate::dsl::model::RowSet,
    plan: &SortPlan,
) -> Result<crate::dsl::model::RowSet> {
    if set.grouped {
        set.partitions = sort_items(set.partitions, plan, resolve_group_value)?;
        Ok(set)
    } else {
        set.map_rows(|rows| {
            sort_items(rows, plan, |row, key| {
                resolve_first_value(row, &key.key_spec.token, key.key_spec.exact)
            })
        })
    }
}

#[cfg(test)]
fn apply_with_plan(items: OutputItems, plan: &SortPlan) -> Result<OutputItems> {
    apply_set(items.into(), plan).map(Into::into)
}

#[cfg(test)]
fn apply_value_with_plan(value: Value, plan: &SortPlan) -> Result<Value> {
    crate::dsl::value::apply_stage(
        value,
        &crate::dsl::compiled::CompiledStage::Sort(plan.clone()),
    )
}

fn sort_items<T>(
    items: Vec<T>,
    plan: &SortPlan,
    resolve: impl Fn(&T, &SortKeySpec) -> Option<Value>,
) -> Result<Vec<T>> {
    let mut keyed = items
        .into_iter()
        .map(|item| {
            let values = plan
                .keys
                .iter()
                .map(|key| resolve(&item, key))
                .collect::<Vec<_>>();
            (item, values)
        })
        .collect::<Vec<_>>();
    for (index, key) in plan.keys.iter().enumerate() {
        if !keyed.is_empty() && !keyed.iter().any(|(_, values)| values[index].is_some()) {
            return Err(anyhow!(
                "S: field '{}' is absent from all rows",
                key.key_spec.token
            ));
        }
    }
    keyed.sort_by(|(_, left), (_, right)| compare_keys(left, right, &plan.keys));
    Ok(keyed.into_iter().map(|(item, _)| item).collect())
}

fn parse_sort_spec(spec: &str) -> Result<Vec<SortKeySpec>> {
    let words = parse_stage_words(spec)?;

    if words.is_empty() {
        return Err(anyhow!("S requires one or more keys"));
    }

    let mut keys = Vec::new();
    let mut index = 0usize;
    while index < words.len() {
        let token = &words[index];
        let descending = token.starts_with(['!', '-']);
        let raw_key = if descending { &token[1..] } else { token };
        if raw_key.is_empty() || matches!(raw_key.to_ascii_lowercase().as_str(), "asc" | "desc") {
            return Err(anyhow!(
                "S: expected a field; use `S -field` or `S field desc`"
            ));
        }
        let mut key = SortKeySpec {
            key_spec: KeySpec::parse(raw_key),
            descending,
            cast: SortCast::Auto,
        };

        index += 1;
        let mut has_direction = false;
        let mut has_cast = false;
        while let Some(modifier) = words.get(index) {
            if modifier.eq_ignore_ascii_case("asc") || modifier.eq_ignore_ascii_case("desc") {
                if has_direction || (descending && modifier.eq_ignore_ascii_case("asc")) {
                    return Err(anyhow!(
                        "S: conflicting or repeated sort direction for {raw_key}"
                    ));
                }
                key.descending = modifier.eq_ignore_ascii_case("desc");
                has_direction = true;
                index += 1;
            } else if modifier.eq_ignore_ascii_case("AS") {
                if has_cast {
                    return Err(anyhow!("S: repeated cast for {raw_key}"));
                }
                let cast = words
                    .get(index + 1)
                    .ok_or_else(|| anyhow!("S: AS requires a cast"))?;
                key.cast = parse_sort_cast(cast)?;
                has_cast = true;
                index += 2;
            } else {
                break;
            }
        }
        keys.push(key);
    }

    Ok(keys)
}

fn parse_sort_cast(raw: &str) -> Result<SortCast> {
    match raw.to_ascii_lowercase().as_str() {
        "auto" => Ok(SortCast::Auto),
        "num" | "number" => Ok(SortCast::Num),
        "str" | "string" => Ok(SortCast::Str),
        "ip" => Ok(SortCast::Ip),
        other => Err(anyhow!("S: unsupported cast '{other}'")),
    }
}

fn compare_keys(left: &[Option<Value>], right: &[Option<Value>], keys: &[SortKeySpec]) -> Ordering {
    for ((left_value, right_value), key) in left.iter().zip(right).zip(keys) {
        let mut ordering =
            compare_optional_values(left_value.as_ref(), right_value.as_ref(), key.cast);
        if key.descending && left_value.is_some() && right_value.is_some() {
            ordering = ordering.reverse();
        }
        if ordering != Ordering::Equal {
            return ordering;
        }
    }
    Ordering::Equal
}

fn resolve_group_value(group: &Group, key: &SortKeySpec) -> Option<Value> {
    group
        .groups
        .get(&key.key_spec.token)
        .or_else(|| group.aggregates.get(&key.key_spec.token))
        .cloned()
        .or_else(|| {
            resolve_first_value(
                &merged_group_row(group),
                &key.key_spec.token,
                key.key_spec.exact,
            )
        })
}

fn merged_group_row(group: &Group) -> Row {
    let mut merged = group.groups.clone();
    merged.extend(group.aggregates.clone());
    merged
}

fn compare_optional_values(
    left: Option<&Value>,
    right: Option<&Value>,
    cast: SortCast,
) -> Ordering {
    match (left, right) {
        (None, None) => Ordering::Equal,
        (None, Some(_)) => Ordering::Greater,
        (Some(_), None) => Ordering::Less,
        (Some(left), Some(right)) => compare_values(left, right, cast),
    }
}

fn compare_values(left: &Value, right: &Value, cast: SortCast) -> Ordering {
    match cast {
        SortCast::Num => compare_numbers(left, right),
        SortCast::Str => to_string_normalized(left).cmp(&to_string_normalized(right)),
        SortCast::Ip => compare_ip(left, right),
        SortCast::Auto => compare_auto(left, right),
    }
}

pub(super) fn compare_auto(left: &Value, right: &Value) -> Ordering {
    // Classification must depend on each value, never on its comparison
    // partner: pairwise numeric/text fallback produces ordering cycles.
    match (to_f64(left), to_f64(right)) {
        (Some(left), Some(right)) => left.total_cmp(&right),
        (Some(_), None) => Ordering::Less,
        (None, Some(_)) => Ordering::Greater,
        (None, None) => compare_ip(left, right),
    }
}

fn compare_numbers(left: &Value, right: &Value) -> Ordering {
    match (to_f64(left), to_f64(right)) {
        (Some(left_num), Some(right_num)) => left_num.total_cmp(&right_num),
        (Some(_), None) => Ordering::Less,
        (None, Some(_)) => Ordering::Greater,
        (None, None) => to_string_normalized(left).cmp(&to_string_normalized(right)),
    }
}

fn compare_ip(left: &Value, right: &Value) -> Ordering {
    match (to_ip(left), to_ip(right)) {
        (Some(left_ip), Some(right_ip)) => left_ip.cmp(&right_ip),
        (Some(_), None) => Ordering::Less,
        (None, Some(_)) => Ordering::Greater,
        (None, None) => to_string_normalized(left).cmp(&to_string_normalized(right)),
    }
}

fn to_f64(value: &Value) -> Option<f64> {
    match value {
        Value::Number(number) => number.as_f64(),
        Value::String(text) => text.parse::<f64>().ok(),
        Value::Bool(flag) => Some(if *flag { 1.0 } else { 0.0 }),
        _ => None,
    }
    .filter(|number| !number.is_nan())
}

fn to_ip(value: &Value) -> Option<IpAddr> {
    match value {
        Value::String(text) => text.parse::<IpAddr>().ok(),
        _ => None,
    }
}

fn to_string_normalized(value: &Value) -> String {
    match value {
        Value::String(text) => text.to_ascii_lowercase(),
        _ => value.to_string().to_ascii_lowercase(),
    }
}

#[cfg(test)]
mod tests;
