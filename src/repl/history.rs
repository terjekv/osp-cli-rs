//! REPL history policy and builtin commands.
//!
//! This module translates resolved config into history behavior, scopes
//! history entries to the current REPL context, and implements the visible
//! `history` builtin commands on top of the shared history store.

use crate::config::{
    ConfigValue, DEFAULT_REPL_HISTORY_MAX_ENTRIES, ResolvedConfig, RuntimeDefaults,
};
use crate::core::command_def::{CommandDef, FlagDef};
use crate::core::row::Row;
use crate::repl::{HistoryConfig, HistoryEntry, SharedHistory};
use crate::ui::theme::DEFAULT_THEME_NAME;
use miette::{Result, miette};
use std::path::PathBuf;

use crate::app::{AppRuntime, AppSession};
use crate::cli::{HistoryArgs, HistoryCommands, HistoryPruneArgs};

use crate::app::{
    CMD_HISTORY, CMD_LIST, DEFAULT_REPL_PROMPT, ReplCommandOutput, StructuredCommandOutput,
    config_usize,
};
use crate::cli::rows::output::rows_to_output_result;

const DEFAULT_REPL_HISTORY_EXCLUDES: [&str; 4] = ["exit", "quit", "help", "history list"];
const DEFAULT_HISTORY_LIST_LIMIT: usize = 20;
const HISTORY_TIMESTAMP_EXAMPLES: &str = "expected YYYY-MM-DD, YYYY-MM-DD HH:MM[:SS], or RFC3339-like input such as 2026-08-11T10:00:00Z or 2026-08-11T12:00:00+02:00";

#[derive(Debug, Clone, PartialEq, Eq)]
struct HistoryScopeView {
    prefix: Option<String>,
    label: String,
}

impl HistoryScopeView {
    fn from_session(session: &AppSession) -> Self {
        Self {
            prefix: session.scope.history_scope_prefix(),
            label: session.scope.history_scope_label(),
        }
    }
}

pub(crate) fn history_command_def(sort_key: impl Into<String>) -> CommandDef {
    CommandDef::new(CMD_HISTORY)
        .about("Inspect or prune REPL history")
        .sort(sort_key)
        .subcommands([
            CommandDef::new(CMD_LIST)
                .about("List recent history")
                .sort("10")
                .flags([
                    FlagDef::new("limit")
                        .long("limit")
                        .takes_value("COUNT")
                        .help("Maximum recent matching entries to show (default: 20)"),
                    FlagDef::new("since")
                        .long("since")
                        .takes_value("TIMESTAMP")
                        .help(format!(
                            "Inclusive lower timestamp; {HISTORY_TIMESTAMP_EXAMPLES}"
                        )),
                    FlagDef::new("until")
                        .long("until")
                        .takes_value("TIMESTAMP")
                        .help(format!(
                            "Inclusive upper timestamp; {HISTORY_TIMESTAMP_EXAMPLES}"
                        )),
                ]),
            CommandDef::new("prune")
                .about("Keep last N entries")
                .sort("11"),
            CommandDef::new("clear").about("Clear history").sort("12"),
        ])
}

pub(crate) fn build_history_config(runtime: &AppRuntime, session: &AppSession) -> HistoryConfig {
    let config = runtime.config.resolved();
    session.sync_history_shell_context();
    HistoryConfig {
        path: Some(
            config
                .get_string("repl.history.path")
                .map(PathBuf::from)
                .unwrap_or_else(default_repl_history_path),
        ),
        max_entries: config_usize(
            config,
            "repl.history.max_entries",
            DEFAULT_REPL_HISTORY_MAX_ENTRIES as usize,
        ),
        enabled: repl_history_enabled(config) && session.history_enabled,
        dedupe: config.get_bool("repl.history.dedupe").unwrap_or(true),
        profile_scoped: config
            .get_bool("repl.history.profile_scoped")
            .unwrap_or(true),
        exclude_patterns: repl_history_exclude_patterns(config),
        profile: Some(config.active_profile().to_string()),
        terminal: Some(
            runtime
                .context
                .terminal_kind()
                .as_config_terminal()
                .to_string(),
        ),
        shell_context: session.history_shell.clone(),
    }
    .normalized()
}

