use super::command_output::{apply_output_stages, run_cli_command};
use super::help::parse_help_render_overrides;
use super::{
    CommandOutput, EXIT_CODE_ERROR, EXIT_CODE_USAGE, RunAction, RuntimeConfigRequest,
    build_cli_session_layer, build_dispatch_plan, classify_exit_code, is_sensitive_key,
    render_report_message, resolve_invocation_ui, resolve_render_settings_with_hint,
    run_inline_builtin_command,
};
use super::{
    authorized_command_catalog_for, command_output, resolve_runtime_config, run_from,
    run_from_with_sink,
};
use crate::app::sink::BufferedUiSink;
use crate::app::{AppState, LaunchContext, RuntimeContext, TerminalKind};
use crate::cli::commands::doctor as doctor_cmd;
use crate::cli::invocation::{InvocationOptions, scan_cli_argv};
use crate::cli::{Cli, Commands, ConfigCommands, PluginsCommands, ThemeCommands};
use crate::config::{ConfigLayer, ConfigResolver, ConfigValue, ResolveOptions, RuntimeLoadOptions};
use crate::core::document::Document;
use crate::core::output::{ColorMode, OutputFormat, RenderMode, UnicodeMode};
use crate::core::plugin::{
    DescribeCommandAuthV1, DescribeCommandV1, DescribeVisibilityModeV1, PLUGIN_PROTOCOL_V1,
    ResponseErrorV1, ResponseMessageV1, ResponseMetaV1, ResponseV1,
};
use crate::plugin::{CommandCatalogEntry, PluginManager, PluginSource};
use crate::repl;
use crate::repl::{HistoryConfig, HistoryShellContext, SharedHistory};
use crate::repl::{completion, dispatch as repl_dispatch, help as repl_help, surface};
use crate::ui::build_presentation_defaults_layer;
use crate::ui::messages::{MessageBuffer, MessageLevel};
use crate::ui::{RenderSettings, render};
use crate::{NativeCommand, NativeCommandContext, NativeCommandOutcome, NativeCommandRegistry};
use clap::Command;
use clap::Parser;
use serde_json::json;
use std::collections::BTreeMap;
use std::ffi::OsString;

mod app_runtime;
mod cli_dispatch;
mod command_surfaces;
mod presentation;
mod repl_completion;
#[cfg(unix)]
mod repl_runtime;

fn repl_view<'a>(
    runtime: &'a crate::app::AppRuntime,
    session: &'a crate::app::AppSession,
) -> repl::ReplViewContext<'a> {
    repl::ReplViewContext::from_parts(runtime, session)
}

fn make_completion_state() -> AppState {
    make_completion_state_with_entries_and_native(&[], NativeCommandRegistry::default())
}

fn make_completion_state_with_entries(entries: &[(&str, &str)]) -> AppState {
    make_completion_state_with_entries_and_native(entries, NativeCommandRegistry::default())
}

fn make_completion_state_with_entries_and_native(
    entries: &[(&str, &str)],
    native_commands: NativeCommandRegistry,
) -> AppState {
    let mut defaults = ConfigLayer::default();
    defaults.set("profile.default", "default");
    defaults.set("repl.history.path", "/tmp/osp-test-repl.history");
    defaults.set(
        "repl.shellable_commands",
        ["ldap", "mreg", "nh", "orch", "vm"]
            .map(str::to_string)
            .to_vec(),
    );
    defaults.set("theme.path", Vec::<String>::new());
    for (key, value) in entries {
        defaults.set(*key, *value);
    }
    let mut layers = crate::config::LoadedLayers {
        defaults,
        ..crate::config::LoadedLayers::default()
    };
    let options = ResolveOptions::default().with_terminal("repl");
    let base = ConfigResolver::from_loaded_layers_with_schema(
        layers.clone(),
        crate::config::ConfigSchema::default(),
    )
    .resolve(options.clone())
    .expect("base test config should resolve");
    layers.presentation = build_presentation_defaults_layer(&base);
    let config = ConfigResolver::from_loaded_layers_with_schema(
        layers.clone(),
        crate::config::ConfigSchema::default(),
    )
    .resolve(options)
    .expect("test config should resolve");

    let settings = RenderSettings::test_plain(OutputFormat::Json);

    let mut state = crate::app::AppStateBuilder {
        context: RuntimeContext::new(None, TerminalKind::Repl, None),
        config,
        launch: LaunchContext::default(),
        native_commands,
        ui: crate::app::UiState::new(settings, MessageLevel::Success, 0),
        plugins: Some(
            PluginManager::new(Vec::new())
                .with_bundled_roots(false)
                .with_default_roots(false),
        ),
        themes: Some(crate::ui::theme_catalog::ThemeCatalog::default()),
        session: None,
        definition: Default::default(),
    }
    .build();
    state
        .session
        .config_overrides
        .set("theme.path", Vec::<String>::new());
    state
}

