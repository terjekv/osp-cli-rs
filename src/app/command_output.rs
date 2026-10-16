//! One command output contract shared by CLI, REPL, progress and replay.

use crate::config::ResolvedConfig;
use crate::core::document::Document;
use crate::core::output::OutputFormat;
use crate::core::output_model::{OutputDocument, OutputItems, OutputResult, RenderRecommendation};
use crate::core::plugin::{ResponseErrorV1, ResponseMessageV1, ResponseMetaV1, ResponseV1};
use crate::dsl::apply_output_pipeline;
use crate::ui::clipboard::ClipboardService;
use crate::ui::messages::{MessageBuffer, MessageLevel};
use crate::ui::{RenderRequest, RenderSettings, copy_output_to_clipboard};
use miette::{Result, WrapErr, miette};
use serde_json::Value;
use terminal_size::{Height, terminal_size};

use crate::app::resolve_render_settings_with_hint;
use crate::app::sink::UiSink;
use crate::app::{AppSession, UiState};
use crate::cli::rows::output::{plugin_data_to_output_result, rows_to_output_result};

/// Transient human presentation belongs to the invocation, not canonical data.
#[derive(Debug, Default, Clone)]
pub(crate) struct CommandPresentation {
    pub(crate) lines: Vec<String>,
    pub(crate) progress_append: Vec<String>,
    pub(crate) progress_replace: bool,
}

/// The mutually exclusive data returned by a command.
#[derive(Debug, Clone)]
pub(crate) enum CommandBody {
    None,
    Output(Box<OutputResult>),
    Text(String),
    Json(Value),
}

impl CommandBody {
    fn into_output(self) -> Option<OutputResult> {
        match self {
            Self::None => None,
            Self::Output(output) => Some(*output),
            Self::Text(text) => Some(text_output_to_rows(&text)),
            Self::Json(value) => Some(OutputResult::from_rows(
                crate::core::output_model::rows_from_value(value),
            )),
        }
    }

    fn json_value(&self) -> Value {
        match self {
            Self::None => Value::Null,
            Self::Output(output) => crate::ui::json_value(output),
            Self::Text(text) => serde_json::from_str(text).unwrap_or(Value::Null),
            Self::Json(value) => value.clone(),
        }
    }
}

#[derive(Debug)]
pub(crate) struct CommandOutput {
    pub(crate) exit_code: i32,
    pub(crate) messages: MessageBuffer,
    pub(crate) body: CommandBody,
    pub(crate) format_hint: Option<OutputFormat>,
    pub(crate) presentation: CommandPresentation,
    /// Passthrough stderr emitted alongside the body, without UI formatting.
    pub(crate) stderr_text: String,
    pub(crate) failure_report: Option<miette::Report>,
}

impl CommandOutput {
    pub(crate) fn exit(exit_code: i32) -> Self {
        Self {
            exit_code,
            messages: MessageBuffer::default(),
            body: CommandBody::None,
            format_hint: None,
            presentation: CommandPresentation::default(),
            stderr_text: String::new(),
            failure_report: None,
        }
    }

    pub(crate) fn output(output: OutputResult, format_hint: Option<OutputFormat>) -> Self {
        Self {
            body: CommandBody::Output(Box::new(output)),
            format_hint,
            ..Self::exit(0)
        }
    }

    pub(crate) fn text(text: impl Into<String>) -> Self {
        Self {
            body: CommandBody::Text(text.into()),
            format_hint: Some(OutputFormat::Value),
            ..Self::exit(0)
        }
    }

    pub(crate) fn json(payload: Value) -> Self {
        Self {
            body: CommandBody::Json(payload),
            format_hint: Some(OutputFormat::Value),
            ..Self::exit(0)
        }
    }

    pub(crate) fn document(document: Document) -> Self {
        Self::output(document.to_output_result(), None)
    }