pub(crate) fn repl_history_enabled(config: &ResolvedConfig) -> bool {
    config.get_bool("repl.history.enabled").unwrap_or(true)
        && config_usize(
            config,
            "repl.history.max_entries",
            DEFAULT_REPL_HISTORY_MAX_ENTRIES as usize,
        ) > 0
}

pub(crate) fn run_history_repl_command(
    session: &mut AppSession,
    args: HistoryArgs,
    history: &SharedHistory,
) -> Result<ReplCommandOutput> {
    if !history
        .with(|store| store.history_enabled())
        .unwrap_or(false)
    {
        return Ok(ReplCommandOutput::Text(
            "History is disabled.\n".to_string(),
        ));
    }

    let scope = HistoryScopeView::from_session(session);
    match args.command {
        HistoryCommands::List(list) => {
            let since = parse_history_timestamp("--since", list.since.as_deref())?;
            let until = parse_history_timestamp("--until", list.until.as_deref())?;
            if since.zip(until).is_some_and(|(since, until)| since > until) {
                return Err(miette!("--since must not be later than --until"));
            }
            let mut entries = filter_history_entries(
                history
                    .with(|store| store.list_entries_for(scope.prefix.as_deref()))
                    .unwrap_or_default(),
                since,
                until,
            );
            entries = entries
                .into_iter()
                .rev()
                .take(
                    list.limit
                        .map(std::num::NonZeroUsize::get)
                        .unwrap_or(DEFAULT_HISTORY_LIST_LIMIT),
                )
                .collect::<Vec<_>>();
            entries.reverse();
            let rows = history_entries_rows(entries);
            let mut output = rows_to_output_result(rows);
            output.meta.display_columns = Some(["id", "command"].map(str::to_string).to_vec());
            Ok(ReplCommandOutput::Output(Box::new(
                StructuredCommandOutput {
                    source_document: None,
                    output,
                    format_hint: None,
                },
            )))
        }
        HistoryCommands::Prune(HistoryPruneArgs { keep }) => {
            let removed = history
                .with(|store| store.prune_for(keep, scope.prefix.as_deref()))
                .map_err(anyhow::Error::msg)
                .and_then(|result| result)
                .map_err(|err| {
                    crate::app::report_anyhow_with_context(err, "failed to prune REPL history")
                })?;
            Ok(ReplCommandOutput::Text(if removed == 0 {
                format!("No entries removed from {}.\n", scope.label)
            } else {
                format!(
                    "Removed {removed} entr{} from {}.\n",
                    if removed == 1 { "y" } else { "ies" },
                    scope.label
                )
            }))
        }
        HistoryCommands::Clear => {
            let removed = history
                .with(|store| store.clear_for(scope.prefix.as_deref()))
                .map_err(anyhow::Error::msg)
                .and_then(|result| result)
                .map_err(|err| {
                    crate::app::report_anyhow_with_context(err, "failed to clear REPL history")
                })?;
            Ok(ReplCommandOutput::Text(if removed == 0 {
                format!("{} is already empty.\n", scope.label)
            } else {
                format!("Cleared {}.\n", scope.label)
            }))
        }
    }
}

fn parse_history_timestamp(flag: &str, value: Option<&str>) -> Result<Option<i64>> {
    value
        .map(|value| {
            crate::dsl::parse_timestamp(value, flag == "--until")
                .map(|time| time.timestamp_millis())
                .ok_or_else(|| {
                    miette!("invalid {flag} timestamp `{value}`; {HISTORY_TIMESTAMP_EXAMPLES}")
                })
        })
        .transpose()
}

fn filter_history_entries(
    entries: Vec<HistoryEntry>,
    since_ms: Option<i64>,
    until_ms: Option<i64>,
) -> Vec<HistoryEntry> {
    if since_ms.is_none() && until_ms.is_none() {
        return entries;
    }
    entries
        .into_iter()
        .filter(|entry| {
            entry.timestamp_ms.is_some_and(|timestamp| {
                since_ms.is_none_or(|since| timestamp >= since)
                    && until_ms.is_none_or(|until| timestamp <= until)
            })
        })
        .collect()
}

