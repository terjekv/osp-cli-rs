use osp_cli::core::document::{DocBlock, Document};
use osp_cli::core::output::OutputFormat;
use osp_cli::dsl::{apply_output_pipeline, parse_pipeline};
use osp_cli::ui::{RenderSettings, render_output};
use serde_json::json;

fn run_guide_pipeline(view: Document, pipeline: &str) -> osp_cli::core::output_model::OutputResult {
    let parsed = parse_pipeline(&format!("fixture | {pipeline}")).expect("pipeline should parse");
    apply_output_pipeline(view.to_output_result(), &parsed.stages).expect("pipeline should succeed")
}

fn sample_guide() -> Document {
    Document::from_help_text(
        "Usage: osp history <COMMAND>\n\nCommands:\n  list   List history entries\n  clear  Clear history entries\n",
    )
}

#[test]
fn guide_payload_narrowing_restores_and_renders_as_markdown_guide() {
    let output = run_guide_pipeline(sample_guide(), "list | ? | L 1");
    assert!(output.meta.document_rows);
    assert!(output.document.is_none());
    assert_eq!(output.as_rows().unwrap().len(), 1);
    assert_eq!(output.as_rows().unwrap()[0]["name"], "list");

    let mut settings = RenderSettings::test_plain(OutputFormat::Markdown);
    settings.width = Some(80);
    settings.format_explicit = true;
    let markdown = render_output(&output, &settings);

    assert!(markdown.contains("list"));
    assert!(markdown.contains("List history entries"));
    assert!(!markdown.contains("clear"));
    assert!(!markdown.contains("| name"));
}

#[test]
fn one_shot_help_pipeline_explains_sort_syntax_and_examples() {
    let mut sink = osp_cli::app::BufferedUiSink::default();
    let exit = osp_cli::App::new()
        .run_with_sink(
            ["osp", "--defaults-only", "--plain", "--help", "|", "H", "S"],
            &mut sink,
        )
        .unwrap();
    assert_eq!(exit, 0);
    let text = sink.stdout.split_whitespace().collect::<Vec<_>>().join(" ");
    assert!(
        text.contains("S field [asc|desc] [AS num|str|ip]"),
        "{text}"
    );
    assert!(text.contains("S -created_at id"), "{text}");
    assert!(sink.stderr.is_empty());
}

#[test]
fn guide_payload_value_extraction_degrades_and_renders_as_plain_values() {
    let output = run_guide_pipeline(sample_guide(), "P name | VALUE name | S value | L 2");
    assert!(!output.meta.document_rows);

    let rendered = render_output(&output, &RenderSettings::test_plain(OutputFormat::Value));
    assert!(rendered.contains("clear"));
    assert!(rendered.contains("list"));
    assert!(!rendered.contains("Usage"));
    assert!(!rendered.contains("Commands"));
}

#[test]
fn clap_command_metadata_projects_into_guides() {
    use clap::{Arg, ArgAction, Command, ValueHint, builder::PossibleValue};

    let command = Command::new("plugins")
        .about("Manage plugin providers")
        .long_about("Long plugin management help")
        .before_help("Plugin preamble")
        .after_help("Plugin epilogue")
        .visible_alias("ext")
        .arg(
            Arg::new("roots")
                .long_help(" Roots to scan ")
                .help_heading("Input")
                .num_args(1..)
                .value_name("ROOT")
                .value_hint(ValueHint::DirPath)
                .default_values(["./plugins"])
                .value_parser([
                    PossibleValue::new("builtin").help("Built-in plugins"),
                    PossibleValue::new("custom").help("Custom plugins"),
                    PossibleValue::new("hidden").help("Hidden").hide(true),
                ]),
        )
        .arg(
            Arg::new("format")
                .long("format")
                .visible_alias("fmt")
                .short('f')
                .visible_short_alias('o')
                .help_heading("Output")
                .long_help(" Output format ")
                .action(ArgAction::Append)
                .value_name("FORMAT")
                .default_values(["json"])
                .value_parser([
                    PossibleValue::new("json").help("Machine readable"),
                    PossibleValue::new("text").help("Human readable"),
                    PossibleValue::new("trace").help("Hidden").hide(true),
                ]),
        )
        .arg(Arg::new("quiet").long("quiet").hide(true))
        .subcommand(Command::new("list").about("List plugins"))
        .subcommand(Command::new("doctor").about("Hidden doctor").hide(true));

    let guide = Document::from_clap_command(&command, true);
    let rows = guide.rows();
    assert_eq!(rows[0]["text"], "Plugin preamble");
    assert_eq!(rows.last().unwrap()["text"], "Plugin epilogue");
    assert!(
        rows.iter()
            .any(|row| row.get("name").is_some_and(|value| value == "list"))
    );
    assert!(
        rows.iter()
            .any(|row| row.get("name").is_some_and(|value| value == "[ROOT]..."))
    );
    assert!(
        rows.iter().any(|row| row
            .get("name")
            .is_some_and(|value| { value.as_str().is_some_and(|name| name.contains("--format")) }))
    );

    let markdown =
        osp_cli::ui::render_document(&guide, &RenderSettings::test_plain(OutputFormat::Markdown));
    assert!(markdown.contains("List plugins"));
    assert!(markdown.contains("Plugin epilogue"));
}

