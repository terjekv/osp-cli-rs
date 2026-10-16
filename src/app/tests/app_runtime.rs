use super::*;
use std::sync::{Arc, Mutex};

fn env_lock() -> &'static Mutex<()> {
    crate::tests::env_lock()
}

fn with_test_xdg_env<T>(run: impl FnOnce() -> T) -> T {
    let _guard = env_lock().lock().expect("env lock should not be poisoned");
    let original_home = std::env::var("HOME").ok();
    let original_xdg_config_home = std::env::var("XDG_CONFIG_HOME").ok();
    let original_xdg_cache_home = std::env::var("XDG_CACHE_HOME").ok();
    let original_xdg_data_home = std::env::var("XDG_DATA_HOME").ok();

    unsafe {
        std::env::set_var("HOME", "/tmp/osp-app-runtime-home");
        std::env::set_var("XDG_CONFIG_HOME", "/tmp/osp-app-runtime-xdg/config");
        std::env::set_var("XDG_CACHE_HOME", "/tmp/osp-app-runtime-xdg/cache");
        std::env::set_var("XDG_DATA_HOME", "/tmp/osp-app-runtime-xdg/data");
    }

    let output = run();

    match original_home {
        Some(value) => unsafe { std::env::set_var("HOME", value) },
        None => unsafe { std::env::remove_var("HOME") },
    }
    match original_xdg_config_home {
        Some(value) => unsafe { std::env::set_var("XDG_CONFIG_HOME", value) },
        None => unsafe { std::env::remove_var("XDG_CONFIG_HOME") },
    }
    match original_xdg_cache_home {
        Some(value) => unsafe { std::env::set_var("XDG_CACHE_HOME", value) },
        None => unsafe { std::env::remove_var("XDG_CACHE_HOME") },
    }
    match original_xdg_data_home {
        Some(value) => unsafe { std::env::set_var("XDG_DATA_HOME", value) },
        None => unsafe { std::env::remove_var("XDG_DATA_HOME") },
    }

    output
}

struct CompletionRefreshingNativeCommand;

struct CapabilityGatedNativeCommand;

struct TypoNativeCommand;

impl crate::NativeCommand for TypoNativeCommand {
    fn command(&self) -> clap::Command {
        clap::Command::new("ldap")
            .subcommand(clap::Command::new("user").arg(clap::Arg::new("USERNAME").required(true)))
    }

    fn execute(
        &self,
        args: &[String],
        _context: &crate::NativeCommandContext<'_>,
    ) -> anyhow::Result<crate::NativeCommandOutcome> {
        self.command()
            .try_get_matches_from(std::iter::once("ldap").chain(args.iter().map(String::as_str)))
            .map_err(anyhow::Error::new)?;
        Ok(crate::NativeCommandOutcome::Exit(0))
    }
}

impl crate::NativeCommand for CapabilityGatedNativeCommand {
    fn command(&self) -> clap::Command {
        clap::Command::new("ldap").subcommand(clap::Command::new("user"))
    }

    fn describe(&self) -> DescribeCommandV1 {
        let mut command = DescribeCommandV1::from_clap(self.command());
        command.subcommands[0].auth = Some(DescribeCommandAuthV1 {
            visibility: Some(DescribeVisibilityModeV1::Authenticated),
            required_capabilities: vec!["ldap.user.read".to_string()],
            ..DescribeCommandAuthV1::default()
        });
        command
    }

    fn execute(
        &self,
        _args: &[String],
        _context: &crate::NativeCommandContext<'_>,
    ) -> anyhow::Result<crate::NativeCommandOutcome> {
        unreachable!("capability gate should deny execution")
    }
}

impl crate::NativeCommand for CompletionRefreshingNativeCommand {
    fn command(&self) -> clap::Command {
        clap::Command::new("refreshable").subcommand(clap::Command::new("refresh"))
    }

    fn execute(
        &self,
        _args: &[String],
        context: &crate::NativeCommandContext<'_>,
    ) -> anyhow::Result<crate::NativeCommandOutcome> {
        context.session_context.request_completion_refresh();
        Ok(crate::NativeCommandOutcome::Help("refreshed".to_string()))
    }
}

#[derive(Clone, Default)]
struct StartupResolutionProbe(Arc<Mutex<Vec<(String, crate::app::TerminalKind)>>>);

