#[test]
fn rebuild_repl_state_preserves_session_defaults_and_shell_context_unit() {
    if crate::tests::isolated_test() { return; }
    with_test_xdg_env(|| {
        let (mut state, _state_root) = make_test_state(Vec::new());
        state.session.history_enabled = false;
        state
            .session
            .config_overrides
            .set("user.name", "launch-user");
        state
            .session
            .config_overrides
            .set("ui.verbosity", "trace");
        state.session.config_overrides.set("debug.level", 2i64);
        state.session.config_overrides.set("ui.format", "json");
        state.session.config_overrides.set("theme.name", "dracula");
        state.session.scope.enter("orch");

        state.session.history_shell = HistoryShellContext::default();
        state.sync_history_shell_context();

        let next = super::super::rebuild_repl_state(&state).expect("rebuild should succeed");

        assert_eq!(
            next.runtime.config.resolved().get_string("user.name"),
            Some("launch-user")
        );
        assert_eq!(next.runtime.ui.verbosity, MessageLevel::Trace);
        assert_eq!(next.runtime.ui.debug_level, 2);
        assert_eq!(next.runtime.ui.render_settings.format, OutputFormat::Json);
        assert_eq!(next.runtime.ui.render_settings.theme_name, "dracula");
        assert!(!next.session.history_enabled);
        assert_eq!(next.session.scope.commands(), vec!["orch".to_string()]);
        assert_eq!(
            next.session.history_shell.prefix(),
            Some("orch ".to_string())
        );
    });
}

#[test]
fn rebuild_repl_state_preserves_rich_terminal_render_runtime_unit() {
    if crate::tests::isolated_test() { return; }
    with_test_xdg_env(|| {
        let (mut state, _state_root) = make_test_state(Vec::new());
        state.session.config_overrides.set("theme.name", "dracula");
        state.runtime.ui.render_settings.mode = crate::core::output::RenderMode::Auto;
        state.runtime.ui.render_settings.color = crate::core::output::ColorMode::Auto;
        state.runtime.ui.render_settings.unicode = crate::core::output::UnicodeMode::Auto;
        state.runtime.ui.render_settings.runtime.stdout_is_tty = true;
        state.runtime.ui.render_settings.runtime.locale_utf8 = Some(true);
        state.runtime.ui.render_settings.runtime.terminal = Some("xterm-256color".to_string());

        let next = super::super::rebuild_repl_state(&state).expect("rebuild should succeed");
        let resolved = next.runtime.ui.render_settings.resolve_render_settings();

        assert_eq!(next.runtime.ui.render_settings.theme_name, "dracula");
        assert!(resolved.color);
        assert!(resolved.unicode);
        assert_eq!(resolved.backend, crate::ui::RenderBackend::Rich);
        assert_eq!(resolved.theme.id, "dracula");
        assert_eq!(resolved.theme.repl_completion_text_spec(), "#000000");
    });
}

#[test]
fn rebuild_repl_state_preserves_session_render_defaults_unit() {
    if crate::tests::isolated_test() { return; }
    with_test_xdg_env(|| {
        let (mut state, _state_root) = make_test_state(Vec::new());
        state.session.config_overrides.set("ui.format", "table");

        let next = super::super::rebuild_repl_state(&state).expect("rebuild should succeed");

        assert_eq!(next.runtime.ui.render_settings.format, OutputFormat::Table);
    });
}
#[cfg_attr(miri, ignore = "plugin restart filesystem/process integration test")]
#[test]
fn repl_plugin_enable_restart_refreshes_command_catalog_unit() {
    if crate::tests::isolated_test() { return; }
    with_temp_config_paths(|| {
        let dir = make_temp_dir("osp-cli-repl-plugin-enable");
        let _plugin = write_pipeline_test_plugin(&dir);
        let (mut state, _state_root) = make_test_state(vec![dir.to_path_buf()]);
        state
            .clients
            .plugins()
            .set_disabled(["hello".to_string()]);
        assert!(state.clients.plugins().command_catalog().is_empty());

        let (history, _history_root) = make_test_history(&mut state);
        let result = repl_dispatch::execute_repl_plugin_line(
            &mut state,
            &history,
            "plugins enable hello", None,).map(|executed| executed.result)
        .expect("plugin enable should succeed");
        assert!(matches!(
            result,
            crate::repl::ReplResult::Restart {
                reload: crate::repl::ReplReloadKind::Default,
                ..
            }
        ));

        let next = super::super::rebuild_repl_state(&state).expect("rebuild should succeed");
        let catalog = next.clients.plugins().command_catalog();
        assert!(
            catalog.iter().any(|entry| entry.name == "hello"),
            "enabled plugin should appear after rebuild"
        );
    });
}

