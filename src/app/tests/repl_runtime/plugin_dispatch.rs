#[cfg_attr(miri, ignore = "plugin filesystem/process integration test")]
#[test]
fn repl_plugin_error_payload_is_handled_as_error_unit() {
    use std::os::unix::fs::PermissionsExt;

    let dir = make_temp_dir("osp-cli-repl-error-plugin");
    let plugin_path = dir.join("osp-fail");
    std::fs::write(
        &plugin_path,
        r#"#!/bin/sh
PATH=/usr/bin:/bin:$PATH
if [ "$1" = "--osp-describe" ]; then
  cat <<'JSON'
{"about":"fail","subcommands":[],"args":[],"flags":{},"output":"json"}
JSON
  exit 0
fi
cat <<'JSON'
{"protocol_version":1,"ok":false,"data":{},"error":{"code":"MOCK_ERR","message":"mock failure","details":{}},"meta":{}}
JSON
"#,
    )
    .expect("plugin script should be written");
    let mut perms = std::fs::metadata(&plugin_path)
        .expect("metadata should be readable")
        .permissions();
    perms.set_mode(0o755);
    std::fs::set_permissions(&plugin_path, perms).expect("script should be executable");

    let mut state = make_test_state(vec![dir.to_path_buf()]);
    state.runtime.ui.render_settings.format = OutputFormat::Table;

    let history = make_test_history(&mut state);
    let mut sink = BufferedUiSink::default();
    repl_dispatch::execute_repl_plugin_line(
        &mut state,
        &history,
        "fail", Some(&mut sink),)
    .expect("failure should render through the sink");
    assert!(sink.stderr.contains("Plugin fail failed: mock failure."));

}

#[cfg_attr(miri, ignore = "plugin filesystem/process integration test")]
#[test]
fn plugin_pipeline_rendering_matches_between_cli_and_repl_unit() {
    let dir = make_temp_dir("osp-cli-plugin-pipeline-parity");
    let _plugin_path = write_pipeline_test_plugin(&dir);
    let mut state = make_test_state(vec![dir.to_path_buf()]);
    let history = make_test_history(&mut state);
    let stages = vec!["message".to_string()];

    let invocation = crate::app::resolve_invocation_ui(state.runtime.config.resolved(), &state.runtime.ui, &Default::default());
    let result = crate::app::external::run_external_command_with_help_renderer(
        &mut state, &["hello".into()], &invocation, Document::from_help_text, None,
    ).and_then(|result| crate::app::command_output::apply_stages_to_cli_result(result, &stages)).expect("CLI plugin output should prepare");
    assert_eq!(result.exit_code, 0);
    assert!(result.failure_report.is_none());
    let crate::app::CommandBody::Output(prepared) = result.body else {
        panic!("expected structured plugin output");
    };
    let cli_rendered = render(&prepared, &crate::ui::RenderRequest::new(&super::super::resolve_render_settings_with_hint(
            &state.runtime.ui.render_settings,
            result.format_hint,
        )));

    let repl_rendered = repl_dispatch::execute_repl_plugin_line(
        &mut state,
        &history,
        "hello | message", None,).map(|executed| executed.result)
    .expect("repl command should succeed");
    match repl_rendered {
        crate::repl::ReplResult::Print(text) => {
            assert_eq!(text.trim(), cli_rendered.trim());
            assert!(text.contains("hello-from-plugin"));
        }
        other => panic!("unexpected repl result: {other:?}"),
    }

}