impl crate::app::AccessPolicy for StartupResolutionProbe {
    fn startup(&self, runtime: &mut crate::app::AppRuntime) -> miette::Result<()> {
        self.0
            .lock()
            .expect("startup probe should not be poisoned")
            .push((
                runtime
                    .config_state()
                    .resolved()
                    .active_profile()
                    .to_string(),
                runtime.context().terminal_kind(),
            ));
        Ok(())
    }
}

#[cfg_attr(miri, ignore = "public app bootstrap integration test")]
#[test]
fn app_help_entrypoints_share_exit_and_sink_routing_invariants_unit() {
    with_test_xdg_env(|| {
        let app = crate::app::App::new();
        let mut help_sink = BufferedUiSink::default();

        assert_eq!(
            app.run_with_sink(["osp", "--defaults-only", "--help"], &mut help_sink)
                .expect("app help with sink should render"),
            0
        );
        assert_eq!(
            app.run_process_with_sink(["osp", "--defaults-only", "--help"], &mut help_sink),
            0
        );
        assert!(!help_sink.stdout.is_empty());
        assert!(help_sink.stderr.is_empty());

        let mut sink = BufferedUiSink::default();
        let exit = super::run_from_with_sink(["osp", "--defaults-only", "--help"], &mut sink)
            .expect("help should render");
        assert_eq!(exit, 0);
        assert!(!sink.stdout.is_empty());
        assert!(sink.stderr.is_empty());

        let mut runner_sink = BufferedUiSink::default();
        let runner = crate::app::App::new();
        assert_eq!(
            runner
                .run_with_sink(["osp", "--defaults-only", "--help"], &mut runner_sink)
                .expect("runner help should render"),
            0
        );
    });
}

#[cfg_attr(miri, ignore = "public app bootstrap integration test")]
#[test]
fn app_public_entrypoints_cover_repeated_sink_and_free_process_wrappers_unit() {
    with_test_xdg_env(|| {
        let app = crate::app::App::new();
        assert_eq!(
            app.run_from(["osp", "--defaults-only", "--help"])
                .expect("app run_from help should render"),
            0
        );
        assert_eq!(app.run_process(["osp", "--defaults-only", "--help"]), 0);

        let mut runner_sink = BufferedUiSink::default();
        let runner = crate::app::App::new();
        assert_eq!(
            runner.run_process_with_sink(["osp", "--defaults-only", "--help"], &mut runner_sink),
            0
        );
        assert!(!runner_sink.stdout.is_empty());
        assert!(runner_sink.stderr.is_empty());

        assert_eq!(
            crate::app::run_process(["osp", "--defaults-only", "--help"]),
            0
        );

        let mut sink = BufferedUiSink::default();
        assert_eq!(
            crate::app::run_process_with_sink(["osp", "--defaults-only", "--help"], &mut sink),
            0
        );
        assert!(!sink.stdout.is_empty());
        assert!(sink.stderr.is_empty());
    });
}

#[cfg_attr(miri, ignore = "public app bootstrap integration test")]
#[test]
fn startup_hook_receives_parent_owned_profile_and_terminal_resolution_unit() {
    with_test_xdg_env(|| {
        let mut product_defaults = crate::config::ConfigLayer::default();
        product_defaults.set("profile.default", "default");
        product_defaults.insert(
            "ui.format",
            "table",
            crate::config::Scope::profile("production"),
        );
        product_defaults.set("theme.path", Vec::<String>::new());

        let probe = StartupResolutionProbe::default();
        let run = |args: &[&str]| {
            let mut sink = BufferedUiSink::default();
            let exit = crate::app::App::new()
                .with_product_defaults(product_defaults.clone())
                .with_access_policy(probe.clone())
                .run_with_sink(args, &mut sink)
                .expect("prepared command should run");
            assert_eq!(exit, 0, "startup probe command should succeed");
        };

        run(&["osp", "--profile=production", "intro"]);
        run(&["osp", "--profile", "production", "intro"]);
        run(&[
            "osp",
            "--profile",
            "production",
            "config",
            "get",
            "profile.active",
        ]);

        assert_eq!(
            *probe
                .0
                .lock()
                .expect("startup probe should not be poisoned"),
            vec![
                ("production".to_string(), crate::app::TerminalKind::Repl),
                ("production".to_string(), crate::app::TerminalKind::Repl),
                ("production".to_string(), crate::app::TerminalKind::Cli),
            ]
        );
    });
}