fn history_entries_rows(entries: Vec<HistoryEntry>) -> Vec<Row> {
    let mut rows = Vec::with_capacity(entries.len());
    for entry in entries {
        let timestamp = entry
            .timestamp_ms
            .map_or(serde_json::Value::Null, |ms| ms.into());
        rows.push(crate::row! {
            "id" => entry.id,
            "timestamp_ms" => timestamp,
            "command" => entry.command,
        });
    }
    rows
}

fn config_string_list(config: &ResolvedConfig, key: &str) -> Vec<String> {
    match config.get(key).map(ConfigValue::reveal) {
        Some(ConfigValue::List(values)) => values
            .iter()
            .filter_map(|value| match value {
                ConfigValue::String(value) => Some(value.clone()),
                ConfigValue::Secret(secret) => match secret.expose() {
                    ConfigValue::String(value) => Some(value.clone()),
                    _ => None,
                },
                _ => None,
            })
            .collect(),
        Some(ConfigValue::String(value)) => vec![value.clone()],
        _ => Vec::new(),
    }
}

fn repl_history_exclude_patterns(config: &ResolvedConfig) -> Vec<String> {
    let mut patterns = config_string_list(config, "repl.history.exclude");
    for default in DEFAULT_REPL_HISTORY_EXCLUDES {
        if patterns.iter().any(|pattern| pattern == default) {
            continue;
        }
        patterns.push(default.to_string());
    }
    patterns
}

fn default_repl_history_path() -> PathBuf {
    let defaults = RuntimeDefaults::from_process_env(DEFAULT_THEME_NAME, DEFAULT_REPL_PROMPT);
    PathBuf::from(
        defaults
            .get_string("repl.history.path")
            .unwrap_or("${user.name}@${profile.active}.history"),
    )
}

#[cfg(test)]
mod tests {
    use super::{
        HistoryScopeView, build_history_config, history_command_def, repl_history_enabled,
        repl_history_exclude_patterns, run_history_repl_command,
    };
    use crate::app::ReplCommandOutput;
    use crate::app::{
        AppSession, AppState, AppStateInit, LaunchContext, RuntimeContext, TerminalKind,
    };
    use crate::cli::{HistoryArgs, HistoryCommands, HistoryListArgs, HistoryPruneArgs};
    use crate::config::{ConfigLayer, ConfigResolver, ResolveOptions};
    use crate::core::output::OutputFormat;
    use crate::repl::{HistoryConfig, SharedHistory};
    use crate::ui::RenderSettings;
    use crate::ui::messages::MessageLevel;
    use clap::Parser;
    use serde_json::Value;
    use std::path::PathBuf;

    #[test]
    fn history_exclude_patterns_include_repl_defaults() {
        let mut defaults = ConfigLayer::default();
        defaults.set("profile.default", "default");
        defaults.set("theme.path", Vec::<String>::new());
        let mut resolver = ConfigResolver::default();
        resolver.set_defaults(defaults);
        let resolved = resolver
            .resolve(ResolveOptions::default())
            .expect("config should resolve");

        let patterns = repl_history_exclude_patterns(&resolved);

        assert!(patterns.contains(&"exit".to_string()));
        assert!(patterns.contains(&"quit".to_string()));
        assert!(patterns.contains(&"help".to_string()));
        assert!(patterns.contains(&"history list".to_string()));
    }

    #[test]
    fn history_exclude_patterns_do_not_duplicate_defaults() {
        let mut defaults = ConfigLayer::default();
        defaults.set("profile.default", "default");
        defaults.set("theme.path", Vec::<String>::new());
        let mut session = ConfigLayer::default();
        session.set("repl.history.exclude", vec!["help".to_string()]);
        let mut resolver = ConfigResolver::default();
        resolver.set_defaults(defaults);
        resolver.set_session(session);
        let resolved = resolver
            .resolve(ResolveOptions::default())
            .expect("config should resolve");

        let patterns = repl_history_exclude_patterns(&resolved);
        assert_eq!(
            patterns
                .iter()
                .filter(|pattern| pattern.as_str() == "help")
                .count(),
            1
        );
    }

