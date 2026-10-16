use super::*;
use serde_json::json;

#[test]
fn presentation_recommendations_do_not_change_pipeline_semantics() {
    use osp_cli::core::output::OutputFormat;
    use osp_cli::core::output_model::{OutputDocument, RenderRecommendation};

    let rows = vec![osp_cli::row! {
        "members" => json!([{"role": "primary"}, {"role": "secondary"}])
    }];
    for document in [
        None,
        Some(OutputDocument::Json(Value::Object(rows[0].clone()))),
    ] {
        let mut output = OutputResult::from_rows(rows.clone());
        output.document = document;
        let stages = ["F role=primary".to_string()];
        let expected = apply_output_pipeline(output.clone(), &stages).unwrap();
        for format in [
            OutputFormat::Mreg,
            OutputFormat::Markdown,
            OutputFormat::Table,
            OutputFormat::Json,
        ] {
            output.meta.render_recommendation = Some(RenderRecommendation::Format(format));
            let actual = apply_output_pipeline(output.clone(), &stages).unwrap();
            assert_eq!(actual.items, expected.items);
            assert_eq!(actual.document, expected.document);
        }
    }
}

// Protects the happy semantic path: a narrowed guide payload should still round
// trip through the DSL and restore as a guide rather than degrading to generic
// rows.
#[test]
fn help_like_payload_restores_after_narrowing_multistage_pipeline() {
    let output = run_guide_pipeline(help_like_guide(), "status | ? | S name | L 1");
    assert!(output.meta.document_rows);
    assert!(output.document.is_none());
    assert_eq!(output.as_rows().unwrap().len(), 1);
    assert_eq!(output.as_rows().unwrap()[0]["name"], "status");
    assert_eq!(output.as_rows().unwrap()[0]["section"], "Commands");
}

// Protects the blunt quick contract for guide-shaped data: descendant matches
// keep only the root fields that matched, while matching array elements survive
// as whole original elements.
#[test]
fn help_like_payload_quick_prunes_unmatched_root_fields_for_partial_match() {
    let output = run_guide_pipeline(help_like_guide(), "status | ? | L 1");
    assert!(output.meta.document_rows);
    assert!(output.document.is_none());
    assert_eq!(output.as_rows().unwrap().len(), 1);
    assert_eq!(output.as_rows().unwrap()[0]["name"], "status");
    assert_eq!(output.as_rows().unwrap()[0]["section"], "Commands");
}

// Protects the degrade path: once a semantic payload is structurally reshaped
// into generic value rows, restore must stop rather than fabricating guide
// semantics from the new shape.
#[test]
fn help_like_payload_does_not_restore_after_value_extraction_pipeline() {
    let output = run_guide_pipeline(
        help_like_guide(),
        "F name ~ ^[a-z] | P name | VALUE name | S value | L 2",
    );

    assert!(!output.meta.document_rows);
    let OutputItems::Rows(rows) = output.items else {
        panic!("expected flat value rows");
    };
    assert_eq!(
        rows,
        vec![
            row(json!({"value": "apply"})),
            row(json!({"value": "doctor"})),
        ]
    );
}

// Protects semantic JQ fanout on guide-shaped payloads: jq multi-result output
// should materialize as one value row per result instead of a single wrapped
// array row.
#[test]
fn help_like_payload_jq_fans_out_scalar_results() {
    let output = run_guide_pipeline(help_like_guide(), "F name ~ ^[a-z] | JQ '.[].name'");

    assert!(!output.meta.document_rows);
    let OutputItems::Rows(rows) = output.items else {
        panic!("expected flat value rows");
    };
    assert_eq!(
        rows,
        vec![
            row(json!({"value": "apply"})),
            row(json!({"value": "doctor"})),
            row(json!({"value": "status"})),
        ]
    );
}

// Protects semantic JQ emptiness: jq `empty` should yield no rows, not a
// synthetic `null` row.
#[test]
fn help_like_payload_jq_empty_yields_no_rows() {
    let output = run_guide_pipeline(help_like_guide(), "JQ 'empty'");

    let OutputItems::Rows(rows) = output.items else {
        panic!("expected flat row output");
    };
    assert!(rows.is_empty());
}

