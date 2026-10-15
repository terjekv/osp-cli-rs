//! Exact integer coercion shared by numeric DSL operations.

use serde_json::Value;

pub(crate) fn integer_value(value: &Value) -> Option<i128> {
    match value {
        Value::Number(number) => number
            .as_i64()
            .map(i128::from)
            .or_else(|| number.as_u64().map(i128::from)),
        Value::String(text) => parse_integer(text),
        _ => None,
    }
}

pub(crate) fn parse_integer(text: &str) -> Option<i128> {
    text.parse::<i64>()
        .ok()
        .map(i128::from)
        .or_else(|| text.parse::<u64>().ok().map(i128::from))
}