    pub(crate) fn document_with_output(
        document: Document,
        mut output: OutputResult,
        format_hint: Option<OutputFormat>,
    ) -> Self {
        output
            .meta
            .render_recommendation
            .get_or_insert(RenderRecommendation::Document);
        output.document = Some(OutputDocument::Semantic(document));
        Self::output(output, format_hint)
    }

    pub(crate) fn from_data(
        data: Value,
        meta: &ResponseMetaV1,
        messages: &[ResponseMessageV1],
        stages: &[String],
    ) -> Self {
        let mut result = Self::output(
            plugin_data_to_output_result(data, Some(meta)),
            meta.format_hint.as_deref().and_then(OutputFormat::parse),
        );
        for message in messages {
            result.messages.push(message.level, message.text.clone());
        }
        if let Some(rest) = meta.partial_rows.as_deref().filter(|_| !stages.is_empty()) {
            result.messages.warning(format!(
                "Pipe stages ran over this page only, so counts and groups are partial; {rest}"
            ));
        }
        result.presentation = CommandPresentation {
            lines: meta.presentation_lines.clone(),
            progress_append: meta.progress_append.clone(),
            progress_replace: meta.progress_replace,
        };
        result
    }

    /// Retains successful command data for replay without diagnostic ownership.
    fn saved(&self) -> Self {
        Self {
            body: self.body.clone(),
            format_hint: self.format_hint,
            presentation: self.presentation.clone(),
            ..Self::exit(0)
        }
    }

    pub(crate) fn apply_stages(&mut self, stages: &[String]) -> Result<()> {
        if stages.is_empty() {
            return Ok(());
        }
        let Some(output) = std::mem::replace(&mut self.body, CommandBody::None).into_output()
        else {
            return Ok(());
        };
        let (output, format_hint) = apply_output_stages(output, stages, self.format_hint)?;
        self.body = CommandBody::Output(Box::new(output));
        self.format_hint = format_hint;
        self.presentation = CommandPresentation::default();
        Ok(())
    }

    fn render(&self, config: &ResolvedConfig, settings: &RenderSettings) -> String {
        let mut effective = resolve_render_settings_with_hint(settings, self.format_hint);
        let output = match &self.body {
            CommandBody::None => return String::new(),
            CommandBody::Text(text) => return text.clone(),
            CommandBody::Output(output) => std::borrow::Cow::Borrowed(output.as_ref()),
            CommandBody::Json(payload) => {
                effective.format = OutputFormat::Json;
                effective.format_explicit = true;
                std::borrow::Cow::Owned(
                    OutputResult::from_rows(Vec::new())
                        .with_document(OutputDocument::Json(payload.clone())),
                )
            }
        };
        crate::ui::render(
            &output,
            &RenderRequest {
                layout: Some(crate::ui::help_layout_from_config(config)),
                presentation_lines: &self.presentation.lines,
                ..RenderRequest::new(&effective)
            },
        )
    }
}

#[derive(Debug)]
pub(crate) struct CommandBackendDetails {
    pub(super) error: ResponseErrorV1,
    source: miette::Report,
    pub(super) backtrace: std::backtrace::Backtrace,
}

impl std::fmt::Display for CommandBackendDetails {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        std::fmt::Display::fmt(&self.source, f)
    }
}

impl std::error::Error for CommandBackendDetails {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        Some(self.source.as_ref())
    }
}

impl miette::Diagnostic for CommandBackendDetails {}

pub(crate) fn json_failure_error(
    report: Option<&miette::Report>,
    code: &str,
    message: String,
) -> Value {
    if let Some(backend) =
        report.and_then(super::host::find_error_in_chain::<CommandBackendDetails>)
    {
        serde_json::json!(backend.error)
    } else {
        serde_json::json!({"code": code, "message": message, "details": null})
    }
}