#[test]
fn parsed_and_authored_guides_merge_round_trip_and_render_through_semantic_output() {
    let mut parsed = Document::from_help_text(
        "Deploy overview\n\nUsage: osp deploy <COMMAND>\n\nCommands:\n  status  Show deployment status\n  apply   Apply pending changes\n\nOptions:\n  --json  Render machine output\n  --wait  Wait for completion\nHint: use doctor before prod.\n\nSession:\ncurrent profile: prod\n",
    );
    let authored = Document {
        blocks: vec![
            DocBlock::section(
                "Notes",
                vec![DocBlock::Paragraph {
                    text: ("Run doctor before deploy").into(),
                }],
            ),
            DocBlock::section(
                "Runtime",
                vec![DocBlock::Data {
                    value: json!({
                        "profile": "prod",
                        "theme": "rose-pine-moon",
                        "count": 2,
                    }),
                }],
            ),
        ],
    };

    parsed.blocks.extend(authored.blocks);
    let output = parsed.to_output_result();
    let restored: Document =
        serde_json::from_value(output.document.as_ref().unwrap().value.clone()).unwrap();
    assert_eq!(restored, parsed);
    let titles = restored
        .blocks
        .iter()
        .filter_map(|block| match block {
            DocBlock::Section { title, .. } => Some(title.as_str()),
            _ => None,
        })
        .collect::<Vec<_>>();
    assert_eq!(
        titles,
        [
            "Usage", "Commands", "Options", "Session", "Notes", "Runtime"
        ]
    );
    let mut settings = RenderSettings::test_plain(OutputFormat::Markdown);
    settings.format_explicit = true;
    let markdown = render_output(&output, &settings);
    assert!(markdown.contains("## Usage") && markdown.contains("## Session"));
    assert!(markdown.contains("current profile: prod"));
    settings.format = OutputFormat::Value;
    let values = render_output(&output, &settings);
    for value in [
        "Show deployment status",
        "Run doctor before deploy",
        "prod",
        "rose-pine-moon",
        "2",
    ] {
        assert!(values.lines().any(|line| line.trim() == value));
    }
}

