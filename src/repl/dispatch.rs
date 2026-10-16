//! REPL line dispatch and shell-scope control flow.
//!
//! This module exists to turn an accepted REPL line into the right next action:
//! builtin handling, shell-scope transitions, ordinary command execution, or
//! inline help/DSL affordances.
//!
//! High-level flow:
//!
//! - classify builtins and bang requests before normal command parsing
//! - parse the REPL line into command and stage components
//! - handle shell shortcuts and scoped help before normal dispatch
//! - run the resolved command and package the next REPL action/result
//!
//! Contract:
//!
//! - this is where REPL-specific dispatch semantics live
//! - editor/menu behavior belongs in `repl::engine`
//! - generic non-interactive command execution should not drift into this layer

mod builtins;
mod command;
mod shell;

use crate::repl::{ReplResult, SharedHistory};
use miette::Result;
use std::time::Instant;

use crate::app::sink::{StdIoUiSink, UiSink};
use crate::app::{AppClients, AppRuntime, AppSession};
use crate::app::{ResolvedInvocation, render_report_message, resolve_invocation_ui};
use crate::cli::invocation::scan_command_tokens;
use crate::ui::messages::MessageLevel;

use super::{ReplViewContext, completion, input};

use builtins::{ReplBuiltin, execute_repl_builtin, is_repl_bang_request, parse_repl_builtin};
use command::{ParsedReplDispatch, execute_repl_command_dispatch, parse_repl_invocation};
use shell::{ReplShortcutPlan, classify_repl_shortcut, execute_repl_shortcut};

#[cfg(test)]
pub(crate) use crate::app::command_plan::repl_command_spec;
#[cfg(test)]
use builtins::{BangCommand, current_history_scope, execute_bang_command, parse_bang_command};
#[cfg(test)]
use command::{
    command_side_effects, config_key_change_requires_intro, finalize_repl_command, parse_clap_help,
    render_repl_command_output, run_repl_command,
};
#[cfg(test)]
pub(crate) use shell::apply_repl_shell_prefix;
#[cfg(test)]
pub(crate) use shell::leave_repl_shell;
#[cfg(test)]
use shell::{handle_repl_exit_request, repl_help_for_scope};

enum ReplLinePlan {
    Builtin {
        raw: String,
        builtin: ReplBuiltin,
    },
    Blank,
    DslHelp {
        help: String,
    },
    Shortcut {
        parsed: input::ReplParsedLine,
        shortcut: Box<ReplShortcutPlan>,
    },
    Command(Box<ParsedReplDispatch>),
}

#[derive(Debug, Clone, Copy)]
enum ReplTimingPlan {
    Flat {
        debug_verbosity: u8,
    },
    ParseOnly {
        debug_verbosity: u8,
    },
    Invocation {
        debug_verbosity: u8,
        parse_finished: Instant,
        execute_finished: Instant,
    },
}

pub(crate) struct ExecutedReplLine {
    pub(crate) result: ReplResult,
    exit_code: i32,
    timing: ReplTimingPlan,
}

impl ExecutedReplLine {
    fn flat(result: ReplResult, debug_verbosity: u8) -> Self {
        let exit_code = match result {
            ReplResult::Exit(code) => code,
            _ => 0,
        };
        Self {
            result,
            exit_code,
            timing: ReplTimingPlan::Flat { debug_verbosity },
        }
    }

    fn parse_only(result: ReplResult, debug_verbosity: u8) -> Self {
        Self {
            result,
            exit_code: 0,
            timing: ReplTimingPlan::ParseOnly { debug_verbosity },
        }
    }

    fn invocation(
        result: ReplResult,
        exit_code: i32,
        debug_verbosity: u8,
        parse_finished: Instant,
        execute_finished: Instant,
    ) -> Self {
        Self {
            result,
            exit_code,
            timing: ReplTimingPlan::Invocation {
                debug_verbosity,
                parse_finished,
                execute_finished,
            },
        }
    }
}

struct ReplExecutionContext<'a, 'sink> {
    runtime: &'a mut AppRuntime,
    session: &'a mut AppSession,
    clients: &'a AppClients,
    history: &'a SharedHistory,
    sink: &'sink mut dyn UiSink,
    accepted: &'a mut bool,
}

#[cfg(test)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ReplLinePlanKind {
    Builtin,
    Blank,
    DslHelp,
    Shortcut,
    Help,
    Invocation,
}

