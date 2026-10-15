use super::*;

#[test]
fn intro_command_dispatches_as_repl_scoped_builtin_unit() {
    let mut cli = Cli::try_parse_from(["osp", "intro"]).expect("cli parse");
    let plan = build_dispatch_plan(&mut cli).expect("dispatch plan");

    assert!(matches!(
        plan.action,
        RunAction::Builtin(Commands::Intro(_))
    ));
    assert_eq!(plan.action.terminal_kind(), TerminalKind::Repl);
}

#[test]
fn staged_semantic_quick_search_preserves_guide_shape_by_default_unit() {
    let guide = Document::from_help_text(
        "Usage: osp [COMMAND]\n\nCommands:\n  help  Show this command overview.\n  theme  Inspect and apply themes\n  config  Inspect and edit runtime config\n",
    );

    let (output, format_hint) =
        apply_output_stages(guide.to_output_result(), &["inspect".to_string()], None)
            .expect("staged guide output should succeed");

    assert!(format_hint.is_none());
    assert!(output.meta.document_rows);
    assert!(output.document.is_none());
    assert_eq!(output.as_rows().unwrap().len(), 2);
    assert!(
        output
            .as_rows()
            .unwrap()
            .iter()
            .all(|row| row["section"] == "Commands")
    );

    let rendered = render_output(&output, &RenderSettings::test_plain(OutputFormat::Auto));
    assert!(rendered.contains("Commands"));
    assert!(!rendered.contains("Usage"));
    assert!(!rendered.contains("Sections"));
    assert!(!rendered.contains("Entries"));
}
