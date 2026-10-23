//! Shared human-rendering contracts, independent of product field names.
use crate::core::{DocBlock, Document, MessageLevel, OutputDocument, OutputFormat, OutputResult};
use crate::ui::{RenderRequest, RenderRuntime, RenderSettings, RenderTarget, render};
use serde_json::json;

fn settings(format: OutputFormat) -> RenderSettings {
    let mut settings = RenderSettings::test_plain(format);
    settings.margin = 2;
    settings.width = Some(95);
    settings.runtime = RenderRuntime {
        stdout_is_tty: true,
        hint_action: None,
        command_scope: crate::core::CommandScope::ReplRoot,
        ..Default::default()
    };
    settings
}

#[test]
fn section_rules_start_at_column_zero() {
    let settings = settings(OutputFormat::Guide);
    let document = Document {
        blocks: vec![DocBlock::section(
            "Section marker",
            vec![DocBlock::Paragraph {
                text: "body".into(),
            }],
        )],
    };
    let mut request = RenderRequest::new(&settings);
    request.layout = Some(crate::ui::HelpLayout::Full);
    request.show_footer_rule = Some(true);
    let output = render(&document.to_output_result(), &request);
    let closing = output.lines().last().unwrap();
    assert!(
        closing.starts_with('-') && closing.chars().all(|ch| ch == '-'),
        "{output:?}"
    );
    assert!(
        output
            .lines()
            .find(|line| line.contains("Section marker"))
            .unwrap()
            .starts_with('-'),
        "{output:?}"
    );
    for frame in crate::ui::SectionFrameStyle::ALL {
        for policy in crate::ui::RuledSectionPolicy::ALL {
            let mut messages = crate::ui::messages::MessageBuffer::default();
            messages.push(MessageLevel::Error, "message body");
            let mut request =
                crate::ui::messages::MessageRequest::new(&settings, MessageLevel::Info);
            request.settings.chrome_frame = frame;
            request.settings.ruled_section_policy = policy;
            let output = crate::ui::messages::render(&messages, &request);
            let borders: Vec<_> = output
                .lines()
                .filter(|line| {
                    let text = line.trim_start();
                    text.starts_with('-') || text.starts_with('+')
                })
                .collect();
            if frame != crate::ui::SectionFrameStyle::None {
                assert!(!borders.is_empty(), "{frame:?}: {output:?}");
            }
            for line in borders {
                assert_eq!(
                    line.len() - line.trim_start().len(),
                    0,
                    "{frame:?}: {output:?}"
                );
            }
        }
    }
}

#[test]
fn nested_blocks_use_one_indent() {
    let mut settings = settings(OutputFormat::Mreg);
    settings.format_explicit = true;
    let mut output = OutputResult::from_rows(vec![]).with_document(OutputDocument::Json(json!({
        "Name": "host.example.org",
        "LongParentLabel": "parent-value",
        "Nested": {"X": "child-value", "Inner": {"Y": "grandchild-value"}},
        "Records": [{"Address":"192.0.2.1", "TTL":300}, {"Address":"192.0.2.2", "TTL":300}],
        "Roles": ["first-long-role-name", "second-long-role-name", "third-long-role-name", "fourth-long-role-name"]
    })));
    let mut request = RenderRequest::new(&settings);
    request.target = RenderTarget::ReplScreen;
    output.meta.title_field = Some("Name".into());
    let output = render(&output, &request);
    let lines: Vec<_> = output.lines().collect();
    let indent = |line: &str| line.len() - line.trim_start().len();
    let title = lines
        .iter()
        .find(|line| line.contains("host.example.org"))
        .unwrap();
    let parent = lines
        .iter()
        .find(|line| line.contains("parent-value"))
        .unwrap();
    let child = lines
        .iter()
        .find(|line| line.contains("child-value"))
        .unwrap();
    let grandchild = lines
        .iter()
        .find(|line| line.contains("grandchild-value"))
        .unwrap();
    assert_eq!(indent(parent) - indent(title), 2, "{output}");
    assert_eq!(indent(child) - indent(parent), 2, "{output}");
    assert_eq!(indent(grandchild) - indent(child), 2, "{output}");
    assert_eq!(
        child.find("child-value").unwrap() - indent(child),
        3,
        "{output}"
    );
    assert_eq!(
        grandchild.find("grandchild-value").unwrap() - indent(grandchild),
        3,
        "{output}"
    );
    let table = lines
        .iter()
        .position(|line| line.trim_start().starts_with("Records"))
        .unwrap();
    let roles = lines
        .iter()
        .position(|line| line.trim_start().starts_with("Roles"))
        .unwrap();
    assert_eq!(
        indent(lines[table + 1]) - indent(lines[table]),
        2,
        "{output}"
    );
    assert_eq!(
        indent(lines[roles + 1]) - indent(lines[roles]),
        2,
        "{output}"
    );
}

