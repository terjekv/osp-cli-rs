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
        flatten::{coalesce_flat_row, flatten_row},
        matchers::{
            contains_case_insensitive, eq_case_insensitive, fuzzy_contains_case_insensitive,
            match_row_keys_detailed, match_row_keys_detailed_fuzzy, render_value,
        },
        resolve::{is_truthy, resolve_pairs, resolve_values_truthy},
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

pub(crate) fn apply_with_plan(rows: Vec<Row>, plan: &QuickPlan) -> Result<Vec<Row>> {
    Ok(rows
        .into_iter()
        .filter(|row| plan.matches_row_filter_mode(row))
        .collect())
}

#[cfg(test)]
pub(crate) fn apply_groups_with_plan(groups: Vec<Group>, plan: &QuickPlan) -> Result<Vec<Group>> {
    map_group_rows(groups, |rows| apply_with_plan(rows, plan))
}

fn matches_row(row: &Row, spec: &CompiledQuickSpec) -> bool {
    if let Some(matched) =
        match_structural_value(&Value::Object(coalesce_flat_row(&flatten_row(row))), spec)
    {
        return matched;
    }

    if spec.existence() {
        let found = resolve_values_truthy(row, spec.token(), spec.exact());
        return if spec.negated() { !found } else { found };
    }

    let flat = flatten_row(row);
    let matches = if spec.fuzzy {
        match_row_keys_detailed_fuzzy(&flat, spec.token(), spec.exact())
    } else {
        match_row_keys_detailed(&flat, spec.token(), spec.exact())
    };
    let key_hits = if matches.exact.is_empty() {
        matches.partial
    } else {
        matches.exact
    };
    let (value_hit, synthetic_hit) = match spec.scope {
        QuickScope::ValueOnly => (
            flat.values()
                .any(|value| value_matches_token(value, spec.token(), spec.exact(), spec.fuzzy)),
            false,
        ),
        QuickScope::KeyOnly => (false, false),
        QuickScope::KeyOrValue => {
            let (pairs, _) = resolve_pairs(&flat, spec.token());
            let value_hit = pairs.iter().any(|(_, value)| {
                value_matches_token(value, spec.token(), spec.exact(), spec.fuzzy)
            });
            let synthetic_hit = pairs.iter().any(|(key, _)| !flat.contains_key(key));
            (value_hit, synthetic_hit)
        }
    };

    let matched = match spec.scope {
        QuickScope::KeyOnly if spec.key_not_equals => {
            let key_set = key_hits.iter().collect::<HashSet<_>>();
            flat.keys().any(|key| !key_set.contains(key))
        }
        QuickScope::KeyOnly => !key_hits.is_empty(),
        QuickScope::ValueOnly => value_hit,
        QuickScope::KeyOrValue => !key_hits.is_empty() || value_hit || synthetic_hit,
    };

    if spec.negated() { !matched } else { matched }
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

fn match_structural_value(root: &Value, spec: &CompiledQuickSpec) -> Option<bool> {
    if !spec.is_structural() || spec.key_not_equals {
        return None;
    }

    let matches = spec.selector.resolve_matches(root);
    let found = if spec.existence() {
        matches.iter().any(|entry| is_truthy(&entry.value))
    } else if matches.is_empty() {
        return None;
    } else {
        true
    };
    Some(if spec.negated() { !found } else { found })
}

#[cfg(test)]
fn apply_value(value: Value, spec: &str) -> Result<Value> {
    crate::dsl::value::apply_stage(
        value,
        &crate::dsl::compiled::CompiledStage::Quick(compile(spec)?),
    )
}

#[cfg(test)]
fn apply_value_with_plan(value: Value, plan: &QuickPlan) -> Result<Value> {
    crate::dsl::value::apply_stage(
        value,
        &crate::dsl::compiled::CompiledStage::Quick(plan.clone()),
    )
}

#[cfg(test)]
mod tests;