#[cfg_attr(miri, ignore = "public app startup context integration test")]
#[test]
fn startup_hook_can_publish_authenticated_subject_without_re_resolving_config_unit() {
    with_test_xdg_env(|| {
        #[derive(Clone)]
        struct SubjectHook(Arc<Mutex<Option<String>>>);

        impl crate::app::AccessPolicy for SubjectHook {
            fn startup(&self, runtime: &mut crate::app::AppRuntime) -> miette::Result<()> {
                runtime.set_authenticated_subject(Some("alice"));
                *self.0.lock().expect("subject probe should not be poisoned") = runtime
                    .context()
                    .authenticated_subject()
                    .map(ToOwned::to_owned);
                Ok(())
            }
        }

        let subject = Arc::new(Mutex::new(None));
        let mut sink = BufferedUiSink::default();
        let exit = crate::app::App::new()
            .with_access_policy(SubjectHook(subject.clone()))
            .run_with_sink(["osp", "config", "get", "profile.active"], &mut sink)
            .expect("startup subject command should run");

        assert_eq!(exit, 0);
        assert_eq!(
            subject
                .lock()
                .expect("subject probe should not be poisoned")
                .as_deref(),
            Some("alice")
        );
    });
}

#[cfg_attr(miri, ignore = "public app bootstrap integration test")]
#[test]
fn app_product_defaults_flow_through_public_setup_surface_unit() {
    with_test_xdg_env(|| {
        let mut product_defaults = crate::config::ConfigLayer::default();
        product_defaults.set("extensions.site.enabled", true);
        product_defaults.set("theme.path", Vec::<String>::new());

        let mut sink = BufferedUiSink::default();
        let exit = crate::app::App::new()
            .with_product_defaults(product_defaults)
            .run_process_with_sink(
                [
                    "osp",
                    "--defaults-only",
                    "--json",
                    "config",
                    "get",
                    "extensions.site.enabled",
                ],
                &mut sink,
            );

        assert_eq!(exit, 0);
        let payload: serde_json::Value =
            serde_json::from_str(&sink.stdout).expect("config get should render JSON");
        let rows = payload
            .as_array()
            .expect("config get JSON should be an array");
        assert!(rows.iter().any(|row| {
            row["key"] == "extensions.site.enabled" && row["value"] == serde_json::json!(true)
        }));
        assert!(sink.stderr.is_empty());
    });
}

#[cfg_attr(miri, ignore = "public app bootstrap integration test")]
#[test]
fn free_process_wrapper_renders_usage_errors_to_bound_sink_unit() {
    with_test_xdg_env(|| {
        let mut sink = BufferedUiSink::default();
        let exit = crate::app::run_process_with_sink(["osp", "--definitely-not-a-flag"], &mut sink);

        assert_eq!(exit, EXIT_CODE_USAGE);
        assert!(sink.stdout.is_empty());
        assert!(
            sink.stderr.contains("definitely-not-a-flag")
                || sink.stderr.contains("unexpected argument")
        );
    });
}

#[test]
fn native_commands_project_into_auth_catalog_unit() {
    let state = make_completion_state_with_entries_and_native(&[], test_native_registry());
    let catalog = super::authorized_command_catalog_for(
        &state.runtime.auth,
        &state.clients,
        state.clients.native_commands().catalog(),
    );
    assert!(catalog.iter().any(|entry| entry.name == "ldap"));
    assert!(
        state
            .runtime
            .auth
            .external_command_access("ldap")
            .is_visible()
    );
}

#[cfg(unix)]
#[test]
fn native_command_can_restart_repl_after_refreshing_completion_unit() {
    let mut state = make_completion_state_with_entries_and_native(
        &[],
        NativeCommandRegistry::new().with_command(CompletionRefreshingNativeCommand),
    );
    let history = make_test_history(&mut state);

    let result =
        repl_dispatch::execute_repl_plugin_line(&mut state, &history, "refreshable refresh", None)
            .map(|executed| executed.result)
            .expect("native refresh command should run");

    assert!(matches!(
        result,
        crate::repl::ReplResult::Restart {
            output,
            reload: crate::repl::ReplReloadKind::Default,
        } if output.contains("refreshed")
    ));
}

