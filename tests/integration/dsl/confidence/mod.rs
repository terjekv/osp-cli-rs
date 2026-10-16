use osp_cli::core::document::Document;
use osp_cli::core::output_model::{OutputItems, OutputResult};
use osp_cli::core::row::Row;
use osp_cli::dsl::{apply_output_pipeline, parse_pipeline};
use serde_json::Value;

mod addressed;
mod rows;
mod semantics;
mod values;

fn row(value: Value) -> Row {
    value
        .as_object()
        .cloned()
        .expect("fixture should be an object")
}

fn run_rows_pipeline(rows: Vec<Row>, pipeline: &str) -> OutputResult {
    let parsed = parse_pipeline(&format!("fixture | {pipeline}")).expect("pipeline should parse");
    apply_output_pipeline(OutputResult::from_rows(rows), &parsed.stages)
        .expect("pipeline should succeed")
}

fn run_guide_pipeline(view: Document, pipeline: &str) -> OutputResult {
    let parsed = parse_pipeline(&format!("fixture | {pipeline}")).expect("pipeline should parse");
    apply_output_pipeline(view.to_output_result(), &parsed.stages).expect("pipeline should succeed")
}

fn help_like_guide() -> Document {
    Document::from_help_text(
        "Deploy commands\n\nUsage: osp deploy <COMMAND>\n\nCommands:\n  apply   Apply pending changes\n  doctor  Inspect runtime health\n  status  Show deployment status\n\nOptions:\n  --verbose  Show additional context\n  --json     Render machine-readable output\n\nNotes:\nRun `doctor` before applying production changes.\n",
    )
}

fn help_like_record() -> Value {
    let commands = serde_json::json!([
        {"name":"apply","short_help":"Apply pending changes"},
        {"name":"doctor","short_help":"Inspect runtime health"},
        {"name":"status","short_help":"Show deployment status"},
    ]);
    let options = serde_json::json!([
        {"name":"--verbose","short_help":"Show additional context"},
        {"name":"--json","short_help":"Render machine-readable output"},
    ]);
    serde_json::json!({"preamble":["Deploy commands"], "usage":["osp deploy <COMMAND>"],
    "commands":commands.clone(), "options":options.clone(),
    "notes":["Run `doctor` before applying production changes."], "epilogue":["footer text"],
    "sections":[
        {"title":"Commands","kind":"commands","paragraphs":["pick one"],"entries":commands},
        {"title":"Options","kind":"options","paragraphs":["rendering"],"entries":options},
    ]})
}

// Structured JSON fixtures deliberately use the service/document boundary;
// guide fixtures above use the declared guide-content boundary.
fn run_document_pipeline(view: Value, pipeline: &str) -> OutputResult {
    let parsed = parse_pipeline(&format!("fixture | {pipeline}")).expect("pipeline should parse");
    let output = OutputResult::from_rows(Vec::new())
        .with_document(osp_cli::core::output_model::OutputDocument::Json(view));
    apply_output_pipeline(output, &parsed.stages).expect("pipeline should succeed")
}

fn result_rows(output: &OutputResult) -> Value {
    serde_json::to_value(output.as_rows().expect("canonical rows")).unwrap()
}