struct TestNativeCommand;

impl NativeCommand for TestNativeCommand {
    fn command(&self) -> Command {
        Command::new("ldap")
            .about("Directory lookup")
            .subcommand(Command::new("user").about("Look up a user"))
    }

    fn auth(&self) -> Option<DescribeCommandAuthV1> {
        Some(DescribeCommandAuthV1 {
            visibility: Some(DescribeVisibilityModeV1::Public),
            required_capabilities: Vec::new(),
            feature_flags: Vec::new(),
            ..DescribeCommandAuthV1::default()
        })
    }

    fn describe(&self) -> DescribeCommandV1 {
        let mut describe = DescribeCommandV1::from_clap(self.command());
        describe.auth = self.auth();
        describe.subcommands[0].auth = Some(DescribeCommandAuthV1 {
            visibility: Some(DescribeVisibilityModeV1::Authenticated),
            required_capabilities: vec!["ldap.user.read".to_string()],
            feature_flags: Vec::new(),
            ..DescribeCommandAuthV1::default()
        });
        describe
    }

    fn execute(
        &self,
        args: &[String],
        _context: &NativeCommandContext<'_>,
    ) -> anyhow::Result<NativeCommandOutcome> {
        if args.iter().any(|arg| arg == "--help") {
            return Ok(NativeCommandOutcome::Help(
                "Usage: osp ldap [COMMAND]\n\nCommands:\n  user  Look up a user\n".to_string(),
            ));
        }

        Ok(NativeCommandOutcome::Response(Box::new(ResponseV1 {
            protocol_version: PLUGIN_PROTOCOL_V1,
            ok: true,
            data: json!([{ "command": "ldap", "args": args }]),
            error: None,
            messages: Vec::new(),
            context: None,
            meta: ResponseMetaV1 {
                context_not_found: false,
                detail_columns: Vec::new(),
                context: Default::default(),
                format_hint: Some("table".to_string()),
                columns: Some(vec!["command".to_string(), "args".to_string()]),
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
        })))
    }
}

fn test_native_registry() -> NativeCommandRegistry {
    NativeCommandRegistry::new().with_command(TestNativeCommand)
}

fn test_config(entries: &[(&str, &str)]) -> crate::config::ResolvedConfig {
    let mut defaults = ConfigLayer::default();
    defaults.set("profile.default", "default");
    defaults.set("theme.path", Vec::<String>::new());
    for (key, value) in entries {
        defaults.set(*key, *value);
    }

    let mut layers = crate::config::LoadedLayers {
        defaults,
        ..crate::config::LoadedLayers::default()
    };
    let options = ResolveOptions::default().with_terminal("cli");
    let base = ConfigResolver::from_loaded_layers_with_schema(
        layers.clone(),
        crate::config::ConfigSchema::default(),
    )
    .resolve(options.clone())
    .expect("base test config should resolve");
    layers.presentation = build_presentation_defaults_layer(&base);
    ConfigResolver::from_loaded_layers_with_schema(
        layers.clone(),
        crate::config::ConfigSchema::default(),
    )
    .resolve(options)
    .expect("test config should resolve")
}

fn render_prompt_snapshot(entries: &[(&str, &str)]) -> String {
    let state = make_completion_state_with_entries(entries);
    crate::repl::presentation::build_repl_prompt(repl_view(&state.runtime, &state.session)).left
}

fn sample_catalog() -> Vec<CommandCatalogEntry> {
    vec![CommandCatalogEntry {
        name: "orch".to_string(),
        about: "Provision orchestrator resources".to_string(),
        auth: None,
        subcommands: vec!["provision".to_string(), "status".to_string()],
        completion: crate::completion::CompletionNode {
            name: "orch".to_string(),
            tooltip: Some("Provision orchestrator resources".to_string()),
            children: vec![
                crate::completion::CompletionNode::new("provision"),
                crate::completion::CompletionNode::new("status"),
            ]
            .into_iter()
            .map(|child| (child.name.clone(), child))
            .collect(),
            ..crate::completion::CompletionNode::default()
        },

        source: crate::plugin::CommandSource::Plugin(PluginSource::Explicit),
    }]
}

fn sample_catalog_with_provision_context() -> Vec<CommandCatalogEntry> {
    let mut catalog = vec![CommandCatalogEntry {
        name: "orch".to_string(),
        about: "Provision orchestrator resources".to_string(),
        auth: None,
        subcommands: vec!["provision".to_string(), "status".to_string()],
        completion: crate::completion::CompletionNode {
            name: "orch".to_string(),
            tooltip: Some("Provision orchestrator resources".to_string()),
            children: vec![
                crate::completion::CompletionNode::new("provision")
                    .arg(
                        crate::completion::ArgNode::named("guest")
                            .tooltip("Guest name for the provision request"),
                    )
                    .arg(
                        crate::completion::ArgNode::named("image")
                            .tooltip("Base image to provision")
                            .suggestions([
                                crate::completion::SuggestionEntry::from("ubuntu"),
                                crate::completion::SuggestionEntry::from("alma"),
                            ]),
                    )
                    .flag(
                        "--provider",
                        crate::completion::FlagNode::new().suggestions([
                            crate::completion::SuggestionEntry::from("alpha"),
                            crate::completion::SuggestionEntry::from("beta"),
                        ]),
                    )
                    .flag(
                        "--image",
                        crate::completion::FlagNode {
                            suggestions: vec![
                                crate::completion::SuggestionEntry::from("red"),
                                crate::completion::SuggestionEntry::from("blue"),
                            ],
                            ..crate::completion::FlagNode::default()
                        },
                    ),
                crate::completion::CompletionNode::new("status"),
            ]
            .into_iter()
            .map(|child| (child.name.clone(), child))
            .collect(),
            ..crate::completion::CompletionNode::default()
        },

        source: crate::plugin::CommandSource::Plugin(PluginSource::Explicit),
    }];
    catalog[0]
        .completion
        .children
        .get_mut("provision")
        .unwrap()
        .dynamic = Some(crate::completion::DynamicCompletion::new(|cmd| {
        let values = match cmd
            .flag_values("--provider")
            .and_then(|values| values.first())
            .map(String::as_str)
        {
            Some("alpha") => vec![crate::completion::SuggestionEntry::from("red")],
            Some("beta") => vec![crate::completion::SuggestionEntry::from("blue")],
            _ => vec![
                crate::completion::SuggestionEntry::from("red"),
                crate::completion::SuggestionEntry::from("blue"),
            ],
        };
        crate::completion::FlagCompletions {
            values: BTreeMap::from([("--image".into(), values)]),
            ..Default::default()
        }
    }));
    catalog
}
fn layer_value<'a>(layer: &'a ConfigLayer, key: &str) -> Option<&'a ConfigValue> {
    layer
        .entries()
        .iter()
        .find(|entry| entry.key == key)
        .map(|entry| &entry.value)
}