impl ReplLinePlan {
    #[cfg(test)]
    fn kind(&self) -> ReplLinePlanKind {
        match self {
            ReplLinePlan::Builtin { .. } => ReplLinePlanKind::Builtin,
            ReplLinePlan::Blank => ReplLinePlanKind::Blank,
            ReplLinePlan::DslHelp { .. } => ReplLinePlanKind::DslHelp,
            ReplLinePlan::Shortcut { .. } => ReplLinePlanKind::Shortcut,
            ReplLinePlan::Command(dispatch) => match dispatch.as_ref() {
                ParsedReplDispatch::Help { .. } => ReplLinePlanKind::Help,
                ParsedReplDispatch::Invocation(_) => ReplLinePlanKind::Invocation,
            },
        }
    }
}

pub(crate) fn execute_repl_plugin_line(
    runtime: &mut AppRuntime,
    session: &mut AppSession,
    clients: &AppClients,
    history: &SharedHistory,
    line: &str,
    sink: Option<&mut dyn UiSink>,
) -> Result<ExecutedReplLine> {
    crate::app::facts::set_command_scope(runtime, &session.scope);
    let mut stdio = StdIoUiSink;
    let sink = sink.unwrap_or(&mut stdio);
    let mut accepted = false;
    let started = Instant::now();
    let mut resolved = line.to_string();
    let mut keep_history = false;
    let execution = (|| {
        if let Some(command @ builtins::BangCommand::Last) = builtins::parse_bang_command(line)? {
            match builtins::execute_bang_command(session, history, line, command)? {
                ReplResult::ReplaceInput(expanded) => {
                    sink.write_stderr(&format!("{expanded}\n"));
                    resolved = expanded;
                }
                result => return Ok(ExecutedReplLine::flat(result, runtime.ui.debug_verbosity)),
            }
        }
        // Alias expansion needs the live execution config, which the generic
        // history backend does not own. Classify before a command can edit it.
        keep_history = input::ReplParsedLine::parse(&resolved, runtime.config.resolved())
            .is_ok_and(|parsed| {
                !super::history_store::is_sensitive_config_set_tokens(&parsed.dispatch_tokens)
            });
        if !matches!(resolved.trim(), "next" | "prev") {
            session.native_context.clear_pagination();
        }
        let plan = classify_repl_line(runtime, session, &resolved)?;
        let parse_finished = Instant::now();
        execute_repl_line_plan(
            ReplExecutionContext {
                runtime,
                session,
                clients,
                history,
                sink,
                accepted: &mut accepted,
            },
            &resolved,
            plan,
            parse_finished,
        )
    })();
    if let Err(err) = history
        .with(|store| store.finalize_execution(line, &resolved, accepted && keep_history))
        .map_err(anyhow::Error::msg)
        .and_then(|result| result)
    {
        sink.write_stderr(&format!("failed to update REPL history: {err}\n"));
    }
    if accepted && !is_repl_bang_request(&resolved) {
        let _ = history.with(|store| store.remember_command(&resolved));
    }
    session.finish_repl_line();
    match execution {
        Ok(executed) => {
            let debug = match executed.timing {
                ReplTimingPlan::Flat { debug_verbosity }
                | ReplTimingPlan::ParseOnly { debug_verbosity }
                | ReplTimingPlan::Invocation {
                    debug_verbosity, ..
                } => debug_verbosity,
            };
            let _logging = crate::app::logging::override_stderr_level(debug);
            tracing::info!(target: "osp", exit_code = executed.exit_code,
                elapsed_ms = started.elapsed().as_millis(), "finished");
            record_repl_timing(session, started, executed.timing);
            Ok(executed)
        }
        Err(err) => {
            let fallback_ui = input::ReplParsedLine::parse(&resolved, runtime.config.resolved())
                .ok()
                .and_then(|parsed| scan_command_tokens(&parsed.dispatch_tokens).ok())
                .map(|scanned| {
                    resolve_invocation_ui(
                        runtime.config.resolved(),
                        &runtime.ui,
                        &scanned.invocation,
                    )
                    .ui
                })
                .unwrap_or_else(|| runtime.ui.clone());
            let settings = crate::app::host::error_display_settings(&err)
                .cloned()
                .unwrap_or_else(|| crate::app::host::ErrorDisplaySettings::from(&fallback_ui));
            let err = std::sync::Arc::new(crate::app::host::report_with_invocation(
                err,
                settings.clone(),
            ));
            let ui = &settings;
            if ui.debug_verbosity > 0 {
                session.record_prompt_timing(ui.debug_verbosity, started.elapsed(), None, None, None);
            }
            let _logging = crate::app::logging::override_stderr_level(ui.debug_verbosity);
            let plain_settings = ui.render_settings.plain_copy_settings();
            let remember = !is_repl_bang_request(&resolved)
                && !is_doctor_request(&resolved, runtime.config.resolved());
            let more = remember
                && crate::app::host::has_report_detail(&err, ui.message_verbosity, &plain_settings);
            if remember {
                session.record_failure_report(&resolved, err.clone(), plain_settings);
            }
            let rendered = crate::app::render_process_error(
                &err,
                &ui.render_settings,
                ui.message_verbosity,
                more,
            );
            if ui.render_settings.format == crate::core::output::OutputFormat::Json {
                sink.write_stdout(&rendered);
                if ui.message_verbosity >= MessageLevel::Info {
                    sink.write_stderr(&format!(
                        "{}\n",
                        render_report_message(&err, ui.message_verbosity, &ui.render_settings)
                            .trim_end()
                    ));
                }
            } else {
                sink.write_stderr(&rendered);
            }
            if ui.debug_verbosity >= 2
                && let Some(backtrace) =
                    crate::app::host::render_backtrace(&err, &ui.render_settings)
            {
                sink.write_stderr(&format!("{backtrace}\n"));
            }
            let exit_code = crate::app::classify_exit_code(&err);
            tracing::info!(target: "osp", exit_code, elapsed_ms = started.elapsed().as_millis(), "finished");
            let mut executed = ExecutedReplLine::flat(ReplResult::Continue, ui.debug_verbosity);
            executed.exit_code = exit_code;
            Ok(executed)
        }
    }
}