pub(crate) fn render_json_failure(
    error: Value,
    exit_code: i32,
    messages: Vec<Value>,
    data: Value,
) -> String {
    let payload = serde_json::json!({
        "ok": false,
        "error": error,
        "exit_code": exit_code,
        "messages": messages,
        "data": data,
    });
    format!(
        "{}\n",
        serde_json::to_string_pretty(&payload).unwrap_or_else(|_| payload.to_string())
    )
}

pub(crate) fn run_cli_command(
    config: &ResolvedConfig,
    ui: &UiState,
    result: CommandOutput,
    sink: &mut dyn UiSink,
) -> Result<i32> {
    if ui.render_settings.format == OutputFormat::Json && result.exit_code != 0 {
        let data = result.body.json_value();
        let messages = result.messages.entries().iter()
            .map(|entry| serde_json::json!({"level": entry.level.as_str(), "text": entry.text.as_str()}))
            .collect();
        let message = result
            .messages
            .entries()
            .iter()
            .find(|entry| entry.level == MessageLevel::Error)
            .map(|entry| entry.text.clone())
            .or_else(|| result.failure_report.as_ref().map(ToString::to_string))
            .unwrap_or_else(|| "command failed".to_string());
        let error = json_failure_error(result.failure_report.as_ref(), "command_failed", message);
        sink.write_stdout(&render_json_failure(
            error,
            result.exit_code,
            messages,
            data,
        ));
        if let Some(report) = &result.failure_report {
            if ui.message_verbosity >= MessageLevel::Info {
                sink.write_stderr(&format!(
                    "{}\n",
                    super::host::render_report_message(
                        report,
                        ui.message_verbosity,
                        &ui.render_settings
                    )
                    .trim_end()
                ));
            }
            if ui.debug_verbosity >= 2
                && let Some(backtrace) = super::host::render_backtrace(report, &ui.render_settings)
            {
                sink.write_stderr(&format!("{backtrace}\n"));
            }
        }
        return Ok(result.exit_code);
    }
    if !result.messages.is_empty() {
        emit_messages_for_ui(config, ui, &result.messages, ui.message_verbosity, sink);
    }
    if let Some(report) = &result.failure_report
        && ui.message_verbosity >= MessageLevel::Info
    {
        let detail =
            super::host::render_report_message(report, ui.message_verbosity, &ui.render_settings);
        let summary =
            super::host::render_report_message(report, MessageLevel::Success, &ui.render_settings);
        let extra = detail
            .strip_prefix(summary.trim())
            .unwrap_or(&detail)
            .trim();
        if !extra.is_empty() {
            sink.write_stderr(&format!("{extra}\n"));
        }
    }
    let show_failure_data = ui.message_verbosity >= MessageLevel::Info
        && result.failure_report.as_ref().is_some_and(|report| {
            let empty = match &result.body {
                CommandBody::None => true,
                CommandBody::Text(text) => text.trim().is_empty(),
                CommandBody::Output(output) => progress_output_is_empty(output),
                CommandBody::Json(value) => {
                    value.is_null()
                        || value.as_array().is_some_and(Vec::is_empty)
                        || value.as_object().is_some_and(serde_json::Map::is_empty)
                }
            };
            if empty {
                return false;
            }
            let data = result.body.json_value();
            !super::host::find_error_in_chain::<CommandBackendDetails>(report).is_some_and(
                |backend| {
                    backend.error.details == data
                        || backend
                            .error
                            .details
                            .as_object()
                            .is_some_and(|details| details.values().any(|value| value == &data))
                },
            )
        });
    if result.failure_report.is_none() || show_failure_data {
        let rendered = result.render(config, &ui.render_settings);
        if result.failure_report.is_some()
            || result
                .messages
                .entries()
                .iter()
                .any(|entry| entry.level == MessageLevel::Error)
        {
            sink.write_stderr(&rendered);
        } else {
            if let Some(pager) = pager_for_output(config, ui, &result, &rendered) {
                sink.write_stdout_paged(&rendered, &pager);
            } else {
                sink.write_stdout(&rendered);
            }
            if let CommandBody::Output(output) = &result.body {
                maybe_copy_output(config, ui, output, sink);
            }
        }
    }
    if ui.debug_verbosity >= 2
        && let Some(report) = &result.failure_report
        && let Some(backtrace) = super::host::render_backtrace(report, &ui.render_settings)
    {
        sink.write_stderr(&format!("{backtrace}\n"));
    }
    if !result.stderr_text.is_empty() {
        sink.write_stderr(&result.stderr_text);
    }
    Ok(result.exit_code)
}

