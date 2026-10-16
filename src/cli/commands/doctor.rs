//! `doctor` builtin command implementation.
//!
//! `doctor` is a thin aggregation surface over other diagnostics providers.
//! This module pulls together config, plugin, theme, and recent-failure views
//! into one operator-facing troubleshooting entrypoint.

use miette::Result;

use crate::app::CommandOutput;
use crate::app::{AppClients, AppRuntime, AppSession, LastFailure, UiState};
use crate::cli::rows::output::rows_to_output_result;
use crate::cli::{DoctorArgs, DoctorCommands, PluginsArgs, PluginsCommands};
use crate::core::MessageLevel;
use crate::core::command_policy::CredentialState;
use crate::core::document::{DocBlock, Document};
use crate::core::output::OutputFormat;
use crate::core::output_model::OutputResult;
use crate::core::row::Row;
use crate::ui::theme_catalog::ThemeCatalog;
use serde::Serialize;
use serde_json::{Map, Value};

use super::{config as config_cmd, plugins as plugins_cmd};

pub(crate) fn run_doctor_command(
    runtime: &AppRuntime,
    session: &AppSession,
    clients: &AppClients,
    ui: &UiState,
    args: DoctorArgs,
) -> Result<CommandOutput> {
    let command = args.command.unwrap_or(DoctorCommands::All);
    match command {
        DoctorCommands::Config => Ok(CommandOutput::output(
            rows_to_output_result(config_cmd::config_diagnostics_rows(runtime, session, ui)),
            None,
        )),
        DoctorCommands::Plugins => plugins_cmd::run_plugins_command(
            runtime,
            clients,
            PluginsArgs {
                command: PluginsCommands::Doctor,
            },
        ),
        DoctorCommands::Last => Ok(render_last_failure_document(
            ui,
            session.last_failure.as_ref(),
        )),
        DoctorCommands::Theme => Ok(CommandOutput::output(
            rows_to_output_result(theme_doctor_rows(&runtime.themes)),
            None,
        )),
        DoctorCommands::All => run_doctor_all(runtime, session, clients, ui),
    }
}

pub(crate) fn doctor_command() -> clap::Command {
    crate::cli::builtin_command("doctor")
}

#[derive(Serialize)]
struct CredentialDiagnostics<'a> {
    service: &'a str,
    #[serde(flatten)]
    state: &'a CredentialState,
}

impl CredentialDiagnostics<'_> {
    fn label(&self) -> String {
        let state = if self.state.valid { "valid" } else { "invalid" };
        self.state.ttl_seconds.map_or_else(
            || format!("{}: {state}", self.service),
            |ttl| format!("{}: {state} ({ttl}s remaining)", self.service),
        )
    }
}

fn run_doctor_all(
    runtime: &AppRuntime,
    session: &AppSession,
    clients: &AppClients,
    ui: &UiState,
) -> Result<CommandOutput> {
    let facts = runtime.auth.session_facts();
    let credentials = facts
        .credentials
        .iter()
        .map(|(service, state)| CredentialDiagnostics { service, state })
        .collect::<Vec<_>>();
    let mut sections: Vec<(&str, Vec<Row>)> = vec![(
        "session",
        session_doctor_rows(
            &facts,
            runtime.config.resolved().active_profile(),
            &credentials,
        ),
    )];

    sections.push((
        "config",
        config_cmd::config_diagnostics_rows(runtime, session, ui),
    ));

    let report = clients.plugins().doctor();
    sections.push(("plugins", plugins_cmd::doctor_rows(&report)));

    sections.push(("theme", theme_doctor_rows(&runtime.themes)));

    if matches!(ui.render_settings.format, OutputFormat::Json) {
        return Ok(CommandOutput::json(doctor_report_json_value(&sections)));
    }

    sections[0].1[0].insert(
        "credentials".to_string(),
        serde_json::json!(
            credentials
                .iter()
                .map(CredentialDiagnostics::label)
                .collect::<Vec<_>>()
        ),
    );

    Ok(CommandOutput::document_with_output(
        doctor_report_guide(&sections),
        doctor_report_output(&sections),
        None,
    ))
}

