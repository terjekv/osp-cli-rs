//! Public document contracts migrated from the removed guide model.

use super::{RenderSettings, render};
use crate::core::document::{DocBlock, Document, DocumentEntry, HelpLevel};
use crate::core::output::OutputFormat;
use crate::core::output_model::OutputResult;
use serde_json::json;

fn settings(format: OutputFormat) -> RenderSettings {
    let mut settings = RenderSettings::test_plain(format);
    settings.format_explicit = true;
    settings
}

#[test]
fn filtered_for_help_level_hides_verbose_sections_until_requested_unit() {
    let mut document = Document::from_help_text(
        "Usage: osp [COMMAND]\n\nCommands:\n  help  Show help\n\nCommon Invocation Options:\n  --json  Render as JSON\n",
    );
    document.blocks.push(DocBlock::section(
        "Notes",
        vec![DocBlock::Paragraph {
            text: "extra note".into(),
        }],
    ));
    let tiny = document.filtered_for_help_level(HelpLevel::Tiny).rows();
    assert_eq!(tiny.len(), 1);
    assert_eq!(tiny[0]["section"], "Usage");
    let normal = document.filtered_for_help_level(HelpLevel::Normal).rows();
    assert!(normal.iter().any(|row| row["section"] == "Commands"));
    assert!(normal.iter().any(|row| row["section"] == "Notes"));
    assert!(
        normal
            .iter()
            .any(|row| row["section"] == "Common Invocation Options")
    );
    let verbose = document.filtered_for_help_level(HelpLevel::Verbose).rows();
    assert!(
        verbose
            .iter()
            .any(|row| row["section"] == "Common Invocation Options")
    );
}

#[test]
fn document_serialization_preserves_parsed_and_authored_order_unit() {
    let help =
        Document::from_help_text("Usage: osp history <COMMAND>\n\nCommands:\n  help  Print help\n");
    for document in [
        help.clone(),
        Document::from_markdown_template("## OSP\nWelcome\n\n{{ help }}\n\n## Tail\nDone", &help),
        Document {
            blocks: vec![DocBlock::Preformatted {
                lines: vec![vec![
                    crate::core::TextSpan::new("Ready: ", crate::core::TextRole::Key),
                    crate::core::TextSpan::new("complete", crate::core::TextRole::Success),
                ]],
            }],
        },
    ] {
        let output = document.to_output_result();
        let json = render(
            &output,
            &crate::ui::RenderRequest::new(&settings(OutputFormat::Json)),
        );
        assert_eq!(
            serde_json::from_str::<serde_json::Value>(&json).unwrap(),
            serde_json::to_value(&document).unwrap()
        );
        assert!(output.meta.document_rows);
        assert_eq!(output.as_rows().unwrap(), document.rows());
    }
}

#[test]
fn document_intent_is_explicit_and_ordinary_records_remain_data_unit() {
    assert!(serde_json::from_value::<Document>(json!({"commands":[{"name":"list"}]})).is_err());
    let output = OutputResult::from_rows(vec![crate::row! {
        "id" => 42, "status" => "failed", "notes" => json!(["check power"]),
    }]);
    assert!(!output.meta.document_rows);
    for format in [
        OutputFormat::Auto,
        OutputFormat::Markdown,
        OutputFormat::Mreg,
    ] {
        let rendered = render(&output, &crate::ui::RenderRequest::new(&settings(format)));
        assert!(
            rendered.contains("42")
                && rendered.contains("failed")
                && rendered.contains("check power")
        );
    }
}

#[test]
fn document_markdown_surfaces_sections_and_keeps_entry_rows_unit() {
    let document = Document::from_help_text(
        "Usage: history <COMMAND>\n\nCommands:\n  list     List history entries\n  plugins  Inspect and manage plugins\n\nOptions:\n  -h, --help  Print help\n",
    );
    let mut settings = settings(OutputFormat::Markdown);
    for width in [None, Some(90)] {
        settings.width = width;
        let markdown = render(
            &document.to_output_result(),
            &crate::ui::RenderRequest::new(&settings),
        );
        for text in [
            "## Usage",
            "history <COMMAND>",
            "## Commands",
            "- `list` List history entries",
            "- `plugins`",
            "## Options",
            "- `-h, --help` Print help",
        ] {
            assert!(markdown.contains(text), "{markdown}");
        }
        assert!(!markdown.contains("| name"));
    }
}

#[test]
fn document_values_use_content_and_preserve_object_value_order_unit() {
    let cases = [
        (
            Document {
                blocks: vec![DocBlock::section(
                    "Commands",
                    vec![DocBlock::Entries {
                        entries: vec![DocumentEntry {
                            role: Default::default(),
                            name: "config".into(),
                            description: "Inspect and edit runtime config".into(),
                        }],
                    }],
                )],
            },
            "Inspect and edit runtime config\n",
        ),
        (
            Document {
                blocks: vec![DocBlock::section(
                    "Session",
                    vec![DocBlock::Data {
                        value: json!({
                            "logged_in_as":"oistes", "theme":"rose-pine-moon", "version":"1.4.9",
                        }),
                    }],
                )],
            },
            "oistes\nrose-pine-moon\n1.4.9\n",
        ),
    ];
    for (document, expected) in cases {
        assert_eq!(
            render(
                &document.to_output_result(),
                &crate::ui::RenderRequest::new(&settings(OutputFormat::Value))
            ),
            expected
        );
    }
}