fn is_doctor_request(line: &str, config: &crate::config::ResolvedConfig) -> bool {
    input::ReplParsedLine::parse(line, config)
        .ok()
        .and_then(|parsed| scan_command_tokens(&parsed.dispatch_tokens).ok())
        .and_then(|scanned| scanned.tokens.into_iter().next())
        .is_some_and(|command| command == crate::app::CMD_DOCTOR)
}

fn classify_repl_line(
    runtime: &AppRuntime,
    session: &AppSession,
    line: &str,
) -> Result<ReplLinePlan> {
    // 1. Classify builtins and blank lines before any normal parsing.
    let raw = line.trim();
    if let Some(plan) = classify_builtin_or_blank(raw)? {
        return Ok(plan);
    }

    // 2. Parse the line into REPL-local command/stage shape.
    let parsed = parse_repl_dispatch_line(runtime, line)?;

    // 3. Handle REPL-local affordances before normal command dispatch.
    if let Some(plan) = classify_repl_local_plan(runtime, session, &parsed)? {
        return Ok(plan);
    }

    // Invocation flags and aliases may precede last, which remains global even
    // inside a product shell. Its cached output uses normal parsing/rendering.
    if scan_command_tokens(&parsed.dispatch_tokens)?
        .tokens
        .first()
        .map(String::as_str)
        == Some("last")
    {
        return Ok(ReplLinePlan::Builtin {
            raw: line.to_string(),
            builtin: ReplBuiltin::Last,
        });
    }

    // 4. Resolve the remaining line as ordinary command/help dispatch.
    classify_repl_command_plan(runtime, session, parsed)
}

fn classify_builtin_or_blank(raw: &str) -> Result<Option<ReplLinePlan>> {
    // Builtins and bang expansion must run before full line parsing so REPL
    // control verbs keep working even when the rest of the line is not a valid
    // command invocation.
    if let Some(builtin) = parse_repl_builtin(raw)? {
        return Ok(Some(ReplLinePlan::Builtin {
            raw: raw.to_string(),
            builtin,
        }));
    }

    if raw.is_empty() {
        return Ok(Some(ReplLinePlan::Blank));
    }

    Ok(None)
}

fn parse_repl_dispatch_line(runtime: &AppRuntime, line: &str) -> Result<input::ReplParsedLine> {
    input::ReplParsedLine::parse(line, runtime.config.resolved())
}

fn classify_repl_local_plan(
    runtime: &AppRuntime,
    session: &AppSession,
    parsed: &input::ReplParsedLine,
) -> Result<Option<ReplLinePlan>> {
    // DSL help is a REPL-local affordance over raw stage text, so surface it
    // before command dispatch rather than forcing it through normal execution.
    if let Some(help) = completion::maybe_render_dsl_help(
        ReplViewContext::from_parts(runtime, session),
        &parsed.stages,
    ) {
        return Ok(Some(ReplLinePlan::DslHelp { help }));
    }

    let base_invocation = base_repl_invocation(runtime);
    if let Some(shortcut) = classify_repl_shortcut(runtime, session, parsed, &base_invocation)? {
        return Ok(Some(ReplLinePlan::Shortcut {
            parsed: parsed.clone(),
            shortcut: Box::new(shortcut),
        }));
    }

    Ok(None)
}

fn classify_repl_command_plan(
    runtime: &AppRuntime,
    session: &AppSession,
    parsed: input::ReplParsedLine,
) -> Result<ReplLinePlan> {
    Ok(ReplLinePlan::Command(Box::new(parse_repl_invocation(
        runtime, session, &parsed,
    )?)))
}