#[cfg(unix)]
#[test]
fn authentication_denial_and_doctor_last_add_progressive_detail_unit() {
    let mut state = make_completion_state_with_entries_and_native(
        &[],
        NativeCommandRegistry::new().with_command(CapabilityGatedNativeCommand),
    );
    state.runtime.ui.render_settings.format = OutputFormat::Table;
    let history = make_test_history(&mut state);

    let mut sink = BufferedUiSink::default();
    repl_dispatch::execute_repl_plugin_line(&mut state, &history, "ldap user", Some(&mut sink))
        .expect("failure should render through the sink");
    let immediate = sink.stderr.split_whitespace().collect::<Vec<_>>().join(" ");
    assert!(immediate.contains("command ldap user"));
    assert!(immediate.contains("requires authentication"));
    assert!(immediate.contains("hint: authenticate"));

    let mut doctor = |line: &str| match repl_dispatch::execute_repl_plugin_line(
        &mut state, &history, line, None,
    )
    .map(|executed| executed.result)
    .expect("doctor last should render")
    {
        crate::repl::ReplResult::Print(output) => output,
        other => panic!("unexpected doctor result: {other:?}"),
    };

    let recap = doctor("doctor last");
    assert!(recap.contains("requires authentication"));
    assert!(
        recap.contains("Action:  authenticate"),
        "unexpected recap: {recap}"
    );
    assert!(!recap.contains("Detail:"));
    assert!(!recap.contains("doctor last"));

    let verbose = doctor("-v doctor last");
    assert!(verbose.contains("requires authentication"));
    assert!(!verbose.contains("osp::auth::missing_capabilities"));

    let debug = doctor("-vv doctor last");
    assert!(debug.contains("requires authentication"));
    assert!(!debug.contains("Stack backtrace"));

    let forensic = doctor("-vvv doctor last");
    assert!(forensic.contains("requires authentication"));
    assert!(!forensic.contains("run `doctor last"));
}

#[cfg(unix)]
#[test]
fn native_subcommand_typo_and_doctor_last_add_progressive_detail_unit() {
    let mut state = make_completion_state_with_entries_and_native(
        &[],
        NativeCommandRegistry::new().with_command(TypoNativeCommand),
    );
    state.runtime.ui.render_settings.format = OutputFormat::Table;
    let history = make_test_history(&mut state);

    let mut sink = BufferedUiSink::default();
    repl_dispatch::execute_repl_plugin_line(&mut state, &history, "ldap usr", Some(&mut sink))
        .expect("failure should render through the sink");
    let immediate = &sink.stderr;
    assert!(immediate.contains("Unknown command 'ldap usr'"));
    assert!(
        immediate.contains("hint: Did you mean ldap user?"),
        "unexpected immediate error: {immediate}"
    );
    assert!(!immediate.contains("Usage:"));
    assert!(!immediate.contains("native command execution failed"));

    let mut doctor = |line: &str| match repl_dispatch::execute_repl_plugin_line(
        &mut state, &history, line, None,
    )
    .map(|executed| executed.result)
    .expect("doctor last should render")
    {
        crate::repl::ReplResult::Print(output) => output,
        other => panic!("unexpected doctor result: {other:?}"),
    };

    let recap = doctor("doctor last");
    assert!(recap.contains("Action:  Did you mean ldap user?"));
    assert!(!recap.contains("Usage:"));

    let verbose = doctor("-v doctor last");
    assert!(!verbose.contains("Usage: ldap"));
    assert!(!verbose.contains("native command execution failed"));

    let debug = doctor("-vv doctor last");
    assert!(debug.contains("Unknown command 'ldap usr'"));
    assert!(!debug.contains("Stack backtrace"));

    let forensic = doctor("-vvv doctor last");
    assert!(!forensic.contains("Stack backtrace"));
}

#[cfg(unix)]
#[test]
fn native_missing_argument_names_input_and_scoped_help_unit() {
    let mut state = make_completion_state_with_entries_and_native(
        &[],
        NativeCommandRegistry::new().with_command(TypoNativeCommand),
    );
    state.runtime.ui.render_settings.format = OutputFormat::Table;
    let history = make_test_history(&mut state);

    let mut sink = BufferedUiSink::default();
    repl_dispatch::execute_repl_plugin_line(&mut state, &history, "ldap user", Some(&mut sink))
        .expect("failure should render through the sink");
    let immediate = &sink.stderr;
    assert!(immediate.contains("<USERNAME>"), "{immediate}");
    assert!(
        immediate.contains("hint: Run ldap user --help."),
        "{immediate}"
    );
    assert!(!immediate.contains("Usage:"));
}