fn session_doctor_rows(
    context: &crate::core::command_policy::SessionFacts,
    profile: &str,
    credentials: &[CredentialDiagnostics<'_>],
) -> Vec<Row> {
    let invalid_credentials = context
        .credentials
        .values()
        .filter(|credential| !credential.valid)
        .count();

    let row = crate::row! {
        "status" => if invalid_credentials > 0 {
            "issue"
        } else if context.authenticated {
            "ok"
        } else {
            "not_authenticated"
        },
        "authenticated" => context.authenticated,
        "profile" => profile,
        "credentials" => serde_json::json!(credentials),
    };
    vec![row]
}

fn doctor_report_guide(sections: &[(&str, Vec<Row>)]) -> Document {
    Document {
        blocks: sections
            .iter()
            .map(|(name, rows)| {
                DocBlock::section(
                    *name,
                    vec![DocBlock::Data {
                        value: doctor_section_view_value(rows),
                    }],
                )
            })
            .collect(),
    }
}

fn doctor_report_output(sections: &[(&str, Vec<Row>)]) -> OutputResult {
    OutputResult::from_rows(vec![doctor_report_row(sections, doctor_section_view_value)])
}

fn doctor_report_json_value(sections: &[(&str, Vec<Row>)]) -> Value {
    doctor_report_value(sections, doctor_section_json_value)
}

fn doctor_report_value(
    sections: &[(&str, Vec<Row>)],
    value_for_rows: impl Fn(&[Row]) -> Value,
) -> Value {
    Value::Object(doctor_report_row(sections, value_for_rows))
}

fn doctor_report_row(
    sections: &[(&str, Vec<Row>)],
    value_for_rows: impl Fn(&[Row]) -> Value,
) -> Map<String, Value> {
    sections
        .iter()
        .map(|(name, rows)| ((*name).to_string(), value_for_rows(rows)))
        .collect::<Map<String, Value>>()
}

fn doctor_section_view_value(rows: &[Row]) -> Value {
    match rows {
        [] => Value::Array(Vec::new()),
        [row] => Value::Object(row.clone()),
        _ => Value::Array(rows.iter().cloned().map(Value::Object).collect()),
    }
}

fn doctor_section_json_value(rows: &[Row]) -> Value {
    Value::Array(rows.iter().cloned().map(Value::Object).collect())
}

fn theme_doctor_rows(themes: &ThemeCatalog) -> Vec<Row> {
    let issues = &themes.issues;
    if issues.is_empty() {
        return vec![crate::row! {
            "status" => "ok",
            "issue_count" => 0,
        }];
    }

    let count = issues.len() as i64;
    issues
        .iter()
        .map(|issue| {
            crate::row! {
                "status" => "issue",
                "issue_count" => count,
                "path" => issue.path.display().to_string(),
                "message" => issue.message.clone(),
            }
        })
        .collect()
}

fn render_last_failure_document(
    ui: &UiState,
    last_failure: Option<&LastFailure>,
) -> crate::app::CommandOutput {
    let Some(last) = last_failure else {
        if matches!(ui.render_settings.format, OutputFormat::Json) {
            return CommandOutput::json(serde_json::json!({
                "status": "ok",
                "failure": null,
                "message": "No recorded REPL failure in this session.",
            }));
        }
        return CommandOutput::text("No recorded REPL failure in this session.\n");
    };

    let (summary, action) = split_failure_action(&last.summary);
    if matches!(ui.render_settings.format, OutputFormat::Json) {
        let payload = serde_json::json!({
            "status": "error",
            "command": last.command_line,
            "summary": summary,
            "action": action,
            "detail": last.detail(MessageLevel::Info),
        });
        let mut output = CommandOutput::json(payload);
        if ui.debug_level >= 2
            && let Some(backtrace) = last.backtrace()
        {
            output.stderr_text = format!("{backtrace}\n");
        }
        return output;
    }

    let mut out = String::new();
    out.push_str("Last REPL failure:\n");
    out.push_str(&format!("  Command: {}\n", last.command_line));
    out.push_str(&format!("  Error:   {summary}\n"));
    if let Some(action) = action {
        out.push_str(&format!("  Action:  {action}\n"));
    }
    let detail = last.detail(ui.verbosity);
    if ui.verbosity >= MessageLevel::Info
        && normalized_failure_detail(&detail) != normalized_failure_detail(&last.summary)
    {
        out.push('\n');
        out.push_str("Detail:\n");
        for line in detail.lines() {
            out.push_str("  ");
            out.push_str(line);
            out.push('\n');
        }
    }
    if let Some(command) = next_failure_detail_command(ui.verbosity, last) {
        out.push('\n');
        out.push_str(&format!("More: run `{command}` for more detail.\n"));
    }
    let mut output = CommandOutput::text(out);
    if ui.debug_level >= 2
        && let Some(backtrace) = last.backtrace()
    {
        output.stderr_text = format!("{backtrace}\n");
    }
    output
}

fn split_failure_action(summary: &str) -> (&str, Option<&str>) {
    let Some((summary, action)) = summary.split_once("Try:") else {
        return (summary.trim(), None);
    };
    (summary.trim().trim_end_matches('.'), Some(action.trim()))
}

fn normalized_failure_detail(detail: &str) -> &str {
    split_failure_action(detail).0.trim()
}

fn next_failure_detail_command(current: MessageLevel, last: &LastFailure) -> Option<&'static str> {
    if current < MessageLevel::Info
        && normalized_failure_detail(&last.summary)
            != normalized_failure_detail(&last.detail(MessageLevel::Info))
    {
        Some("doctor last -v")
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::{
        doctor_command, render_last_failure_document, run_doctor_command, theme_doctor_rows,
    };
    use crate::app::{LastFailure, RuntimeContext, TerminalKind, UiState};
    use crate::cli::{DoctorArgs, DoctorCommands};
    use crate::config::{ConfigLayer, ConfigResolver, ResolveOptions, RuntimeLoadOptions};
    use crate::core::MessageLevel;
    use crate::core::document::DocBlock;
    use crate::core::output::OutputFormat;
    use crate::plugin::PluginManager;
    use crate::ui::RenderSettings;
    use crate::ui::theme_catalog::{ThemeCatalog, ThemeLoadIssue};
    use serde_json::Value;
    use std::path::PathBuf;

    fn ui_state(format: OutputFormat, debug_level: u8) -> UiState {
        UiState::new(
            RenderSettings::test_plain(format),
            MessageLevel::Success,
            debug_level,
        )
    }

    fn doctor_context(format: OutputFormat, _builtins: Option<&str>) -> crate::app::AppState {
        let mut defaults = ConfigLayer::default();
        defaults.set("profile.default", "default");
        defaults.set("theme.path", Vec::<String>::new());
        let mut resolver = ConfigResolver::default();
        resolver.set_defaults(defaults);
        let resolved = resolver
            .resolve(ResolveOptions::default().with_terminal("cli"))
            .expect("test config should resolve");
        let mut builder = crate::app::AppStateBuilder::new(
            RuntimeContext::new(None, TerminalKind::Cli, None),
            resolved,
            ui_state(format, 0),
        );
        builder.themes = Some(ThemeCatalog::default());
        builder.launch = crate::app::LaunchContext::default()
            .with_runtime_load(RuntimeLoadOptions::defaults_only());
        builder
            .with_plugins(
                PluginManager::new(Vec::new())
                    .with_bundled_roots(false)
                    .with_default_roots(false),
            )
            .build()
    }

    #[test]
    fn doctor_last_rendering_covers_empty_text_debug_and_json_modes_unit() {
        let empty = render_last_failure_document(&ui_state(OutputFormat::Table, 0), None);
        let crate::app::CommandBody::Text(empty_text) = empty.body else {
            panic!("expected text output");
        };
        assert!(empty_text.contains("No recorded REPL failure"));

        let mut failure = LastFailure::from_report(
            "ldap user nope".to_string(),
            std::sync::Arc::new(
                miette::miette!("missing capability: ldap.user.read")
                    .wrap_err("policy gate: ldap user"),
            ),
            crate::ui::RenderSettings::test_plain(OutputFormat::Table),
        );
        failure.summary = "request failed. Try: authenticate again".to_string();

        let verbose = render_last_failure_document(
            &UiState::new(
                RenderSettings::test_plain(OutputFormat::Table),
                MessageLevel::Info,
                0,
            ),
            Some(&failure),
        );
        let crate::app::CommandBody::Text(verbose_text) = verbose.body else {
            panic!("expected text output");
        };
        assert!(verbose_text.contains("Command: ldap user nope"));
        assert!(verbose_text.contains("Error:   request failed"));
        assert!(verbose_text.contains("Detail:"));
        assert!(verbose_text.contains("missing capability: ldap.user.read"));

        let compact =
            render_last_failure_document(&ui_state(OutputFormat::Table, 0), Some(&failure));
        let crate::app::CommandBody::Text(compact_text) = compact.body else {
            panic!("expected text output");
        };
        assert!(compact_text.contains("Error:   request failed"));
        assert!(compact_text.contains("Action:  authenticate again"));
        assert!(!compact_text.contains("Detail:"));
        assert!(compact_text.contains("doctor last -v"));

        let debug = render_last_failure_document(
            &UiState::new(
                RenderSettings::test_plain(OutputFormat::Table),
                MessageLevel::Trace,
                0,
            ),
            Some(&failure),
        );
        let crate::app::CommandBody::Text(debug_text) = debug.body else {
            panic!("expected text output");
        };
        assert!(debug_text.contains("policy gate: ldap user"));
        assert!(!debug_text.contains("Stack backtrace"));

        let forensic =
            render_last_failure_document(&ui_state(OutputFormat::Table, 3), Some(&failure));
        assert!(forensic.stderr_text.contains("Stack backtrace"));
        let crate::app::CommandBody::Text(forensic_text) = forensic.body else {
            panic!("expected text output");
        };
        assert!(!forensic_text.contains("Stack backtrace"));

        let json_failure = LastFailure::new(
            "plugins refresh".to_string(),
            "plugin failed".to_string(),
            "plugin failed".to_string(),
        );
        let json_result =
            render_last_failure_document(&ui_state(OutputFormat::Json, 0), Some(&json_failure));
        let crate::app::CommandBody::Json(json) = json_result.body else {
            panic!("expected json output");
        };
        assert_eq!(json["status"], Value::String("error".to_string()));
        assert_eq!(
            json["command"],
            Value::String("plugins refresh".to_string())
        );

        let empty = theme_doctor_rows(&ThemeCatalog::default());
        assert_eq!(
            empty,
            vec![crate::row! { "status" => "ok", "issue_count" => 0 }]
        );

        let catalog = ThemeCatalog {
            entries: Default::default(),
            issues: vec![ThemeLoadIssue {
                path: PathBuf::from("/tmp/theme.toml"),
                message: "broken palette".to_string(),
            }],
        };

        let rows = theme_doctor_rows(&catalog);
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0]["status"], Value::String("issue".to_string()));
        assert_eq!(rows[0]["issue_count"], Value::from(1));
        assert_eq!(
            rows[0]["message"],
            Value::String("broken palette".to_string())
        );
    }

    #[test]
    fn doctor_commands_respect_visibility_output_shapes_and_subcommand_defs_unit() {
        let state = doctor_context(OutputFormat::Json, Some("config,plugins,theme"));
        let visible = run_doctor_command(
            &state.runtime,
            &state.session,
            &state.clients,
            &state.runtime.ui,
            DoctorArgs {
                command: Some(DoctorCommands::All),
            },
        )
        .expect("doctor all should succeed");
        let crate::app::CommandBody::Json(json) = visible.body else {
            panic!("expected json output");
        };
        assert!(json.get("config").is_some());
        assert!(json.get("plugins").is_some());
        assert!(json.get("theme").is_some());

        let state = doctor_context(OutputFormat::Table, Some("theme"));
        let theme = run_doctor_command(
            &state.runtime,
            &state.session,
            &state.clients,
            &state.runtime.ui,
            DoctorArgs {
                command: Some(DoctorCommands::Theme),
            },
        )
        .expect("doctor theme should succeed");
        assert!(!matches!(theme.body, crate::app::CommandBody::None));
        assert!(theme.messages.is_empty());

        let table = run_doctor_command(
            &state.runtime,
            &state.session,
            &state.clients,
            &state.runtime.ui,
            DoctorArgs {
                command: Some(DoctorCommands::All),
            },
        )
        .expect("doctor all should succeed");
        let crate::app::CommandBody::Output(guide) = table.body else {
            panic!("expected guide output");
        };
        assert!(
            guide
                .document
                .as_ref()
                .and_then(crate::core::output_model::OutputDocument::semantic)
                .expect("expected semantic guide payload")
                .blocks
                .iter()
                .all(|block| matches!(block, DocBlock::Section { blocks, .. }
                    if matches!(blocks.as_slice(), [DocBlock::Data { .. }])))
        );
        let rows = guide
            .as_rows()
            .expect("doctor guide output should keep a structured fallback");
        assert!(rows[0].get("config").is_some());
        assert!(rows[0].get("theme").is_some());

        let def = doctor_command();
        let names = def
            .get_subcommands()
            .map(|child| child.get_name())
            .collect::<Vec<_>>();

        assert_eq!(def.get_name(), "doctor");
        assert_eq!(names, vec!["all", "config", "last", "plugins", "theme"]);
    }
}
