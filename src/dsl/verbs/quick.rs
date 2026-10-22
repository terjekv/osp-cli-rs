#[cfg(test)]
use crate::dsl::verbs::common::map_group_rows;
use std::collections::HashSet;

use anyhow::{Result, anyhow};
use serde_json::Value;

#[cfg(test)]
use crate::core::output_model::Group;
use crate::core::row::Row;
use crate::dsl::{
    eval::{
        flatten::{coalesce_flat_row, coalesce_flat_row_with_fill, flatten_row},
        matchers::{
            contains_case_insensitive, eq_case_insensitive, fuzzy_contains_case_insensitive,
            match_row_keys_detailed, match_row_keys_detailed_fuzzy, render_value,
        },
        resolve::{compact_sparse_arrays, resolve_pairs, resolve_values_truthy, sparse_hole},
    },
    parse::{
        key_spec::ExactMode,
        lexer::{Span, StageSegment, tokenize_stage_words},
        quick::{QuickScope, parse_quick_spec},
    },
};

use super::selector;

#[derive(Debug, Clone)]
pub(crate) struct QuickPlan {
    spec: CompiledQuickSpec,
}

#[derive(Debug, Clone)]
struct CompiledQuickSpec {
    scope: QuickScope,
    selector: selector::CompiledSelector,
    key_not_equals: bool,
    fuzzy: bool,
}

impl CompiledQuickSpec {
    fn from_parsed(spec: crate::dsl::parse::quick::QuickSpec) -> Self {
        Self {
            scope: spec.scope,
            selector: selector::CompiledSelector::from_key_spec(spec.key_spec),
            key_not_equals: spec.key_not_equals,
            fuzzy: spec.fuzzy,
        }
    }

    fn token(&self) -> &str {
        self.selector.token()
    }

    fn exact(&self) -> ExactMode {
        self.selector.exact()
    }

    fn negated(&self) -> bool {
        self.selector.key_spec.negated
    }

    fn existence(&self) -> bool {
        self.selector.key_spec.existence
    }

    fn is_structural(&self) -> bool {
        self.selector.is_structural()
    }
}

impl QuickPlan {
    /// Whether this stage narrows fields instead of filtering rows: quick
    /// search reshapes a lone object, except for presence checks.
    pub(crate) fn narrows(&self, row_count: usize) -> bool {
        row_count == 1 && !self.spec.existence()
    }

    pub(crate) fn matches_row_filter_mode(&self, row: &Row) -> bool {
        matches_row(row, &self.spec)
    }
}

pub(crate) fn compile(raw_stage: &str) -> Result<QuickPlan> {
    let mut parsed = parse_quick_spec(raw_stage);
    parsed.key_spec.token = decode_search_token(&parsed.key_spec.token)?;
    let spec = CompiledQuickSpec::from_parsed(parsed);
    let token = spec.token().trim();
    if token.is_empty() {
        return Err(anyhow!("quick stage requires a search token"));
    }
    if spec.fuzzy {
        if spec.existence() {
            return Err(anyhow!(
                "% quick does not support existence filters; use plain ?path or literal quick"
            ));
        }
        if !matches!(spec.exact(), ExactMode::None) || spec.key_not_equals {
            return Err(anyhow!(
                "% quick does not support exact-match key operators; use plain quick operators"
            ));
        }
        if spec.is_structural() {
            return Err(anyhow!(
                "% quick does not support path selectors; use plain path quick instead"
            ));
        }
    }
    Ok(QuickPlan { spec })
}

fn decode_search_token(raw_token: &str) -> Result<String> {
    let segment = StageSegment {
        raw: raw_token.to_string(),
        span: Span {
            start: 0,
            end: raw_token.len(),
        },
    };
    let words = tokenize_stage_words(&segment)?;
    let mut decoded = String::with_capacity(raw_token.len());
    let mut previous_end = 0;
    for (index, word) in words.iter().enumerate() {
        if index > 0 {
            decoded.push_str(&raw_token[previous_end..word.span.start]);
        }
        decoded.push_str(&word.text);
        previous_end = word.span.end;
    }
    Ok(decoded)
}

pub(crate) fn apply_with_plan(mut rows: Vec<Row>, plan: &QuickPlan) -> Result<Vec<Row>> {
    if plan.narrows(rows.len()) {
        return Ok(narrow_row(rows.remove(0), &plan.spec));
    }
    Ok(rows
        .into_iter()
        .filter(|row| plan.matches_row_filter_mode(row))
        .collect())
}

#[cfg(test)]
#[cfg_attr(coverage_nightly, coverage(off))]
pub(crate) fn apply_groups_with_plan(groups: Vec<Group>, plan: &QuickPlan) -> Result<Vec<Group>> {
    map_group_rows(groups, |rows| apply_with_plan(rows, plan))
}

fn matches_row(row: &Row, spec: &CompiledQuickSpec) -> bool {
    if spec.existence() {
        let found = resolve_values_truthy(row, spec.token(), spec.exact());
        return if spec.negated() { !found } else { found };
    }

    match_flat_row(&flatten_row(row), spec).matched
}

struct RowMatch {
    matched: bool,
    key_hits: HashSet<String>,
    value_hits: HashSet<String>,
    synthetic: Row,
    projection: bool,
}