#[cfg(unix)]
fn make_test_history(state: &mut AppState) -> SharedHistory {
    let history_shell = state.session.history_shell.clone();
    state.sync_history_shell_context();

    let history_config = HistoryConfig {
        legacy_path: None,
        path: miri_history_path(),
        max_entries: 128,
        enabled: true,
        dedupe: true,
        exclude_patterns: Vec::new(),
        shell_context: history_shell,
    }
    .normalized();

    SharedHistory::new(history_config)
}

#[cfg(unix)]
fn make_test_state(plugin_dirs: Vec<std::path::PathBuf>) -> AppState {
    let mut defaults = ConfigLayer::default();
    defaults.set("profile.default", "default");
    defaults.set(
        "repl.history.path",
        "/tmp/osp-repl-runtime-tests-history.jsonl",
    );
    defaults.set("theme.path", Vec::<String>::new());
    let layers = crate::config::LoadedLayers {
        defaults,
        ..crate::config::LoadedLayers::default()
    };
    let config = ConfigResolver::from_loaded_layers_with_schema(
        layers.clone(),
        crate::config::ConfigSchema::default(),
    )
    .resolve(ResolveOptions::default().with_terminal("repl"))
    .expect("test config should resolve");

    let settings = RenderSettings::test_plain(OutputFormat::Json);

    let config_root = miri_root_dir("osp-cli-test-config");
    let cache_root = miri_root_dir("osp-cli-test-cache");
    let launch = LaunchContext::default()
        .with_plugin_dirs(plugin_dirs.clone())
        .with_config_root(config_root.clone())
        .with_cache_root(cache_root.clone())
        .with_runtime_load(if cfg!(miri) {
            RuntimeLoadOptions::defaults_only()
        } else {
            RuntimeLoadOptions::default()
        });

    let mut state = crate::app::AppStateBuilder {
        context: RuntimeContext::new(None, TerminalKind::Repl, None),
        config,
        launch,
        native_commands: crate::native::NativeCommandRegistry::default(),
        ui: crate::app::UiState::new(settings, MessageLevel::Success, 0),
        plugins: Some(
            PluginManager::new(plugin_dirs)
                .with_roots(config_root, cache_root)
                .with_bundled_roots(!cfg!(miri))
                .with_default_roots(!cfg!(miri)),
        ),
        themes: Some(crate::ui::theme_catalog::ThemeCatalog::default()),
        session: None,
        definition: Default::default(),
    }
    .build();
    state
        .session
        .config_overrides
        .set("theme.path", Vec::<String>::new());
    state
}