// Protects the new semantic unroll path: nested entry arrays should duplicate
// their parent section shell per entry instead of flattening into anonymous
// row-like fragments.
#[test]
fn help_like_payload_unroll_preserves_parent_section_shell() {
    let output = run_document_pipeline(help_like_record(), "U entries");

    assert!(!output.meta.document_rows);
    assert!(output.document.is_none());
    let value = result_rows(&output)[0].clone();
    assert_eq!(
        value,
        json!({
            "preamble": ["Deploy commands"],
            "usage": ["osp deploy <COMMAND>"],
            "commands": [
                {"name": "apply", "short_help": "Apply pending changes"},
                {"name": "doctor", "short_help": "Inspect runtime health"},
                {"name": "status", "short_help": "Show deployment status"}
            ],
            "options": [
                {"name": "--verbose", "short_help": "Show additional context"},
                {"name": "--json", "short_help": "Render machine-readable output"}
            ],
            "notes": ["Run `doctor` before applying production changes."],
            "sections": [
                {
                    "title": "Commands",
                    "kind": "commands",
                    "paragraphs": ["pick one"],
                    "entries": {"name": "apply", "short_help": "Apply pending changes"}
                },
                {
                    "title": "Commands",
                    "kind": "commands",
                    "paragraphs": ["pick one"],
                    "entries": {"name": "doctor", "short_help": "Inspect runtime health"}
                },
                {
                    "title": "Commands",
                    "kind": "commands",
                    "paragraphs": ["pick one"],
                    "entries": {"name": "status", "short_help": "Show deployment status"}
                },
                {
                    "title": "Options",
                    "kind": "options",
                    "paragraphs": ["rendering"],
                    "entries": {"name": "--verbose", "short_help": "Show additional context"}
                },
                {
                    "title": "Options",
                    "kind": "options",
                    "paragraphs": ["rendering"],
                    "entries": {"name": "--json", "short_help": "Render machine-readable output"}
                }
            ],
            "epilogue": ["footer text"]
        })
    );
}

// Protects fuzzy quick as a permissive end-to-end semantic feature:
// typo-tolerant guide narrowing should still restore cleanly, keep the useful
// envelope, and retain the intended near-hit without requiring a single exact
// survivor.
#[test]
fn help_like_payload_fuzzy_quick_restores_typo_matched_command() {
    let output = run_guide_pipeline(help_like_guide(), "K name | %docter | ?");

    assert!(output.meta.document_rows);
    assert!(output.document.is_none());
    assert!(
        output
            .as_rows()
            .unwrap()
            .iter()
            .any(|row| row["name"] == "doctor")
    );
    assert!(
        output
            .as_rows()
            .unwrap()
            .iter()
            .filter(|row| row["name"] == "doctor")
            .all(|row| row["section"] == "Commands")
    );
}

// Protects render recommendation on guide-shaped payloads: narrowing,
// cleanup, sorting, limiting, and copy should keep the guide recommendation so
// callers can still render the surviving semantic payload as guide output.
#[test]
fn help_like_payload_keeps_render_recommendation_through_non_structural_stages() {
    for pipeline in ["doctor | ? | L 1", "S name", "Y"] {
        let output = run_guide_pipeline(help_like_guide(), pipeline);
        assert_eq!(
            output.meta.render_recommendation,
            Some(osp_cli::core::output_model::RenderRecommendation::Document),
            "pipeline={pipeline}"
        );
    }
}

// Protects the degrade boundary for guide-shaped payloads: once the pipeline
// structurally reshapes or materializes the payload, the guide render hint must
// clear instead of surviving into generic row output.
#[test]
fn help_like_payload_clears_render_recommendation_after_structural_reshapes() {
    for pipeline in ["P name", "U name", "VALUE name"] {
        let output = run_guide_pipeline(help_like_guide(), pipeline);
        assert_eq!(
            output.meta.render_recommendation, None,
            "pipeline={pipeline}"
        );
    }
}