fn match_flat_row(flat: &Row, spec: &CompiledQuickSpec) -> RowMatch {
    let matches = if matches!(spec.scope, QuickScope::ValueOnly) {
        Default::default()
    } else if spec.fuzzy {
        match_row_keys_detailed_fuzzy(flat, spec.token(), spec.exact())
    } else {
        match_row_keys_detailed(flat, spec.token(), spec.exact())
    };
    let mut projection = !matches.exact.is_empty();
    let mut key_hits = if matches.exact.is_empty() {
        matches.partial
    } else {
        matches.exact
    }
    .into_iter()
    .collect::<HashSet<_>>();
    if spec.is_structural() && !matches!(spec.scope, QuickScope::ValueOnly) {
        let root = Value::Object(coalesce_flat_row(flat));
        let addressed = spec.selector.resolve_matches(&root);
        if !addressed.is_empty() {
            projection = true;
            key_hits = flat
                .keys()
                .filter(|key| {
                    addressed
                        .iter()
                        .any(|entry| is_within(key, &entry.flat_key))
                })
                .cloned()
                .collect();
        }
    }
    let mut synthetic = if !spec.is_structural() || matches!(spec.scope, QuickScope::ValueOnly) {
        Row::new()
    } else {
        resolve_pairs(flat, spec.token())
            .0
            .into_iter()
            .filter(|(parent, _)| {
                !flat.contains_key(parent) && key_hits.iter().any(|key| is_within(key, parent))
            })
            .collect::<Row>()
    };
    // Values are searched in every field, not only the fields whose names
    // matched: `| uio` keeps `uioAffiliation` and each netgroup containing uio.
    let value_hits = flat
        .iter()
        .filter(|(_, value)| {
            !matches!(spec.scope, QuickScope::KeyOnly)
                && value_matches_token(value, spec.token(), spec.exact(), spec.fuzzy)
        })
        .map(|(key, _)| key.clone())
        .collect::<HashSet<_>>();
    if spec.key_not_equals {
        key_hits = flat
            .keys()
            .filter(|key| !key_hits.contains(*key))
            .cloned()
            .collect();
        synthetic.clear();
        projection = false;
    }
    let matched = !key_hits.is_empty() || !value_hits.is_empty() || !synthetic.is_empty();

    RowMatch {
        matched: if spec.negated() { !matched } else { matched },
        key_hits,
        value_hits,
        synthetic,
        projection,
    }
}

fn narrow_row(row: Row, spec: &CompiledQuickSpec) -> Vec<Row> {
    let flat = flatten_row(&row);
    let result = match_flat_row(&flat, spec);
    let negated = spec.negated();
    if !result.matched && !negated {
        return Vec::new();
    }

    // Addressed containers and selector fanout can become standalone rows.
    // Bare field names keep the field and its complete value in place.
    if result.projection && !negated && !result.synthetic.is_empty() {
        return result
            .synthetic
            .into_iter()
            .map(|(key, value)| {
                let projected = rebuild_row(&[(key, value)].into_iter().collect());
                if projected.len() == 1 {
                    match projected.values().next() {
                        Some(Value::Object(map)) => return map.clone(),
                        Some(Value::Array(items)) if items.len() == 1 => {
                            if let Value::Object(map) = &items[0] {
                                return map.clone();
                            }
                        }
                        _ => {}
                    }
                }
                projected
            })
            .filter(|row| !row.is_empty())
            .collect();
    }

    let values_count = negated || !result.projection;
    // A value inside a list of objects keeps its whole object, so a matching
    // member stays readable: `commands[0].name` keeps all of `commands[0]`.
    let members = result
        .value_hits
        .iter()
        .filter(|_| values_count)
        .filter_map(|key| key.find("].").map(|end| &key[..=end]))
        .collect::<HashSet<_>>();
    let narrowed = flat
        .into_iter()
        .filter(|(key, _)| {
            let selected = result.key_hits.contains(key)
                || (values_count && result.value_hits.contains(key))
                || members.iter().any(|member| is_within(key, member));
            selected != negated
        })
        .collect::<Row>();
    if narrowed.is_empty() {
        Vec::new()
    } else {
        vec![rebuild_row(&narrowed)]
    }
}

fn is_within(key: &str, parent: &str) -> bool {
    key == parent
        || key
            .strip_prefix(parent)
            .is_some_and(|rest| rest.starts_with('.') || rest.starts_with('['))
}

fn rebuild_row(flat: &Row) -> Row {
    let mut row = coalesce_flat_row_with_fill(flat, &sparse_hole());
    for value in row.values_mut() {
        compact_sparse_arrays(value);
    }
    row
}

fn value_matches_token(value: &Value, token: &str, exact: ExactMode, fuzzy: bool) -> bool {
    match exact {
        ExactMode::CaseSensitive => match value {
            Value::Array(values) => values
                .iter()
                .any(|item| value_matches_token(item, token, exact, fuzzy)),
            scalar => render_value(scalar) == token,
        },
        ExactMode::CaseInsensitive => match value {
            Value::Array(values) => values
                .iter()
                .any(|item| value_matches_token(item, token, exact, fuzzy)),
            scalar => eq_case_insensitive(&render_value(scalar), token),
        },
        ExactMode::None => match value {
            Value::Array(values) => values
                .iter()
                .any(|item| value_matches_token(item, token, exact, fuzzy)),
            scalar if fuzzy => fuzzy_contains_case_insensitive(&render_value(scalar), token),
            scalar => contains_case_insensitive(&render_value(scalar), token),
        },
    }
}

#[cfg(test)]
#[cfg_attr(coverage_nightly, coverage(off))]
fn apply_value(value: Value, spec: &str) -> Result<Value> {
    crate::dsl::value::apply_stage(
        value,
        &crate::dsl::compiled::CompiledStage::Quick(compile(spec)?),
    )
}

#[cfg(test)]
#[cfg_attr(coverage_nightly, coverage(off))]
fn apply_value_with_plan(value: Value, plan: &QuickPlan) -> Result<Value> {
    crate::dsl::value::apply_stage(
        value,
        &crate::dsl::compiled::CompiledStage::Quick(plan.clone()),
    )
}

#[cfg(test)]
#[cfg_attr(coverage_nightly, coverage(off))]
mod tests;