#[test]
fn bootstrap_verbosity_handles_non_utf8_short_flags_and_double_dash_unit() {
    let mut args = vec![
        OsString::from("osp"),
        OsString::from("--verbose"),
        OsString::from("-qv"),
    ];
    #[cfg(unix)]
    {
        use std::os::unix::ffi::OsStringExt;
        args.push(OsString::from_vec(vec![0xFF]));
    }
    args.extend([
        OsString::from("--"),
        OsString::from("-vvv"),
        OsString::from("--quiet"),
    ]);

    let counts = crate::cli::invocation::scan_cli_argv(&args).invocation;
    let (verbose, quiet, debug) = (counts.verbose, counts.quiet, counts.debug);
    assert_eq!((verbose, quiet, debug), (2, 1, 0));
    assert_eq!(
        crate::ui::messages::adjust_verbosity(MessageLevel::Success, verbose, quiet),
        MessageLevel::Info
    );
}

#[test]
fn bootstrap_error_detail_handles_non_utf8_short_flags_and_double_dash_unit() {
    let mut args = vec![
        OsString::from("osp"),
        OsString::from("--verbose"),
        OsString::from("-qv"),
    ];
    #[cfg(unix)]
    {
        use std::os::unix::ffi::OsStringExt;
        args.push(OsString::from_vec(vec![0xFF]));
    }
    args.extend([
        OsString::from("--"),
        OsString::from("-vvv"),
        OsString::from("--quiet"),
    ]);

    let counts = crate::cli::invocation::scan_cli_argv(&args).invocation;
    let (verbose, quiet, debug) = (counts.verbose, counts.quiet, counts.debug);
    assert_eq!((verbose, quiet, debug), (2, 1, 0));
    assert_eq!(
        crate::ui::messages::adjust_verbosity(MessageLevel::Success, verbose, quiet),
        MessageLevel::Info
    );
}

#[cfg_attr(
    miri,
    ignore = "forensic backtrace rendering uses std backtrace display"
)]
#[test]
fn run_cli_command_routes_messages_stdout_and_stderr_through_sink_unit() {
    let config = test_config(&[]);
    let ui = crate::app::UiState::new(
        RenderSettings::test_plain(OutputFormat::Value),
        MessageLevel::Success,
        0,
    );

    let mut sink = BufferedUiSink::default();
    let mut messages = MessageBuffer::default();
    messages.success("done");

    let exit = super::run_cli_command(
        &config,
        &ui,
        &super::CommandOutput {
            exit_code: 7,
            messages,
            stderr_text: "warn\n".to_string(),
            failure_report: None,
            ..super::CommandOutput::text("payload\n".to_string())
        },
        &mut sink,
    )
    .expect("command output should render");

    assert_eq!(exit, 7);
    assert_eq!(sink.stdout, "payload\n");
    assert!(sink.stderr.contains("done"));
    assert!(sink.stderr.contains("warn"));
}

#[test]
fn run_cli_command_builds_runtime_from_config_and_ui_unit() {
    let config = test_config(&[]);
    let ui = crate::app::UiState::new(
        RenderSettings::test_plain(OutputFormat::Value),
        MessageLevel::Success,
        0,
    );
    let mut sink = BufferedUiSink::default();

    let exit = super::run_cli_command(
        &config,
        &ui,
        &super::CommandOutput {
            exit_code: 3,
            messages: MessageBuffer::default(),
            stderr_text: String::new(),
            failure_report: None,
            ..super::CommandOutput::text("payload\n".to_string())
        },
        &mut sink,
    )
    .expect("command output should render");

    assert_eq!(exit, 3);
    assert_eq!(sink.stdout, "payload\n");
    assert!(sink.stderr.is_empty());
}