#[cfg(unix)]
fn miri_history_path() -> Option<std::path::PathBuf> {
    if cfg!(miri) {
        None
    } else {
        Some(make_temp_dir("osp-cli-test-history").join("history.jsonl"))
    }
}

#[cfg(unix)]
fn miri_root_dir(prefix: &str) -> Option<std::path::PathBuf> {
    if cfg!(miri) {
        None
    } else {
        Some(make_temp_dir(prefix).to_path_buf())
    }
}

#[cfg(unix)]
fn with_temp_config_paths<T>(callback: impl FnOnce() -> T) -> T {
    let root = make_temp_dir("osp-cli-test-config-paths");
    let config_path = root.join("config.toml");
    let secrets_path = root.join("secrets.toml");
    let previous_config = std::env::var_os("OSP_CONFIG_FILE");
    let previous_secrets = std::env::var_os("OSP_SECRETS_FILE");

    unsafe {
        std::env::set_var("OSP_CONFIG_FILE", &config_path);
        std::env::set_var("OSP_SECRETS_FILE", &secrets_path);
    }

    let result = callback();

    match previous_config {
        Some(value) => unsafe { std::env::set_var("OSP_CONFIG_FILE", value) },
        None => unsafe { std::env::remove_var("OSP_CONFIG_FILE") },
    }
    match previous_secrets {
        Some(value) => unsafe { std::env::set_var("OSP_SECRETS_FILE", value) },
        None => unsafe { std::env::remove_var("OSP_SECRETS_FILE") },
    }
    result
}

#[cfg(unix)]
fn write_pipeline_test_plugin(dir: &std::path::Path) -> std::path::PathBuf {
    use std::os::unix::fs::PermissionsExt;

    let plugin_path = dir.join("osp-hello");
    std::fs::write(
            &plugin_path,
            r#"#!/bin/sh
PATH=/usr/bin:/bin:$PATH
if [ "$1" = "--osp-describe" ]; then
  cat <<'JSON'
{"about":"hello plugin","args":[],"flags":{},"subcommands":[],"output":"json"}
JSON
  exit 0
fi

cat <<'JSON'
{"protocol_version":1,"ok":true,"data":{"message":"hello-from-plugin"},"error":null,"meta":{"format_hint":"table","columns":["message"]}}
JSON
"#,
        )
        .expect("plugin script should be written");
    let mut perms = std::fs::metadata(&plugin_path)
        .expect("metadata should be readable")
        .permissions();
    perms.set_mode(0o755);
    std::fs::set_permissions(&plugin_path, perms).expect("script should be executable");
    plugin_path
}
#[cfg(unix)]
fn make_temp_dir(prefix: &str) -> crate::tests::TestTempDir {
    crate::tests::make_temp_dir(prefix)
}

mod launch;

#[cfg(unix)]
#[path = "../../tests/support/plugin.rs"]
mod plugin_support;