fn execute_repl_line_plan(
    context: ReplExecutionContext<'_, '_>,
    line: &str,
    plan: ReplLinePlan,
    parse_finished: Instant,
) -> Result<ExecutedReplLine> {
    let ReplExecutionContext {
        runtime,
        session,
        clients,
        history,
        sink,
        accepted,
    } = context;
    match plan {
        ReplLinePlan::Builtin { raw, builtin } => {
            *accepted = true;
            let debug_level = input::ReplParsedLine::parse(&raw, runtime.config.resolved())
                .ok()
                .and_then(|parsed| scan_command_tokens(&parsed.dispatch_tokens).ok())
                .map(|scanned| {
                    resolve_invocation_ui(
                        runtime.config.resolved(),
                        &runtime.ui,
                        &scanned.invocation,
                    )
                    .ui
                    .debug_verbosity
                })
                .unwrap_or(runtime.ui.debug_verbosity);
            let _logging = crate::app::logging::override_stderr_level(debug_level);
            let result =
                execute_repl_builtin(runtime, session, clients, history, &raw, builtin, sink)?;
            Ok(ExecutedReplLine::flat(result, debug_level))
        }
        ReplLinePlan::Blank => Ok(ExecutedReplLine::flat(
            ReplResult::Continue,
            runtime.ui.debug_verbosity,
        )),
        ReplLinePlan::DslHelp { help } => {
            *accepted = true;
            Ok(ExecutedReplLine::flat(
                ReplResult::Print(help),
                runtime.ui.debug_verbosity,
            ))
        }
        ReplLinePlan::Shortcut { parsed, shortcut } => {
            *accepted = true;
            let invocation = match shortcut.as_ref() {
                ReplShortcutPlan::Help { invocation }
                | ReplShortcutPlan::CurrentShellHelp { invocation, .. }
                | ReplShortcutPlan::ShellEntry { invocation, .. } => invocation,
            };
            let debug_level = invocation.ui.debug_verbosity;
            let error_display = crate::app::host::ErrorDisplaySettings::from(&invocation.ui);
            let _logging = crate::app::logging::override_stderr_level(debug_level);
            let result =
                execute_repl_shortcut(runtime, session, clients, &parsed, *shortcut, line, sink)
                    .map_err(|err| crate::app::host::report_with_invocation(err, error_display))?;
            Ok(ExecutedReplLine::flat(result, debug_level))
        }
        ReplLinePlan::Command(dispatch) => {
            let ui = match dispatch.as_ref() {
                ParsedReplDispatch::Help { effective, .. } => &effective.ui,
                ParsedReplDispatch::Invocation(invocation) => &invocation.effective.ui,
            };
            let _logging = crate::app::logging::override_stderr_level(ui.debug_verbosity);
            let error_display = crate::app::host::ErrorDisplaySettings::from(ui);
            execute_repl_command_dispatch(
                ReplExecutionContext {
                    runtime,
                    session,
                    clients,
                    history,
                    sink,
                    accepted,
                },
                line,
                *dispatch,
                parse_finished,
            )
            .map_err(|err| crate::app::host::report_with_invocation(err, error_display))
        }
    }
}

fn record_repl_timing(session: &AppSession, started: Instant, timing: ReplTimingPlan) {
    let finished = Instant::now();
    match timing {
        ReplTimingPlan::Flat { debug_verbosity } => {
            session.record_prompt_timing(
                debug_verbosity,
                finished.saturating_duration_since(started),
                None,
                None,
                None,
            );
        }
        ReplTimingPlan::ParseOnly { debug_verbosity } => {
            session.record_prompt_timing(
                debug_verbosity,
                finished.saturating_duration_since(started),
                Some(finished.saturating_duration_since(started)),
                None,
                None,
            );
        }
        ReplTimingPlan::Invocation {
            debug_verbosity,
            parse_finished,
            execute_finished,
        } => {
            session.record_prompt_timing(
                debug_verbosity,
                finished.saturating_duration_since(started),
                Some(parse_finished.saturating_duration_since(started)),
                Some(execute_finished.saturating_duration_since(parse_finished)),
                Some(finished.saturating_duration_since(execute_finished)),
            );
        }
    }
}

fn base_repl_invocation(runtime: &AppRuntime) -> ResolvedInvocation {
    // Shortcut/help rendering starts from the ambient REPL UI state, not from
    // per-command flags, because no concrete command has been resolved yet.
    resolve_invocation_ui(runtime.config.resolved(), &runtime.ui, &Default::default())
}

#[cfg(test)]
pub(crate) fn classify_repl_line_kind(
    runtime: &AppRuntime,
    session: &AppSession,
    line: &str,
) -> Result<ReplLinePlanKind> {
    Ok(classify_repl_line(runtime, session, line)?.kind())
}

#[cfg(test)]
mod tests;