#[cfg_attr(miri, ignore = "public app bootstrap integration test")]
#[test]
fn state_and_client_builders_produce_coherent_embedder_state_unit() {
    let config = test_config(&[]);
    let ui = crate::app::UiState::new(
        RenderSettings::test_plain(OutputFormat::Json),
        MessageLevel::Trace,
        2,
    );
    let launch = crate::app::LaunchContext::default()
        .with_plugin_dir("/tmp/osp-plugin-a")
        .with_config_root(Some(std::path::PathBuf::from("/tmp/osp-config")))
        .with_cache_root(Some(std::path::PathBuf::from("/tmp/osp-cache")));
    let session = crate::app::AppSession::default().with_prompt_prefix("osp-dev");
    let plugins =
        crate::plugin::PluginManager::new(vec![std::path::PathBuf::from("/tmp/osp-plugin-a")])
            .with_roots(
                Some(std::path::PathBuf::from("/tmp/osp-config")),
                Some(std::path::PathBuf::from("/tmp/osp-cache")),
            )
            .with_bundled_roots(false)
            .with_default_roots(false);

    let state = crate::app::AppStateBuilder::new(
        crate::app::RuntimeContext::new(None, crate::app::TerminalKind::Cli, None),
        config,
        ui,
    )
    .with_launch(launch)
    .with_plugins(plugins)
    .with_session(session)
    .with_native_commands(test_native_registry())
    .build();

    assert_eq!(state.runtime.ui.verbosity, MessageLevel::Trace);
    assert_eq!(state.runtime.ui.debug_level, 2);
    assert_eq!(state.session.prompt_prefix, "osp-dev");
    assert_eq!(
        state.clients.plugins().explicit_dirs(),
        &[std::path::PathBuf::from("/tmp/osp-plugin-a")]
    );
    assert_eq!(
        state.clients.plugins().config_root(),
        Some(std::path::Path::new("/tmp/osp-config"))
    );
    assert_eq!(
        state.clients.plugins().cache_root(),
        Some(std::path::Path::new("/tmp/osp-cache"))
    );
    assert!(state.clients.native_commands().command("ldap").is_some());

    let config = test_config(&[
        ("ui.verbosity", "trace"),
        ("debug.level", "2"),
        ("theme.name", "dracula"),
    ]);
    let context = crate::app::RuntimeContext::new(
        None,
        crate::app::TerminalKind::Cli,
        Some("xterm-256color".to_string()),
    );
    let ui = crate::app::UiState::from_resolved_config(&context, &config)
        .expect("ui state should derive from resolved config");
    assert_eq!(ui.verbosity, MessageLevel::Trace);
    assert_eq!(ui.debug_level, 2);
    assert_eq!(ui.render_settings.theme_name, "dracula");

    let state = crate::app::AppState::from_resolved_config(context, config)
        .expect("app state should derive from resolved config");
    assert_eq!(state.runtime.ui.verbosity, MessageLevel::Trace);
    assert_eq!(state.runtime.ui.debug_level, 2);
    assert_eq!(state.runtime.ui.render_settings.theme_name, "dracula");
    assert!(state.clients.plugins().explicit_dirs().is_empty());

    let clients = crate::app::AppClients::new(
        crate::plugin::PluginManager::new(vec![std::path::PathBuf::from("/tmp/osp-plugin-a")]),
        test_native_registry(),
    );
    assert_eq!(
        clients.plugins().explicit_dirs(),
        &[std::path::PathBuf::from("/tmp/osp-plugin-a")]
    );
    assert!(clients.native_commands().command("ldap").is_some());
}

#[cfg_attr(miri, ignore = "public app bootstrap integration test")]
#[test]
fn state_builder_preserves_derived_plugin_and_theme_state_unit() {
    let config = test_config(&[
        ("ui.verbosity", "trace"),
        ("debug.level", "2"),
        ("theme.name", "dracula"),
    ]);
    let context = crate::app::RuntimeContext::new(
        None,
        crate::app::TerminalKind::Cli,
        Some("xterm-256color".to_string()),
    );
    let launch = crate::app::LaunchContext::default().with_plugin_dir("/tmp/osp-plugin-a");
    let state = crate::app::AppStateBuilder::from_resolved_config(context, config)
        .expect("host inputs should derive")
        .with_launch(launch)
        .build();

    assert_eq!(state.runtime.ui.verbosity, MessageLevel::Trace);
    assert_eq!(state.runtime.ui.debug_level, 2);
    assert_eq!(state.runtime.ui.render_settings.theme_name, "dracula");
    assert_eq!(
        state.clients.plugins().explicit_dirs(),
        &[std::path::PathBuf::from("/tmp/osp-plugin-a")]
    );
}