#[test]
fn mixed_guide_section_data_renders_through_canonical_pipeline_end_to_end() {
    let guide = Document {
        blocks: vec![
            DocBlock::section(
                "Session",
                vec![DocBlock::Data {
                    value: json!({
                        "profile": "prod",
                        "theme": "rose-pine-moon"
                    }),
                }],
            ),
            DocBlock::section(
                "Examples",
                vec![DocBlock::Data {
                    value: json!([
                        "osp history list",
                        "osp history clear",
                        "osp history last",
                        "osp history search",
                        "osp history export",
                        "osp history import"
                    ]),
                }],
            ),
            DocBlock::section(
                "Shortcuts",
                vec![DocBlock::Data {
                    value: json!([
                        {"name": "list", "short_help": "List history"},
                        {"name": "clear", "short_help": "Clear history"}
                    ]),
                }],
            ),
            DocBlock::section(
                "Matrix",
                vec![DocBlock::Data {
                    value: json!([
                        {"uid": "alice", "state": "ok"},
                        {"uid": "bob", "state": "warn"}
                    ]),
                }],
            ),
            DocBlock::section(
                "Reservation",
                vec![DocBlock::Data {
                    value: json!({
                        "approved": true,
                        "limits": {"cpu_cores": 4, "memory_mib": 8192},
                        "owners": ["alice", "bob"],
                        "networks": [{"name": "prod", "ipv4": "192.0.2.8"}]
                    }),
                }],
            ),
            DocBlock::section("Ready", vec![DocBlock::Data { value: json!(true) }]),
            DocBlock::section("Capacity", vec![DocBlock::Data { value: json!(4) }]),
        ],
    };
    let output = guide.to_output_result();

    let mut guide_settings = RenderSettings::test_plain(OutputFormat::Auto);
    guide_settings.width = Some(40);
    let rendered = render_output(&output, &guide_settings);
    assert!(rendered.contains("Session"));
    assert!(rendered.contains("profile"));
    assert!(rendered.contains("osp history list"));
    assert!(rendered.contains("alice"));

    let mut markdown_settings = RenderSettings::test_plain(OutputFormat::Markdown);
    markdown_settings.format_explicit = true;
    let markdown = render_output(&output, &markdown_settings);
    assert!(markdown.contains("## Session"));
    assert!(markdown.contains("- profile: prod"));
    assert!(markdown.contains("| name"));
    assert!(markdown.contains("List history"));
    assert!(markdown.contains("| uid"));

    let restored: Document =
        serde_json::from_value(output.document.as_ref().unwrap().value.clone()).unwrap();
    assert_eq!(restored, guide);
    let mut json_settings = RenderSettings::test_plain(OutputFormat::Json);
    json_settings.format_explicit = true;
    let json_output = render_output(&output, &json_settings);
    assert_eq!(
        serde_json::from_str::<serde_json::Value>(&json_output).unwrap(),
        serde_json::to_value(&guide).unwrap(),
    );
    let mut value_settings = RenderSettings::test_plain(OutputFormat::Value);
    value_settings.format_explicit = true;
    let values = render_output(&output, &value_settings);
    for expected in [
        "prod",
        "rose-pine-moon",
        "List history",
        "Clear history",
        "true",
        "4",
        "alice, bob",
    ] {
        assert!(
            values.lines().any(|line| line.trim() == expected),
            "missing value {expected:?}:\n{values}"
        );
    }

    let mut styled = RenderSettings::test_plain(OutputFormat::Auto);
    styled.mode = osp_cli::core::output::RenderMode::Rich;
    styled.color = osp_cli::core::output::ColorMode::Always;
    styled.unicode = osp_cli::core::output::UnicodeMode::Always;
    styled.width = Some(60);
    let rich = render_output(&output, &styled);
    assert!(
        rich.contains('\x1b'),
        "explicit color should style the guide: {rich}"
    );
    let copied = osp_cli::ui::render_output_for_copy(&output, &styled);
    assert!(
        !copied.contains('\x1b'),
        "copied output must remain plain: {copied}"
    );
    for expected in [
        "Reservation",
        "cpu_cores",
        "memory_mib",
        "8192",
        "192.0.2.8",
        "Ready",
        "Capacity",
    ] {
        assert!(
            copied.contains(expected),
            "missing {expected:?} in copy-safe guide:\n{copied}"
        );
    }

    let mut mreg_settings = RenderSettings::test_plain(OutputFormat::Mreg);
    mreg_settings.format_explicit = true;
    let mreg = render_output(&output, &mreg_settings);
    for entry in [
        "Session",
        "profile",
        "prod",
        "theme",
        "rose-pine-moon",
        "Examples",
        "osp history list",
        "osp history clear",
        "osp history last",
        "osp history search",
        "osp history export",
        "osp history import",
        "Shortcuts",
        "list",
        "List history",
        "clear",
        "Clear history",
        "Matrix",
        "uid",
        "alice",
        "state",
        "ok",
        "bob",
        "warn",
    ] {
        assert!(
            mreg.contains(entry),
            "missing {entry:?} in Mreg guide:\n{mreg}"
        );
    }
}