/// Renders a progress event on stderr; transformed empty results are silent.
pub(crate) fn run_progress_command_with_ui(
    config: &ResolvedConfig,
    ui: &UiState,
    result: CommandOutput,
    sink: &mut dyn UiSink,
) -> Result<()> {
    let CommandBody::Output(output) = &result.body else {
        return Ok(());
    };
    let json_output = resolve_render_settings_with_hint(&ui.render_settings, result.format_hint)
        .format
        == OutputFormat::Json;
    let framed = result.presentation.progress_replace
        && !json_output
        && sink.stderr_is_terminal()
        && !progress_output_is_empty(output);
    if !result.messages.is_empty() && !framed {
        emit_messages_for_ui(config, ui, &result.messages, ui.message_verbosity, sink);
    }
    if progress_output_is_empty(output) {
        return Ok(());
    }
    let append_lines = !sink.stderr_is_terminal() || json_output;
    let mut settings = ui.render_settings.clone();
    if let Some(width) = sink.stderr_width() {
        settings.width = Some(width);
        settings.runtime.width = Some(width);
    }
    let mut rendered = if append_lines && !result.presentation.progress_append.is_empty() {
        plain_progress_settings(&mut settings);
        crate::ui::render(
            output,
            &RenderRequest {
                presentation_lines: &result.presentation.progress_append,
                ..RenderRequest::new(&settings)
            },
        )
    } else if !json_output && sink.stderr_is_terminal() {
        result.render(config, &settings)
    } else {
        plain_progress_settings(&mut settings);
        crate::ui::render(
            output,
            &RenderRequest {
                layout: Some(crate::ui::help_layout_from_config(config)),
                presentation_lines: &result.presentation.lines,
                ..RenderRequest::new(&settings)
            },
        )
    };
    if framed && !result.messages.is_empty() {
        rendered.insert_str(
            0,
            &crate::ui::render_messages(&result.messages, &{
                let mut request =
                    crate::ui::MessageRequest::new(&ui.render_settings, ui.message_verbosity);
                request.layout = crate::ui::messages::message_layout_from_config(config);
                request
            }),
        );
    }
    if !rendered.is_empty() {
        sink.write_progress(
            &rendered,
            result.presentation.progress_replace && !json_output,
        );
    }
    Ok(())
}

fn plain_progress_settings(settings: &mut RenderSettings) {
    settings.format = OutputFormat::Value;
    settings.format_explicit = true;
    settings.mode = crate::core::output::RenderMode::Plain;
    settings.color = crate::core::output::ColorMode::Never;
    settings.unicode = crate::core::output::UnicodeMode::Never;
}

fn progress_output_is_empty(output: &OutputResult) -> bool {
    match &output.items {
        OutputItems::Rows(rows) => {
            rows.is_empty()
                || rows.iter().all(|row| {
                    row.is_empty()
                        || (row.len() == 1 && row.get("value").is_some_and(Value::is_null))
                })
        }
        OutputItems::Groups(groups) => groups.is_empty(),
    }
}