    #[test]
    fn history_scope_label_tracks_current_shell_unit() {
        let mut session = AppSession::default();
        assert_eq!(
            HistoryScopeView::from_session(&session).label,
            "root history"
        );

        session.scope.enter("orch");
        let scope = HistoryScopeView::from_session(&session);
        assert_eq!(scope.prefix.as_deref(), Some("orch "));
        assert_eq!(scope.label, "orch shell history");
    }

    #[test]
    fn history_command_def_exposes_expected_subcommands_unit() {
        let spec = history_command_def("20");
        let names = spec
            .subcommands
            .iter()
            .map(|child| child.name.as_str())
            .collect::<Vec<_>>();

        assert_eq!(spec.name, "history");
        assert_eq!(names, vec!["list", "prune", "clear"]);
    }

    #[test]
    fn repl_history_enabled_obeys_toggle_and_capacity_unit() {
        let disabled = config_with_entries(&[
            ("profile.default", "default"),
            ("repl.history.enabled", "false"),
        ]);
        assert!(!repl_history_enabled(&disabled));

        let positive_capacity_keeps_history_enabled = config_with_entries(&[
            ("profile.default", "default"),
            ("repl.history.max_entries", "1"),
        ]);
        assert!(repl_history_enabled(
            &positive_capacity_keeps_history_enabled
        ));

        let enabled = config_with_entries(&[("profile.default", "default")]);
        assert!(repl_history_enabled(&enabled));
    }

    #[test]
    fn build_history_config_reads_effective_defaults_and_overrides_unit() {
        let config = config_with_entries(&[
            ("profile.default", "default"),
            ("repl.history.max_entries", "42"),
            ("repl.history.enabled", "true"),
            ("repl.history.dedupe", "false"),
            ("repl.history.profile_scoped", "false"),
            ("repl.history.path", "/tmp/custom-history.jsonl"),
            ("repl.history.exclude", "help"),
        ]);

        let state = app_state(config);
        let policy = build_history_config(&state.runtime, &state.session);

        assert_eq!(policy.max_entries, 42);
        assert!(policy.enabled);
        assert_eq!(
            policy.path,
            Some(PathBuf::from("/tmp/custom-history.jsonl"))
        );
        assert!(!policy.dedupe);
        assert!(!policy.profile_scoped);
        assert!(policy.exclude_patterns.contains(&"help".to_string()));
        assert!(policy.exclude_patterns.contains(&"exit".to_string()));
    }

    #[test]
    fn build_history_config_tracks_current_shell_scope_without_manual_sync_unit() {
        let config = config_with_entries(&[("profile.default", "default")]);
        let mut state = app_state(config);

        state.session.enter_repl_scope("ldap");

        let history_config = build_history_config(&state.runtime, &state.session);

        assert_eq!(
            history_config.shell_context.prefix().as_deref(),
            Some("ldap ")
        );
    }

    #[test]
    fn build_history_config_respects_session_history_opt_out_unit() {
        let config = config_with_entries(&[("profile.default", "default")]);
        let mut state = app_state(config);
        state.session.history_enabled = false;

        let history_config = build_history_config(&state.runtime, &state.session);

        assert!(!history_config.enabled);
    }

    #[test]
    fn run_history_repl_command_reports_disabled_history_unit() {
        let history = shared_history(false);
        let mut session = AppSession::default();

        let output = run_history_repl_command(
            &mut session,
            HistoryArgs {
                command: HistoryCommands::List(HistoryListArgs::default()),
            },
            &history,
        )
        .expect("history command should return a disabled notice");

        match output {
            ReplCommandOutput::Text(text) => assert_eq!(text, "History is disabled.\n"),
            other => panic!("unexpected disabled history output: {other:?}"),
        }
    }