#[test]
fn app_session_helpers_cover_public_session_surface_unit() {
    let mut neutral_session = crate::app::AppSession::default();
    neutral_session.scope.enter("theme");
    assert_eq!(
        neutral_session.scope.help_tokens(),
        vec!["theme".to_string(), "--help".to_string()]
    );

    let mut overrides = ConfigLayer::default();
    overrides.set("extensions.site.enabled", true);
    let history_shell = HistoryShellContext::default();

    let built_session = crate::app::AppSession::default()
        .with_prompt_prefix("builder")
        .with_history_enabled(false)
        .with_history_shell(history_shell.clone())
        .with_config_overrides(overrides.clone());
    assert_eq!(built_session.prompt_prefix, "builder");
    assert!(!built_session.history_enabled);
    assert_eq!(
        built_session
            .config_overrides
            .entries()
            .iter()
            .filter(|entry| entry.key == "extensions.site.enabled")
            .count(),
        1
    );

    let mut session = crate::app::AppSession::default()
        .with_prompt_prefix("demo")
        .with_history_enabled(false)
        .with_history_shell(history_shell)
        .with_config_overrides(overrides);
    assert_eq!(session.prompt_prefix, "demo");
    assert!(!session.history_enabled);

    session.record_failure("   ", "ignored", "ignored");
    assert!(session.last_failure.is_none());
    session.record_failure("ldap user alice", "boom", "boom detail");
    assert_eq!(
        session
            .last_failure
            .as_ref()
            .expect("failure should be recorded")
            .summary,
        "boom"
    );

    session.record_prompt_timing(1, std::time::Duration::from_millis(10), None, None, None);
    assert!(session.prompt_timing.badge().is_some());
    session.record_prompt_timing(0, std::time::Duration::from_millis(0), None, None, None);
    assert!(session.prompt_timing.badge().is_none());
}

#[test]
fn prepare_plugin_response_handles_failures_and_pipeline_hints_unit() {
    let response = ResponseV1 {
        protocol_version: 1,
        ok: false,
        data: serde_json::json!({
            "code": "vm_ambiguous",
            "candidates": [{"name": "db01.uio.no", "provider": "vmware"}]
        }),
        error: Some(ResponseErrorV1 {
            code: "NOT_FOUND".to_string(),
            message: "missing user".to_string(),
            details: serde_json::json!({}),
        }),
        messages: vec![ResponseMessageV1 {
            level: MessageLevel::Warning,
            text: "queried fallback backend".to_string(),
        }],
        context: None,
        meta: ResponseMetaV1::default(),
    };

    let prepared = super::command_output::cli_result_from_plugin_response(response, &[])
        .expect("protocol failure should still parse");
    assert_eq!(prepared.exit_code, 1);
    let report = prepared.failure_report.expect("expected failure response");
    let crate::app::CommandBody::Output(output) = prepared.body else {
        panic!("expected structured failure data");
    };
    let rendered = crate::ui::render_messages(
        &prepared.messages,
        &crate::ui::MessageRequest::new(
            &crate::ui::RenderSettings::test_plain(OutputFormat::Auto),
            MessageLevel::Trace,
        ),
    );
    assert!(rendered.contains("queried fallback backend"));
    assert_eq!(report.to_string(), "missing user");
    assert_eq!(report.to_string(), "missing user");
    assert_eq!(
        output
            .document
            .as_ref()
            .map(|document| document.json_value()),
        None
    );
    assert_eq!(
        crate::core::output_model::output_items_to_value(&output.items),
        serde_json::json!({
            "code": "vm_ambiguous",
            "candidates": [{"name": "db01.uio.no", "provider": "vmware"}]
        })
    );

    let response = ResponseV1 {
        protocol_version: 1,
        ok: true,
        data: serde_json::json!([{"uid": "alice"}]),
        error: None,
        messages: Vec::new(),
        context: None,
        meta: ResponseMetaV1 {
            detail_columns: Vec::new(),
            format_hint: Some("table".to_string()),
            columns: Some(vec!["uid".to_string()]),
            column_align: Vec::new(),
            column_labels: Vec::new(),
            row_path: None,
            unix_timestamp_columns: Vec::new(),
            display_rules: Vec::new(),
            preserve_json_document: false,
            presentation_lines: Vec::new(),
            progress_append: Vec::new(),
            progress_replace: false,
            display_limit: None,
            partial_rows: None,
        },
    };
    let prepared =
        super::command_output::cli_result_from_plugin_response(response, &["P uid".to_string()])
            .and_then(|result| {
                super::command_output::apply_stages_to_cli_result(result, &["P uid".to_string()])
            })
            .expect("pipeline should apply");
    assert_eq!(prepared.exit_code, 0);
    assert!(prepared.failure_report.is_none());
    let crate::app::CommandBody::Output(output) = prepared.body else {
        panic!("expected successful output response");
    };
    assert!(prepared.format_hint.is_none());
    assert!(output.document.is_none());
}

