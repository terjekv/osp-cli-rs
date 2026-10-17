//! `VALUE` / `VAL` extraction logic.
//!
//! `VALUE` is intentionally transforming rather than filtering. It resolves
//! the shared selector surface, then emits matched leaves as flat
//! `{value: ...}` rows regardless of the input substrate.
//!
//! Examples:
//! - row input `VALUE uid` becomes `[{"value": "alice"}, ...]`
//! - semantic input `VALUE sections[0].entries[0].name` produces the same flat
//!   value-row shape and deliberately discards the semantic envelope
//! - multiple selectors emit in selector order; each selector retains
//!   depth-first document order and duplicate values at distinct addresses

use crate::core::row::Row;

use anyhow::Result;
use serde_json::Value;

use crate::dsl::verbs::common::parse_terms;

use super::selector;

#[derive(Debug, Clone, Default)]
pub(crate) struct ValuesPlan {
    selectors: Vec<selector::CompiledSelector>,
}

pub(crate) fn compile(spec: &str) -> Result<ValuesPlan> {
    Ok(ValuesPlan {
        selectors: parse_terms(spec)?
            .into_iter()
            .map(|token| {
                selector::CompiledSelector::from_token(
                    token,
                    crate::dsl::parse::key_spec::ExactMode::CaseSensitive,
                )
            })
            .collect(),
    })
}

#[cfg(test)]
/// Extracts values from flat rows and emits `{value: ...}` rows.
///
/// With an empty spec, values from every field are emitted in row order.
pub fn apply(rows: Vec<Row>, spec: &str) -> Result<Vec<Row>> {
    let plan = compile(spec)?;
    apply_with_plan(rows, &plan)
}

pub(crate) fn apply_with_plan(rows: Vec<Row>, plan: &ValuesPlan) -> Result<Vec<Row>> {
    let mut out = Vec::new();
    if plan.selectors.is_empty() {
        for row in rows {
            for value in row.into_values() {
                emit_value_rows(&mut out, value);
            }
        }
    } else {
        let root = Value::Array(rows.into_iter().map(Value::Object).collect());
        for entry in selector::collect_compiled_matches(&root, &plan.selectors) {
            emit_value_rows(&mut out, entry.value);
        }
    }
    Ok(out)
}

fn emit_value_rows(out: &mut Vec<Row>, value: Value) {
    match value {
        Value::Array(values) => {
            for value in values {
                out.push(crate::row! { "value" => value });
            }
        }
        value => out.push(crate::row! { "value" => value }),
    }
}

#[cfg(test)]
fn apply_value_with_plan(value: Value, plan: &ValuesPlan) -> Result<Value> {
    crate::dsl::value::apply_stage(
        value,
        &crate::dsl::compiled::CompiledStage::Values(plan.clone()),
    )
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::{apply, apply_value_with_plan, compile};

    #[test]
    fn explodes_array_values() {
        let rows = vec![
            json!({"members": ["a", "b"]})
                .as_object()
                .cloned()
                .expect("object"),
        ];

        let output = apply(rows, "members").expect("values should work");
        assert_eq!(output.len(), 2);
    }

    #[test]
    fn emits_requested_scalar_values_and_ignores_missing_keys() {
        let rows = vec![
            json!({"uid": "oistes", "mail": "oistes@example.org"})
                .as_object()
                .cloned()
                .expect("object"),
        ];

        let output = apply(rows, "uid missing").expect("values should work");
        assert_eq!(output.len(), 1);
        assert_eq!(
            output[0].get("value").and_then(|value| value.as_str()),
            Some("oistes")
        );
    }

    #[test]
    fn empty_spec_emits_all_scalar_and_array_values_in_order() {
        let rows = vec![
            json!({"uid": "oistes", "members": ["a", "b"], "active": true})
                .as_object()
                .cloned()
                .expect("object"),
        ];

        let output = apply(rows, "").expect("empty values stage should enumerate all fields");
        let mut values = output
            .iter()
            .map(|row| {
                row.get("value")
                    .cloned()
                    .unwrap_or(serde_json::Value::Null)
                    .to_string()
            })
            .collect::<Vec<_>>();
        values.sort();

        assert_eq!(values, vec!["\"a\"", "\"b\"", "\"oistes\"", "true"]);
    }

    #[test]
    fn resolves_nested_paths_and_quoted_terms() {
        let rows = vec![
            json!({
                "metadata": {"display,name": "Alice"},
                "members": [{"uid": "alice"}, {"uid": "bob"}]
            })
            .as_object()
            .cloned()
            .expect("object"),
        ];

        let output = apply(rows, "\"metadata.display,name\" members[].uid")
            .expect("nested values should work");
        let values = output
            .iter()
            .map(|row| row.get("value").cloned().expect("value"))
            .collect::<Vec<_>>();

        assert_eq!(values, vec![json!("Alice"), json!("alice"), json!("bob")]);
    }

    #[test]
    fn extracts_top_level_scalar_arrays_from_semantic_payloads() {
        let plan = compile("usage").expect("plan should compile");
        let extracted = apply_value_with_plan(
            json!({
                "usage": ["osp deploy <COMMAND>"],
                "notes": ["read this first"],
                "sections": [
                    {
                        "title": "Commands",
                        "entries": [
                            {"name": "deploy", "short_help": "Apply changes"}
                        ]
                    }
                ]
            }),
            &plan,
        )
        .expect("semantic value extraction should succeed");

        assert_eq!(
            extracted,
            json!([
                {"value": "osp deploy <COMMAND>"}
            ])
        );
    }

    #[test]
    fn extracts_addressed_nested_values_as_flat_rows() {
        let plan = compile("sections[0].entries[0].name").expect("plan should compile");
        let extracted = apply_value_with_plan(
            json!({
                "preamble": ["Deploy commands"],
                "sections": [
                    {
                        "title": "Commands",
                        "kind": "commands",
                        "paragraphs": ["pick one"],
                        "entries": [
                            {"name": "deploy", "short_help": "Apply changes"},
                            {"name": "status", "short_help": "Inspect deployment"}
                        ]
                    }
                ]
            }),
            &plan,
        )
        .expect("semantic value extraction should succeed");

        assert_eq!(
            extracted,
            json!([
                {"value": "deploy"}
            ])
        );
    }

    #[test]
    fn missing_semantic_value_path_returns_no_rows() {
        let plan = compile("missing.path").expect("plan should compile");
        let extracted = apply_value_with_plan(
            json!({
                "usage": ["osp deploy <COMMAND>"]
            }),
            &plan,
        )
        .expect("semantic value extraction should succeed");

        assert_eq!(extracted, json!([]));
    }
}