    #[test]
    fn run_history_repl_command_lists_visible_rows_unit() {
        let history = shared_history(true);
        history
            .with(|store| store.save_command_line("config show"))
            .expect("history lock should be available")
            .expect("history seed should save");
        let mut session = AppSession::default();

        let output = run_history_repl_command(
            &mut session,
            HistoryArgs {
                command: HistoryCommands::List(HistoryListArgs::default()),
            },
            &history,
        )
        .expect("history list should succeed");

        match output {
            ReplCommandOutput::Output(output) => {
                let rows = output.output.into_rows().expect("list should produce rows");
                assert_eq!(rows.len(), 1);
                assert_eq!(rows[0]["command"], Value::String("config show".to_string()));
                assert!(rows[0].contains_key("timestamp_ms"));
            }
            other => panic!("unexpected history list output: {other:?}"),
        }
    }

    #[test]
    fn history_list_filters_an_inclusive_iso_time_window_unit() {
        let temp = tempfile::tempdir().expect("history tempdir should exist");
        let path = temp.path().join("history.jsonl");
        let timestamp = |value: &str| {
            chrono::DateTime::parse_from_rfc3339(value)
                .expect("test timestamp should parse")
                .timestamp_millis()
        };
        let records = [
            serde_json::json!({
                "id": 0,
                "command_line": "ldap user before",
                "timestamp_ms": timestamp("2026-08-11T09:59:59Z")
            }),
            serde_json::json!({
                "id": 1,
                "command_line": "ldap user lower-bound",
                "timestamp_ms": timestamp("2026-08-11T10:00:00Z")
            }),
            serde_json::json!({
                "id": 2,
                "command_line": "ldap user upper-bound",
                "timestamp_ms": timestamp("2026-08-11T11:00:00Z")
            }),
            serde_json::json!({
                "id": 3,
                "command_line": "ldap user after",
                "timestamp_ms": timestamp("2026-08-11T11:00:01Z")
            }),
            serde_json::json!({
                "id": 4,
                "command_line": "mreg host in-window",
                "timestamp_ms": timestamp("2026-08-11T10:30:00Z")
            }),
        ];
        std::fs::write(
            &path,
            records
                .iter()
                .map(serde_json::Value::to_string)
                .collect::<Vec<_>>()
                .join("\n"),
        )
        .expect("history fixture should write");
        let history = SharedHistory::new(HistoryConfig {
            path: Some(path),
            max_entries: 32,
            dedupe: false,
            profile_scoped: false,
            ..HistoryConfig::default()
        });
        let mut session = AppSession::default();
        session.scope.enter("ldap");

        let output = run_history_repl_command(
            &mut session,
            HistoryArgs {
                command: HistoryCommands::List(HistoryListArgs {
                    limit: None,
                    since: Some("2026-08-11T10:00:00Z".to_string()),
                    until: Some("2026-08-11T11:00:00Z".to_string()),
                }),
            },
            &history,
        )
        .expect("bounded history list should succeed");

        let ReplCommandOutput::Output(output) = output else {
            panic!("history list should return structured rows");
        };
        let rows = output.output.into_rows().expect("list should produce rows");
        assert_eq!(
            rows.iter()
                .map(|row| row["command"].as_str().expect("command should be text"))
                .collect::<Vec<_>>(),
            vec!["user lower-bound", "user upper-bound"]
        );
    }

    #[test]
    fn history_list_help_and_invalid_bounds_show_iso_examples_unit() {
        let help = crate::cli::Cli::try_parse_from(["osp", "history", "list", "--help"])
            .err()
            .expect("help should stop parsing")
            .to_string();
        assert!(help.contains("--since <TIMESTAMP>"));
        assert!(help.contains("--until <TIMESTAMP>"));
        assert!(help.contains("2026-08-11T10:00:00Z"));
        assert!(help.contains("+02:00"));

        let history = shared_history(true);
        let mut session = AppSession::default();
        let error = run_history_repl_command(
            &mut session,
            HistoryArgs {
                command: HistoryCommands::List(HistoryListArgs {
                    limit: None,
                    since: Some("today".to_string()),
                    until: None,
                }),
            },
            &history,
        )
        .expect_err("natural-language bounds should be rejected")
        .to_string();
        assert!(error.contains("invalid --since timestamp `today`"));
        assert!(error.contains("YYYY-MM-DD"));
        assert!(error.contains("2026-08-11T10:00:00Z"));

        let reversed = run_history_repl_command(
            &mut session,
            HistoryArgs {
                command: HistoryCommands::List(HistoryListArgs {
                    limit: None,
                    since: Some("2026-08-11T11:00:00Z".to_string()),
                    until: Some("2026-08-11T10:00:00Z".to_string()),
                }),
            },
            &history,
        )
        .expect_err("reversed bounds should be rejected")
        .to_string();
        assert_eq!(reversed, "--since must not be later than --until");
    }