#[test]
fn plugin_failure_diagnostics_keep_backend_evidence_out_of_the_terse_message() {
    let response = ResponseV1 {
        protocol_version: 1,
        ok: false,
        data: serde_json::json!({}),
        error: Some(ResponseErrorV1 {
            code: "resource_not_found".to_string(),
            message: "No visible resource matched".to_string(),
            details: serde_json::json!({
                "http_status": 404,
                "problem": {"ref": "missing.uio.no", "targets": []}
            }),
        }),
        messages: vec![],
        context: None,
        meta: ResponseMetaV1::default(),
    };
    let result = super::command_output::cli_result_from_plugin_response(response, &[])
        .expect("protocol failure should parse");
    let report = result
        .failure_report
        .expect("failure should carry diagnostics");
    let settings = RenderSettings::test_plain(OutputFormat::Auto);

    let terse = render_report_message(&report, MessageLevel::Success, &settings);
    assert!(terse.contains("No visible resource matched"), "{terse}");
    assert!(!terse.contains("http_status"));

    let normal = render_report_message(&report, MessageLevel::Info, &settings);
    assert!(!normal.contains("http_status"));
    assert!(!normal.contains("missing.uio.no"));

    let debug = render_report_message(&report, MessageLevel::Trace, &settings);
    assert!(!debug.contains("Backend details"));
    assert!(!debug.contains("missing.uio.no"));
}

#[test]
fn prepared_plugin_response_maps_into_cli_command_result_unit() {
    let response = ResponseV1 {
        protocol_version: 1,
        ok: false,
        data: serde_json::json!({}),
        error: Some(ResponseErrorV1 {
            code: "NOT_FOUND".to_string(),
            message: "missing user".to_string(),
            details: serde_json::json!({}),
        }),
        messages: vec![ResponseMessageV1 {
            level: MessageLevel::Warning,
            text: "queried fallback backend".to_string(),
        }],
        context: None,
        meta: ResponseMetaV1::default(),
    };
    let result = super::command_output::cli_result_from_plugin_response(response, &[])
        .expect("protocol failure should still parse");

    assert_eq!(result.exit_code, 1);
    let crate::app::CommandBody::Output(output) = &result.body else {
        panic!("structured failure data should remain renderable");
    };
    assert_eq!(
        crate::core::output_model::output_items_to_value(&output.items),
        serde_json::json!({})
    );
    assert_eq!(
        result.failure_report.as_ref().map(ToString::to_string),
        Some("missing user".to_string())
    );
    assert!(!result.messages.is_empty());
}

#[test]
fn exit_code_classification_reserves_specific_codes_for_usage_unit() {
    with_test_xdg_env(|| {
        let clap_report = super::run_from(["osp", "--defaults-only", "--definitely-not-a-flag"])
            .expect_err("parse should fail");
        assert_eq!(classify_exit_code(&clap_report), EXIT_CODE_USAGE);

        let mut invalid_session = ConfigLayer::default();
        invalid_session.set("ui.verbosity", "definitely-invalid");
        let config_report = super::resolve_runtime_config(
            RuntimeConfigRequest::new(None, Some("cli"))
                .with_runtime_load(crate::config::RuntimeLoadOptions::defaults_only())
                .with_session_layer(Some(invalid_session)),
        )
        .expect_err("config resolution should fail");
        assert_eq!(classify_exit_code(&config_report), EXIT_CODE_ERROR);

        let plugin_report = miette::miette!("unknown plugin ldap");
        assert_eq!(classify_exit_code(&plugin_report), EXIT_CODE_ERROR);
    });
}
