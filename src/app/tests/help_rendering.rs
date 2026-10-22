mod output_support {
    include!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/tests/support/output.rs"
    ));
}

use super::*;
use crate::cli::Cli;
use crate::core::document::{DocBlock, Document, DocumentEntry};
use crate::core::output::OutputFormat;
use crate::dsl::apply_output_pipeline;
use crate::ui::{HelpLayout, RenderSettings};
use clap::Parser;

fn help_settings() -> RenderSettings {
    RenderSettings {
        command_prefixes: crate::command_words::CommandPrefixes::Off,
        width: Some(24),
        width_max: 0,
        ..RenderSettings::test_plain(OutputFormat::Auto)
    }
}

fn render_output(
    output: &crate::core::OutputResult,
    settings: &RenderSettings,
    layout: HelpLayout,
) -> String {
    let preset = match layout {
        HelpLayout::Full => "expressive",
        HelpLayout::Compact => "compact",
        HelpLayout::Minimal => "austere",
    };
    let state = make_completion_state_with_entries(&[("ui.presentation", preset)]);
    CommandOutput::output(output.clone(), None).render(state.runtime.config(), settings)
}

pub(super) fn render_help(
    help_text: &str,
    settings: &RenderSettings,
    layout: HelpLayout,
) -> String {
    let mut output = Document::from_help_text(help_text).to_output_result();
    output.meta.document_long_help = true;
    render_output(&output, settings, layout)
}

fn normalize_help_text(text: &str) -> String {
    let mut normalized = text
        .lines()
        .map(|line| line.trim_end().replace('`', ""))
        .filter(|line| !line.is_empty())
        .collect::<Vec<_>>()
        .join("\n");
    if text.ends_with('\n') {
        normalized.push('\n');
    }
    normalized
}

fn clap_help(args: &[&str]) -> String {
    Cli::try_parse_from(args)
        .err()
        .expect("args should trigger clap help")
        .to_string()
}

fn guide_render_layout() -> HelpLayout {
    HelpLayout::Compact
}

fn filtered_guide_output() -> crate::core::output_model::OutputResult {
    let output = Document {
        blocks: vec![
            DocBlock::section(
                "Usage",
                vec![DocBlock::Paragraph {
                    text: "history <COMMAND>".to_string(),
                }],
            ),
            DocBlock::section(
                "Commands",
                vec![DocBlock::Entries {
                    entries: vec![
                        DocumentEntry {
                            role: Default::default(),
                            name: "list".to_string(),
                            description: "List history entries".to_string(),
                        },
                        DocumentEntry {
                            role: Default::default(),
                            name: "prune".to_string(),
                            description: "Remove old history entries".to_string(),
                        },
                    ],
                }],
            ),
        ],
    }
    .to_output_result();
    apply_output_pipeline(output, &["list".to_string()]).expect("guide quick should work")
}

#[test]
fn help_chrome_preserves_custom_preamble_and_extra_sections_unit() {
    let preamble = render_help(
        "Custom plugin help\nwith two intro lines\n\nUsage: osp sample\n\nCommands:\n  run\n",
        &help_settings(),
        HelpLayout::Compact,
    );
    assert!(preamble.contains("Custom plugin help"));
    assert!(preamble.contains("with two intro lines"));
    assert!(preamble.contains("Usage: osp sample"));
    assert!(preamble.contains("Commands:\n  run"));

    for layout in [HelpLayout::Compact, HelpLayout::Minimal] {
        let rendered = render_help(
            "Usage: osp sample\n\nExamples:\n  osp sample run\n\nNotes:\n  extra detail\n",
            &help_settings(),
            layout,
        );
        assert!(rendered.contains("Examples:\n  osp sample run"));
        assert!(rendered.contains("Notes:\n  extra detail"));
    }
}

#[test]
fn compact_and_austere_help_surfaces_match_clap_layout_unit() {
    for raw in [
        clap_help(&["osp", "--help"]),
        clap_help(&["osp", "theme", "--help"]),
    ] {
        for layout in [HelpLayout::Compact, HelpLayout::Minimal] {
            let rendered = render_help(&raw, &help_settings(), layout);
            assert_eq!(
                normalize_help_text(rendered.trim_end()),
                normalize_help_text(raw.trim_end())
            );
        }
    }
}

#[test]
fn compact_help_can_add_color_without_box_chrome_unit() {
    let raw = clap_help(&["osp", "--help"]);
    let mut resolved = help_settings();
    resolved.mode = RenderMode::Rich;
    resolved.color = ColorMode::Always;

    let rendered = render_help(&raw, &resolved, HelpLayout::Compact);
    assert!(rendered.contains("\u{1b}["));
    assert!(!rendered.contains('│'));
    assert!(!rendered.contains('┌'));
    assert_eq!(
        normalize_help_text(&output_support::strip_ansi(rendered.trim_end())),
        normalize_help_text(raw.trim_end())
    );
}

#[test]
fn guide_rendering_prefers_semantic_help_until_explicit_format_wins_unit() {
    let guide = Document::from_help_text("Usage: osp history <COMMAND>\n\nCommands:\n  list\n");

    let configured = RenderSettings::test_plain(OutputFormat::Json);
    let rendered = render_output(
        &guide.to_output_result(),
        &configured,
        guide_render_layout(),
    );
    assert!(rendered.contains("Usage:"));
    assert!(rendered.contains("list"));
    assert!(!rendered.trim_start().starts_with('['));

    let mut explicit = RenderSettings::test_plain(OutputFormat::Json);
    explicit.format_explicit = true;
    let rendered = render_output(&guide.to_output_result(), &explicit, guide_render_layout());
    assert!(rendered.trim_start().starts_with('{'));
}

#[test]
fn guide_output_after_pipeline_preserves_recommendation_until_explicit_format_or_markdown_overrides_unit()
 {
    let from_text = Document::from_help_text("Usage: osp history <COMMAND>\n\nCommands:\n  list\n")
        .to_output_result();
    let rehydrated = RenderSettings::test_plain(OutputFormat::Json);
    let rendered = render_output(&from_text, &rehydrated, guide_render_layout());
    assert!(rendered.contains("Usage:"));
    assert!(!rendered.trim_start().starts_with('['));

    let filtered = filtered_guide_output();
    let semantic = RenderSettings::test_plain(OutputFormat::Auto);
    let rendered = render_output(&filtered, &semantic, guide_render_layout());
    assert!(rendered.contains("Commands:"));
    assert!(rendered.contains("list"));
    assert!(!rendered.trim_start().starts_with('['));

    let mut explicit_json = RenderSettings::test_plain(OutputFormat::Json);
    explicit_json.format_explicit = true;
    let rendered = render_output(&filtered, &explicit_json, guide_render_layout());
    assert!(rendered.trim_start().starts_with('['));
    assert!(rendered.contains("\"name\""));
    assert!(rendered.contains("\"description\""));

    let mut markdown = RenderSettings::test_plain(OutputFormat::Markdown);
    markdown.format_explicit = true;
    let rendered = render_output(&filtered, &markdown, guide_render_layout());
    assert!(rendered.contains("## Commands"));
    assert!(rendered.contains("- `list` List history entries"));
    assert!(!rendered.contains("\"commands\""));
    assert!(!rendered.contains("+---"));
    assert!(!rendered.contains("| name"));
}
