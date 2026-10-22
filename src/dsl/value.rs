//! JSON fixtures enter through the same command-output adapter as plugins.
use super::compiled::{CompiledPipeline, CompiledStage};
use anyhow::Result;
use serde_json::Value;

pub(crate) fn apply_stage(value: Value, stage: &CompiledStage) -> Result<Value> {
    let output = crate::cli::rows::output::plugin_data_to_output_result(value, None);
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