    #[test]
    fn run_history_repl_command_prunes_and_clears_with_scope_unit() {
        let history = shared_history(true);
        history
            .with(|store| store.save_command_line("ldap user alice"))
            .expect("history lock should be available")
            .expect("history seed should save");
        history
            .with(|store| store.save_command_line("ldap user bob"))
            .expect("history lock should be available")
            .expect("history seed should save");
        history
            .with(|store| store.save_command_line("mreg host a"))
            .expect("history lock should be available")
            .expect("history seed should save");
        let mut session = AppSession::default();
        session.scope.enter("ldap");

        let prune = run_history_repl_command(
            &mut session,
            HistoryArgs {
                command: HistoryCommands::Prune(HistoryPruneArgs { keep: 1 }),
            },
            &history,
        )
        .expect("scoped prune should succeed");
        match prune {
            ReplCommandOutput::Text(text) => {
                assert_eq!(text, "Removed 1 entry from ldap shell history.\n")
            }
            other => panic!("unexpected prune output: {other:?}"),
        }

        let remaining = history
            .with(|store| store.list_entries_for(Some("ldap")))
            .unwrap_or_default();
        assert_eq!(remaining.len(), 1);
        assert_eq!(remaining[0].command, "user bob");
        assert_eq!(
            history
                .with(|store| store.list_entries_for(Some("mreg")))
                .unwrap_or_default()
                .len(),
            1
        );

        let clear = run_history_repl_command(
            &mut session,
            HistoryArgs {
                command: HistoryCommands::Clear,
            },
            &history,
        )
        .expect("scoped clear should succeed");
        match clear {
            ReplCommandOutput::Text(text) => assert_eq!(text, "Cleared ldap shell history.\n"),
            other => panic!("unexpected clear output: {other:?}"),
        }
        assert!(
            history
                .with(|store| store.list_entries_for(Some("ldap")))
                .unwrap_or_default()
                .is_empty()
        );
        assert_eq!(
            history
                .with(|store| store.list_entries_for(Some("mreg")))
                .unwrap_or_default()
                .len(),
            1
        );
    }

    fn config_with_entries(entries: &[(&str, &str)]) -> crate::config::ResolvedConfig {
        let mut defaults = ConfigLayer::default();
        defaults.set("repl.history.path", "/tmp/osp-repl-history-tests.jsonl");
        defaults.set("theme.path", Vec::<String>::new());
        for (key, value) in entries {
            defaults.set(*key, *value);
        }
        let mut resolver = ConfigResolver::default();
        resolver.set_defaults(defaults);
        resolver
            .resolve(ResolveOptions::default())
            .expect("config should resolve")
    }

    fn app_state(config: crate::config::ResolvedConfig) -> AppState {
        AppState::new(AppStateInit {
            context: RuntimeContext::new(None, TerminalKind::Repl, None),
            config,
            render_settings: RenderSettings::test_plain(OutputFormat::Json),
            message_verbosity: MessageLevel::Success,
            error_detail: crate::app::ErrorDetail::Terse,
            debug_verbosity: 0,
            plugins: crate::plugin::PluginManager::new(Vec::new())
                .with_bundled_roots(false)
                .with_default_roots(false),
            native_commands: crate::native::NativeCommandRegistry::default(),
            themes: crate::ui::theme_catalog::ThemeCatalog::default(),
            launch: LaunchContext::default(),
        })
    }

    fn shared_history(enabled: bool) -> SharedHistory {
        SharedHistory::new(
            HistoryConfig {
                path: None,
                max_entries: 32,
                enabled,
                dedupe: false,
                profile_scoped: false,
                exclude_patterns: Vec::new(),
                profile: None,
                terminal: None,
                shell_context: crate::repl::HistoryShellContext::default(),
            }
            .normalized(),
        )
    }
}
