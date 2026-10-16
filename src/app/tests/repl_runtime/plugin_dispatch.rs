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
if [ "$1" = "--describe" ]; then
  cat <<'JSON'
{"protocol_version":1,"plugin_id":"fail","plugin_version":"0.1.0","min_osp_version":"0.1.0","commands":[{"name":"fail","about":"fail","subcommands":[],"args":[],"flags":{}}]}
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
        &mut state.runtime,
        &mut state.session,
        &state.clients,
        &history,
        "fail", Some(&mut sink),)
    .expect("failure should render through the sink");
    assert!(sink.stderr.contains("MOCK_ERR: mock failure"));

}

#[cfg_attr(miri, ignore = "plugin filesystem/process integration test")]
#[test]
fn plugin_pipeline_rendering_matches_between_cli_and_repl_unit() {
    let dir = make_temp_dir("osp-cli-plugin-pipeline-parity");
    let _plugin_path = write_pipeline_test_plugin(&dir);
    let mut state = make_test_state(vec![dir.to_path_buf()]);
    let history = make_test_history(&mut state);
    let stages = vec!["message".to_string()];

    let dispatch_context =
        super::super::plugin_dispatch_context_for_runtime(&state.runtime, &state.clients, None);
    let response = state
        .clients
        .plugins()
        .dispatch("hello", &[], &dispatch_context)
        .expect("plugin dispatch should succeed");
    let result = super::super::cli_result_from_plugin_response(response, &stages)
        .and_then(|result| super::super::command_output::apply_stages_to_cli_result(result, &stages))
        .expect("plugin response should prepare");
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
        &mut state.runtime,
        &mut state.session,
        &state.clients,
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

#[cfg_attr(miri, ignore = "plugin filesystem/process integration test")]
#[test]
fn repl_plugin_provider_override_dispatches_selected_provider_unit() {
    let dir = make_temp_dir("osp-cli-repl-provider-override");
    let _alpha = write_provider_test_plugin(&dir, "alpha-provider", "hello", "alpha");
    let _beta = write_provider_test_plugin(&dir, "beta-provider", "hello", "beta");
    let mut state = make_test_state(vec![dir.to_path_buf()]);
    state.runtime.ui.render_settings.format = OutputFormat::Table;
    let history = make_test_history(&mut state);

    let mut sink = BufferedUiSink::default();
    repl_dispatch::execute_repl_plugin_line(
        &mut state.runtime,
        &mut state.session,
        &state.clients,
        &history,
        "hello", Some(&mut sink),)
    .expect("ambiguity should render through the sink");
    let message = &sink.stderr;
    assert!(
        message.contains("provided by multiple plugins")
            || message.contains("requires provider selection"),
        "unexpected ambiguity message: {message}",
    );

    let repl_rendered = repl_dispatch::execute_repl_plugin_line(
        &mut state.runtime,
        &mut state.session,
        &state.clients,
        &history,
        "--plugin-provider beta-provider hello", None,).map(|executed| executed.result)
    .expect("repl command should honor one-shot provider override");
    match repl_rendered {
        crate::repl::ReplResult::Print(text) => {
            assert!(text.contains("beta-from-plugin"));
            assert!(!text.contains("alpha-from-plugin"));
        }
        other => panic!("unexpected repl result: {other:?}"),
    }

}