#[test]
fn repl_reload_intent_matches_command_scope_unit() {
    let (mut state, _state_root) = make_test_state(Vec::new());
    state.runtime.themes =
        crate::ui::theme_catalog::load_theme_catalog(state.runtime.config.resolved());
    let (history, _history_root) = make_test_history(&mut state);

    let theme_result = repl_dispatch::execute_repl_plugin_line(
        &mut state,
        &history,
        "theme use dracula", None,).map(|executed| executed.result)
    .expect("theme use should succeed");
    assert!(matches!(
        theme_result,
        crate::repl::ReplResult::Restart {
            reload: crate::repl::ReplReloadKind::WithIntro,
            ..
        }
    ));

    let format_result = repl_dispatch::execute_repl_plugin_line(
        &mut state,
        &history,
        "config set ui.format json", None,).map(|executed| executed.result)
    .expect("config set should succeed");
    assert!(matches!(
        format_result,
        crate::repl::ReplResult::Restart {
            reload: crate::repl::ReplReloadKind::Default,
            ..
        }
    ));

    let unset_format_result = repl_dispatch::execute_repl_plugin_line(
        &mut state,
        &history,
        "config unset ui.format", None,).map(|executed| executed.result)
    .expect("config unset should succeed");
    assert!(matches!(
        unset_format_result,
        crate::repl::ReplResult::Restart {
            reload: crate::repl::ReplReloadKind::Default,
            ..
        }
    ));

    let dry_run_unset_result = repl_dispatch::execute_repl_plugin_line(
        &mut state,
        &history,
        "config unset ui.format --dry-run", None,).map(|executed| executed.result)
    .expect("dry-run config unset should succeed");
    assert!(matches!(
        dry_run_unset_result,
        crate::repl::ReplResult::Print(_)
    ));
}

#[test]
fn repl_config_unset_rebuilds_runtime_state_unit() {
    if crate::tests::isolated_test() { return; }
    with_test_xdg_env(|| {
        let (mut state, _state_root) = make_test_state(Vec::new());
        state
            .session
            .config_overrides
            .insert("ui.format", "table", crate::config::Scope::profile("default"));
        state = super::super::rebuild_repl_state(&state).expect("rebuild should succeed");
        assert_eq!(state.runtime.ui.render_settings.format, OutputFormat::Table);

        let (history, _history_root) = make_test_history(&mut state);
        let result = repl_dispatch::execute_repl_plugin_line(
            &mut state,
            &history,
            "config unset ui.format", None,).map(|executed| executed.result)
        .expect("config unset should succeed");
        assert!(matches!(
            result,
            crate::repl::ReplResult::Restart {
                reload: crate::repl::ReplReloadKind::Default,
                ..
            }
        ));
        assert_eq!(
            layer_value(&state.session.config_overrides, "ui.format"),
            None
        );

        let next = super::super::rebuild_repl_state(&state).expect("rebuild should succeed");
        assert_eq!(next.runtime.config.resolved().get_string("ui.format"), None);
        assert_eq!(next.runtime.ui.render_settings.format, OutputFormat::Auto);
    });
}

#[test]
fn repl_config_unset_dry_run_preserves_session_state_unit() {
    if crate::tests::isolated_test() { return; }
    with_test_xdg_env(|| {
        let (mut state, _state_root) = make_test_state(Vec::new());
        state
            .session
            .config_overrides
            .insert("ui.format", "table", crate::config::Scope::profile("default"));
        state = super::super::rebuild_repl_state(&state).expect("rebuild should succeed");
        assert_eq!(state.runtime.ui.render_settings.format, OutputFormat::Table);

        let (history, _history_root) = make_test_history(&mut state);
        let result = repl_dispatch::execute_repl_plugin_line(
            &mut state,
            &history,
            "config unset ui.format --dry-run", None,).map(|executed| executed.result)
        .expect("dry-run config unset should succeed");
        assert!(matches!(result, crate::repl::ReplResult::Print(_)));
        assert_eq!(
            layer_value(&state.session.config_overrides, "ui.format"),
            Some(&ConfigValue::from("table"))
        );
        assert_eq!(state.runtime.ui.render_settings.format, OutputFormat::Table);
    });
}