#[test]
fn repl_margin_applies_once_and_exports_have_none() {
    let settings = settings(OutputFormat::Guide);
    let output = Document {
        blocks: vec![DocBlock::section(
            "Margin marker",
            vec![DocBlock::Paragraph {
                text: "Body marker".into(),
            }],
        )],
    }
    .to_output_result();
    for (target, indent) in [
        (RenderTarget::ReplScreen, 2),
        (RenderTarget::OneShotScreen, 0),
        (RenderTarget::Clipboard, 0),
    ] {
        let mut request = RenderRequest::new(&settings);
        request.target = target;
        let rendered = render(&output, &request);
        if target == RenderTarget::Clipboard {
            assert_eq!(
                rendered.trim_end(),
                "Margin marker:\nBody marker",
                "{rendered:?}"
            );
        }
        assert!(
            rendered
                .lines()
                .any(|line| line == format!("{}Body marker", " ".repeat(indent))),
            "{target:?}: {rendered:?}"
        );
    }
}

#[test]
fn colored_prose_wraps_without_losing_text() {
    let mut settings = settings(OutputFormat::Guide);
    settings.width = Some(32);
    settings.color = crate::core::ColorMode::Always;
    settings.mode = crate::core::RenderMode::Rich;
    settings.theme_name = "dracula".into();
    let text = "Every word in this deliberately long paragraph must survive wrapping across several short terminal lines.";
    let output = Document {
        blocks: vec![DocBlock::Paragraph { text: text.into() }],
    }
    .to_output_result();
    let rendered = render(&output, &RenderRequest::new(&settings));
    assert!(rendered.contains('\x1b'));
    for line in rendered.lines() {
        assert!(crate::ui::text::display_width(line) <= 32, "{line:?}");
    }
    let plain = crate::ui::sanitize_human_text(&rendered);
    assert_eq!(
        plain.split_whitespace().collect::<Vec<_>>(),
        text.split_whitespace().collect::<Vec<_>>()
    );
}

#[test]
fn scalar_value_lists_render_one_entry_per_line() {
    let output = OutputResult::from_rows(vec![
        crate::row! {"names" => json!(["first", "second", "third"])},
    ]);
    let settings = settings(OutputFormat::Value);
    let mut request = RenderRequest::new(&settings);
    request.target = RenderTarget::Clipboard;
    assert_eq!(render(&output, &request), "first\nsecond\nthird\n");
}

#[test]
fn fitting_footer_names_losses_and_recovery() {
    let mut settings = settings(OutputFormat::Table);
    settings.width = Some(50);
    settings.table_too_wide = crate::ui::TableTooWide::Hide;
    settings.table_overflow = crate::ui::TableOverflow::Ellipsis;
    let output = OutputResult::from_rows(vec![crate::row! {
        "id" => "record-17", "comment" => "A long comment with deliberately lengthy prose that cannot fit in this terminal",
        "details" => "A second long field with deliberately lengthy prose that cannot fit",
        "fourth" => "hidden-width-field", "fifth" => "another-hidden-field"
    }]);
    let rendered = render(&output, &RenderRequest::new(&settings));
    assert!(
        rendered.contains("hidden:") && rendered.contains("shortened in"),
        "{rendered}"
    );
    assert!(rendered.contains("comment"), "{rendered}");
    assert!(
        rendered.contains("| P") && rendered.contains("--mreg") && rendered.contains("| LESS"),
        "{rendered}"
    );
}
