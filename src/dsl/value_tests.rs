//! JSON fixtures enter through the canonical core row normalization.
use super::compiled::{CompiledPipeline, CompiledStage};
use crate::core::output_model::{OutputResult, rows_from_value};
use anyhow::Result;
use serde_json::Value;

pub(crate) fn apply_stage(value: Value, stage: &CompiledStage) -> Result<Value> {
    let output = OutputResult::from_rows(rows_from_value(value));
    let output = super::engine::run_compiled(
        output,
        &CompiledPipeline {
            stages: vec![stage.clone()],
        },
    )?;
    Ok(crate::ui::json_value(&output))
}

#[path = "tests/value_semantics.rs"]
mod tests;