pub(crate) fn apply_stages_to_cli_result(
    mut result: CommandOutput,
    stages: &[String],
) -> Result<CommandOutput> {
    let context = match &result.body {
        CommandBody::Text(_) => "failed to apply DSL stages to textual command output",
        CommandBody::Json(_) => "failed to apply DSL stages to JSON command output",
        CommandBody::None | CommandBody::Output(_) => {
            "failed to apply DSL stages to builtin command output"
        }
    };
    result.apply_stages(stages).wrap_err(context)?;
    Ok(result)
}

pub(crate) fn emit_messages_for_ui(
    config: &ResolvedConfig,
    ui: &UiState,
    messages: &MessageBuffer,
    verbosity: MessageLevel,
    sink: &mut dyn UiSink,
) {
    if ui.render_settings.format == OutputFormat::Json {
        let entries = messages
            .entries()
            .iter()
            .filter(|entry| entry.level <= verbosity)
            .map(|entry| {
                serde_json::json!({
                    "level": entry.level.as_str(),
                    "text": entry.text.as_str(),
                })
            })
            .collect::<Vec<_>>();
        if !entries.is_empty() {
            let rendered = serde_json::to_string_pretty(&serde_json::json!({
                "messages": entries,
            }))
            .unwrap_or_else(|_| r#"{"messages":[]}"#.to_string());
            sink.write_stderr(&format!("{rendered}\n"));
        }
        return;
    }

    let rendered = crate::ui::render_messages(messages, &{
        let mut request = crate::ui::MessageRequest::new(&ui.render_settings, verbosity);
        request.layout = crate::ui::messages::message_layout_from_config(config);
        request
    });
    if !rendered.is_empty() {
        sink.write_stderr(&rendered);
    }
}

fn maybe_copy_output(
    config: &ResolvedConfig,
    ui: &UiState,
    output: &OutputResult,
    sink: &mut dyn UiSink,
) {
    if !output.meta.wants_copy {
        return;
    }
    if let Err(err) =
        copy_output_to_clipboard(output, &ui.render_settings, &ClipboardService::new())
    {
        let mut messages = MessageBuffer::default();
        messages.warning(format!("clipboard copy failed: {err}"));
        emit_messages_for_ui(config, ui, &messages, ui.message_verbosity, sink);
    }
}

pub(crate) fn cli_result_from_plugin_response(
    response: ResponseV1,
    stages: &[String],
) -> Result<CommandOutput> {
    let mut result =
        CommandOutput::from_data(response.data, &response.meta, &response.messages, stages);
    if !response.ok {
        let report = if let Some(error) = response.error {
            result
                .messages
                .error(format!("{}: {}", error.code, error.message));
            let report = miette!("{}: {}", error.code, error.message);
            miette::Report::new(CommandBackendDetails {
                error,
                source: report,
                backtrace: std::backtrace::Backtrace::force_capture(),
            })
        } else {
            result.messages.error("command failed");
            miette!("command failed")
        };
        result.exit_code = 1;
        result.failure_report = Some(report);
    }
    Ok(result)
}

pub(crate) fn apply_output_stages(
    mut output: OutputResult,
    stages: &[String],
    format_hint: Option<OutputFormat>,
) -> Result<(OutputResult, Option<OutputFormat>)> {
    if output.meta.render_recommendation.is_none()
        && let Some(format) = format_hint
    {
        output.meta.render_recommendation = Some(RenderRecommendation::Format(format));
    }

    if !stages.is_empty() {
        tracing::trace!(stage_count = stages.len(), "applying DSL output pipeline");
        // This is the central fan-in for staged structured output. Keep all
        // callers on the canonical DSL entrypoint so semantic/output policy
        // stays consistent across CLI, REPL, and plugin flows.
        output = apply_output_pipeline(output, stages).map_err(|err| {
            crate::app::report_anyhow_with_context(
                err,
                crate::app::host::ErrorContext::Operation("failed to apply DSL output pipeline"),
            )
        })?;
        output.meta.display_limit = None;
        // Once a DSL pipeline runs, producer-side format hints stop being an
        // out-of-band override. Any surviving recommendation now lives on the
        // transformed output metadata itself.
        return Ok((output, None));
    }

    Ok((output, format_hint))
}

fn pager_for_output(
    config: &ResolvedConfig,
    ui: &UiState,
    output: &CommandOutput,
    rendered: &str,
) -> Option<String> {
    if config.get_string("ui.pager") != Some("auto")
        || !ui.render_settings.runtime.stdout_is_tty
        || match &output.body {
            CommandBody::Json(_) => true,
            CommandBody::Text(_) => ui.render_settings.format == OutputFormat::Json,
            CommandBody::None | CommandBody::Output(_) => {
                resolve_render_settings_with_hint(&ui.render_settings, output.format_hint).format
                    == OutputFormat::Json
            }
        }
        || rendered.lines().count() <= terminal_height()?
    {
        return None;
    }
    Some(
        std::env::var("PAGER")
            .ok()
            .filter(|pager| !pager.trim().is_empty())
            .unwrap_or_else(|| "less -RFX".to_string()),
    )
}

fn terminal_height() -> Option<usize> {
    terminal_size()
        .map(|(_, Height(rows))| rows as usize)
        .filter(|rows| *rows > 0)
        .or_else(|| {
            std::env::var("LINES")
                .ok()
                .and_then(|value| value.trim().parse::<usize>().ok())
                .filter(|rows| *rows > 0)
        })
}

pub(crate) fn render_saved_repl_output(
    config: &ResolvedConfig,
    ui: &UiState,
    output: &CommandOutput,
    stages: &[String],
) -> Result<String> {
    let mut staged = output.saved();
    let context = match &staged.body {
        CommandBody::Text(_) | CommandBody::Json(_)
            if stages.is_empty() && !ui.render_settings.format_explicit =>
        {
            return Ok(staged.render(config, &ui.render_settings));
        }
        CommandBody::Text(_) => "failed to replay staged textual output",
        CommandBody::Json(_) => "failed to replay staged JSON output",
        CommandBody::None | CommandBody::Output(_) => "failed to replay staged structured output",
    };
    staged.body = match staged.body.into_output() {
        Some(output) => CommandBody::Output(Box::new(output)),
        None => CommandBody::None,
    };
    staged.apply_stages(stages).wrap_err(context)?;
    Ok(staged.render(config, &ui.render_settings))
}

pub(crate) fn render_repl_command(
    config: &ResolvedConfig,
    ui: &UiState,
    session: &mut AppSession,
    line: &str,
    stages: &[String],
    mut result: CommandOutput,
    sink: &mut dyn UiSink,
) -> Result<String> {
    if result.exit_code != 0
        && let Some(report) = result.failure_report.take()
    {
        return Err(report);
    }
    if !result.messages.is_empty() {
        emit_messages_for_ui(config, ui, &result.messages, ui.message_verbosity, sink);
    }
    let saved = result.saved();
    let context = match &result.body {
        CommandBody::Text(_) => "failed to apply staged textual output pipeline",
        CommandBody::Json(_) => "failed to apply staged JSON output pipeline",
        CommandBody::None | CommandBody::Output(_) => {
            "failed to apply staged structured output pipeline"
        }
    };
    result.apply_stages(stages).wrap_err(context)?;
    let rendered = result.render(config, &ui.render_settings);
    if let CommandBody::Output(output) = &result.body {
        maybe_copy_output(config, ui, output, sink);
    }
    if !result.stderr_text.is_empty() {
        sink.write_stderr(&result.stderr_text);
    }
    if !matches!(saved.body, CommandBody::None) {
        session.record_success_output(line, saved, stages);
    }
    Ok(rendered)
}

fn text_output_to_rows(text: &str) -> OutputResult {
    rows_to_output_result(
        text.lines()
            .filter(|line| !line.is_empty())
            .map(|line| crate::row! { "value" => line })
            .collect(),
    )
}
