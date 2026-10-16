//! External command parsing and dispatch.
//!
//! This module owns the path from already-tokenized user input to either a
//! native command, plugin command, inline builtin, or help rewrite on the
//! external command surface. It sits after top-level host setup but before the
//! final native/plugin execution paths.

use std::cell::RefCell;

use miette::{Result, WrapErr, miette};

use crate::app::{AppClients, AppRuntime, AppSession};
use crate::app::{RuntimeContext, TerminalKind, UiState};
use crate::cli::Commands;
use crate::cli::invocation::{InvocationOptions, extend_with_invocation_help, scan_command_tokens};
use crate::cli::pipeline::{ParsedCommandLine, parse_command_tokens_with_aliases};
use crate::core::document::Document;
use crate::native::{
    InvocationElevation, NativeCommandContext, NativeCommandOutcome, NativeProgressEvent,
    NativeProgressSink, NativeSessionContext,
};
use crate::plugin::PluginManager;
use crate::repl::completion;
use crate::repl::is_repl_shellable_command;

use super::command_output::{PreparedPluginOutput, apply_output_stages};
use super::dispatch::{
    ExternalCommandSource, ExternalPathAccessRequirement, before_external_run,
    canonical_external_command_name, ensure_external_access, resolve_external_command_source,
};
use super::sink::{ProgressUiSink, UiSink};
use super::{
    CliCommandResult, ResolvedInvocation, cli_result_from_plugin_response, enrich_dispatch_error,
    plugin_dispatch_context_for, run_inline_builtin_command, run_progress_command_with_ui,
    runtime_hints_for_invocation,
};

pub(super) struct ExternalCommandRuntime<'a> {
    pub(super) context: &'a RuntimeContext,
    pub(super) config_state: &'a crate::app::ConfigState,
    pub(super) ui: &'a UiState,
    pub(super) clients: &'a AppClients,
    pub(super) plugins: &'a PluginManager,
}

impl<'a> ExternalCommandRuntime<'a> {
    pub(super) fn from_parts(runtime: &'a AppRuntime, clients: &'a AppClients) -> Self {
        Self {
            context: &runtime.context,
            config_state: &runtime.config,
            ui: &runtime.ui,
            clients,
            plugins: clients.plugins(),
        }
    }
}

struct ParsedExternalInvocation {
    tokens: Vec<String>,
    stages: Vec<String>,
    apply_output_stages: bool,
    inline_command: Option<Commands>,
    elevation: InvocationElevation,
}

impl ParsedExternalInvocation {
    fn finish(&self, result: CliCommandResult) -> Result<CliCommandResult> {
        if self.apply_output_stages {
            super::command_output::apply_stages_to_cli_result(result, &self.stages)
        } else {
            Ok(result)
        }
    }
}

enum ExternalParse {
    Handled(Box<CliCommandResult>),
    Invocation(ParsedExternalInvocation),
}

pub(super) fn run_external_command(
    runtime: &mut AppRuntime,
    session: &mut AppSession,
    clients: &AppClients,
    tokens: &[String],
    options: &InvocationOptions,
    invocation: &mut ResolvedInvocation,
    sink: &mut dyn UiSink,
) -> Result<i32> {
    let expanded = expand_external_tokens(runtime, tokens);
    // Report expansion failures during dispatch, with the same command context
    // as the REPL. Invocation conflicts still fail before inline parsing.
    let scanned = match &expanded {
        Ok((parsed, _)) => Some(scan_command_tokens(&parsed.tokens)?),
        Err(_) => None,
    };
    if let Some(scanned) = &scanned
        && scanned.invocation != InvocationOptions::default()
    {
        let mut options = options.clone();
        options.merge(scanned.invocation.clone())?;
        *invocation =
            super::resolve_invocation_ui(runtime.config.resolved(), &runtime.ui, &options);
    }
    let parsed = expanded
        .and_then(|(mut parsed, elevation)| {
            if let Some(scanned) = scanned {
                parsed.tokens = scanned.tokens;
            }
            plan_external_invocation(runtime, session, parsed, elevation, invocation)
        })
        .wrap_err_with(|| external_parse_context(tokens))?;
    let result = dispatch_external_invocation(
        runtime,
        session,
        clients,
        parsed,
        invocation,
        |stdout| {
            let mut guide = Document::from_help_text(stdout);
            extend_with_invocation_help(&mut guide, invocation.help_level);
            guide.filtered_for_help_level(invocation.help_level)
        },
        Some(sink),
        None,
    )?;
    super::run_cli_command_with_ui(runtime.config.resolved(), &invocation.ui, result, sink)
}

pub(crate) fn run_external_command_with_help_renderer(
    runtime: &mut AppRuntime,
    session: &mut AppSession,
    clients: &AppClients,
    tokens: &[String],
    invocation: &ResolvedInvocation,
    guide_help: impl Fn(&str) -> Document,
    progress_sink: Option<&mut dyn UiSink>,
) -> Result<CliCommandResult> {
    let parsed = parse_external_invocation(runtime, session, tokens, invocation, &[])?;
    dispatch_external_invocation(
        runtime,
        session,
        clients,
        parsed,
        invocation,
        guide_help,
        progress_sink,
        None,
    )
}

pub(crate) fn run_external_command_for_repl(
    runtime: &mut AppRuntime,
    session: &mut AppSession,
    clients: &AppClients,
    command: ParsedCommandLine,
    invocation: &ResolvedInvocation,
    guide_help: impl Fn(&str) -> Document,
    progress_sink: Option<&mut dyn UiSink>,
    accepted: &mut bool,
) -> Result<CliCommandResult> {
    let parsed = parse_external_invocation(
        runtime,
        session,
        &command.tokens,
        invocation,
        &command.stages,
    )?;
    dispatch_external_invocation(
        runtime,
        session,
        clients,
        parsed,
        invocation,
        guide_help,
        progress_sink,
        Some(accepted),
    )
}

fn dispatch_external_invocation(
    runtime: &mut AppRuntime,
    session: &mut AppSession,
    clients: &AppClients,
    parsed: ExternalParse,
    invocation: &ResolvedInvocation,
    guide_help: impl Fn(&str) -> Document,
    progress_sink: Option<&mut dyn UiSink>,
    mut accepted: Option<&mut bool>,
) -> Result<CliCommandResult> {
    let mut parsed = match parsed {
        ExternalParse::Handled(result) => {
            mark_repl_command_accepted(&mut accepted);
            return Ok(*result);
        }
        ExternalParse::Invocation(parsed) => parsed,
    };

    if parsed.elevation.is_elevated()
        && parsed
            .inline_command
            .as_ref()
            .is_some_and(|command| !matches!(command, Commands::External(_)))
    {
        return Err(miette!(
            "elevation prefix is unsupported for built-in commands"
        ));
    }
    if let Some(command) = parsed.inline_command.take() {
        if !matches!(&command, Commands::External(_)) {
            mark_repl_command_accepted(&mut accepted);
        }
        if let Some(result) = run_inline_builtin_command(
            runtime,
            session,
            clients,
            Some(invocation),
            command,
            &parsed.stages,
        )? {
            return parsed.finish(result);
        }
    }
    if !parsed.stages.is_empty() {
        completion::validate_dsl_stages(&parsed.stages)
            .wrap_err("failed to validate DSL pipeline stages")?;
    }

    let (command, args) = parsed
        .tokens
        .split_first()
        .ok_or_else(|| miette!("missing external command"))?;
    let command = canonical_external_command_name(&runtime.auth, clients, command)?;
    let result = match resolve_external_command_source(
        &runtime.auth,
        clients,
        &command,
        invocation.plugin_provider.as_deref(),
    )? {
        ExternalCommandSource::Native => {
            let native_command = clients
                .native_commands()
                .command(&command)
                .ok_or_else(|| miette!("no native command provides `{command}`"))?;
            run_native_invocation(
                native_command.as_ref(),
                runtime,
                session,
                &command,
                args,
                &parsed,
                invocation,
                guide_help,
                progress_sink,
                &mut accepted,
            )
        }
        ExternalCommandSource::Plugin => {
            if parsed.elevation.is_elevated() {
                return Err(miette!(
                    "elevation prefix is unsupported for plugin command `{command}`"
                ));
            }
            run_external_plugin_command(
                runtime,
                session,
                clients,
                &command,
                &parsed,
                invocation,
                guide_help,
                &mut accepted,
            )
        }
    }?;
    parsed.finish(result)
}

struct NativeParse {
    args: Vec<String>,
    grammar: clap::Command,
    matches: std::result::Result<clap::ArgMatches, clap::Error>,
    path: crate::command_policy::CommandPath,
    render_help: bool,
}

impl NativeParse {
    fn new(
        command: &dyn crate::native::NativeCommand,
        name: &str,
        args: &[String],
    ) -> Result<Self> {
        let args = command.normalize_args(args).map_err(|error| {
            crate::app::report_anyhow_with_context(
                error,
                "failed to normalize native command arguments",
            )
        })?;
        let grammar = command.command();
        let matches = grammar
            .clone()
            .try_get_matches_from(std::iter::once(name.to_string()).chain(args.iter().cloned()));
        let path = native_resolved_path(&grammar, name, &args, matches.as_ref().ok());
        let render_help = matches
            .as_ref()
            .is_err_and(|error| is_native_help_error(error.kind()));
        Ok(Self {
            args,
            grammar,
            matches,
            path,
            render_help,
        })
    }
}

struct NativeIntent {
    requirement: ExternalPathAccessRequirement,
    render_help: bool,
    refresh_metadata: bool,
}

impl NativeIntent {
    fn from_parse(parsed: &NativeParse) -> Self {
        let render_help = parsed.render_help;
        let help_requested = render_help
            || parsed.args.first().is_some_and(|arg| arg == "help")
            || parsed
                .args
                .iter()
                .take_while(|arg| arg.as_str() != "--")
                .any(|arg| matches!(arg.as_str(), "-h" | "--help" | "-V" | "--version"));
        Self {
            requirement: if help_requested {
                ExternalPathAccessRequirement::Visible
            } else {
                ExternalPathAccessRequirement::Runnable
            },
            render_help,
            refresh_metadata: parsed.matches.is_err() && (render_help || !help_requested),
        }
    }
}

fn refresh_native_parse(
    command: &dyn crate::native::NativeCommand,
    name: &str,
    args: &[String],
    config: &crate::config::ResolvedConfig,
    parsed: NativeParse,
    intent: &NativeIntent,
) -> Result<NativeParse> {
    if !intent.refresh_metadata {
        return Ok(parsed);
    }
    let refreshed = command.prepare_invocation_metadata(args, config);
    if intent.requirement == ExternalPathAccessRequirement::Visible {
        match refreshed {
            Ok(true) => match NativeParse::new(command, name, args) {
                Ok(parsed) => return Ok(parsed),
                Err(error) => {
                    tracing::debug!(command = %name, error = %error, "refreshed native help arguments could not be normalized; using cached help")
                }
            },
            Ok(false) => {}
            Err(error) => {
                tracing::debug!(command = %name, error = %error, "native help metadata refresh failed; using cached help")
            }
        }
        return Ok(parsed);
    }
    if refreshed.map_err(|error| {
        crate::app::report_anyhow_with_context(
            error,
            "failed to prepare native invocation metadata",
        )
    })? {
        NativeParse::new(command, name, args)
    } else {
        Ok(parsed)
    }
}

fn run_native_invocation(
    command: &dyn crate::native::NativeCommand,
    runtime: &mut AppRuntime,
    session: &mut AppSession,
    name: &str,
    args: &[String],
    invocation_args: &ParsedExternalInvocation,
    invocation: &ResolvedInvocation,
    guide_help: impl Fn(&str) -> Document,
    progress_sink: Option<&mut dyn UiSink>,
    accepted: &mut Option<&mut bool>,
) -> Result<CliCommandResult> {
    if invocation_args.elevation.is_elevated() && !command.supports_invocation_elevation() {
        return Err(miette!(
            "elevation prefix is unsupported for native command `{name}`"
        ));
    }
    let parsed = NativeParse::new(command, name, args)?;
    let intent = NativeIntent::from_parse(&parsed);
    before_external_run(runtime, &parsed.path, intent.requirement, "command")?;
    let parsed = refresh_native_parse(
        command,
        name,
        args,
        runtime.config.resolved(),
        parsed,
        &intent,
    )?;
    runtime.auth.refresh_native_auth(&command.describe());
    ensure_external_access(runtime, &parsed.path, intent.requirement, "command")?;

    if intent.render_help || parsed.render_help {
        mark_repl_command_accepted(accepted);
        if let Err(error) = &parsed.matches
            && error.kind() == clap::error::ErrorKind::DisplayVersion
        {
            return Ok(CliCommandResult::document(guide_help(&error.to_string())));
        }
        let tokens = std::iter::once(name.to_string())
            .chain(parsed.args.iter().cloned())
            .collect::<Vec<_>>();
        let (document, _, _) = crate::cli::command_help_document(
            runtime.auth.visible_help_command(parsed.grammar, &[]),
            &tokens,
        );
        return Ok(CliCommandResult::document(document));
    }
    if let Err(error) = parsed.matches {
        let mut selected = &parsed.grammar;
        for segment in parsed.path.as_slice().iter().skip(1) {
            if let Some(child) = selected.find_subcommand(segment) {
                selected = child;
            }
        }
        if selected.has_subcommands() || parsed.args.len() + 1 == parsed.path.as_slice().len() {
            return Err(super::host::report_clap_error_with_context(
                error,
                "native command syntax is invalid",
                parsed.grammar,
                std::iter::once(name).chain(parsed.args.iter().map(String::as_str)),
            ));
        }
    }
    mark_repl_command_accepted(accepted);
    run_native_command(
        command,
        runtime,
        session,
        NativeRunInput {
            args: &parsed.args,
            stages: &invocation_args.stages,
            invocation,
            command_path: parsed.path.as_slice().join(" "),
            elevation: invocation_args.elevation,
            progress_sink,
        },
        guide_help,
        accepted,
    )
}

fn mark_repl_command_accepted(accepted: &mut Option<&mut bool>) {
    if let Some(accepted) = accepted.as_mut() {
        **accepted = true;
    }
}

fn mark_repl_command_rejected(accepted: &mut Option<&mut bool>) {
    if let Some(accepted) = accepted.as_mut() {
        **accepted = false;
    }
}

fn native_path_from_matches(
    command: &str,
    matches: &clap::ArgMatches,
) -> crate::command_policy::CommandPath {
    let mut segments = vec![command.to_string()];
    let mut current = matches;
    while let Some((subcommand, submatches)) = current.subcommand() {
        segments.push(subcommand.to_string());
        current = submatches;
    }
    crate::command_policy::CommandPath::new(segments)
}

fn is_native_help_error(kind: clap::error::ErrorKind) -> bool {
    matches!(
        kind,
        clap::error::ErrorKind::DisplayHelp
            | clap::error::ErrorKind::DisplayVersion
            | clap::error::ErrorKind::DisplayHelpOnMissingArgumentOrSubcommand
            | clap::error::ErrorKind::MissingSubcommand
    )
}

fn native_resolved_path(
    grammar: &clap::Command,
    name: &str,
    args: &[String],
    matches: Option<&clap::ArgMatches>,
) -> crate::command_policy::CommandPath {
    if let Some(matches) = matches {
        return native_path_from_matches(name, matches);
    }
    let mut tokens = args.iter().map(String::as_str).collect::<Vec<_>>();
    // Clap's help subcommand names a command path rather than executing it.
    if tokens.first() == Some(&"help") {
        tokens.remove(0);
    }
    tokens.retain(|arg| !matches!(*arg, "--help" | "-h"));
    crate::cli::passive_command(grammar.clone())
        .try_get_matches_from(std::iter::once(name).chain(tokens))
        .map(|matches| native_path_from_matches(name, &matches))
        .unwrap_or_else(|_| crate::command_policy::CommandPath::new([name]))
}

struct NativeRunInput<'args, 'stages, 'invocation, 'sink> {
    args: &'args [String],
    stages: &'stages [String],
    invocation: &'invocation ResolvedInvocation,
    command_path: String,
    elevation: InvocationElevation,
    progress_sink: Option<&'sink mut dyn UiSink>,
}

fn run_native_command(
    command: &dyn crate::native::NativeCommand,
    runtime: &mut AppRuntime,
    session: &mut AppSession,
    input: NativeRunInput<'_, '_, '_, '_>,
    guide_help: impl Fn(&str) -> Document,
    accepted: &mut Option<&mut bool>,
) -> Result<CliCommandResult> {
    session.native_context.clear_pagination();
    let progress_renderer = input.progress_sink.map(|sink| NativeProgressRenderer {
        config: runtime.config.resolved(),
        ui: &input.invocation.ui,
        stages: input.stages,
        sink: RefCell::new(ProgressUiSink::new(
            sink,
            input
                .invocation
                .ui
                .render_settings
                .resolve_render_settings()
                .unicode,
        )),
        pending: RefCell::new(None),
        last_draw: std::cell::Cell::new(None),
    });
    let mut context = NativeCommandContext::new(
        runtime.config.resolved(),
        runtime_hints_for_invocation(runtime, input.invocation),
    )
    .with_invocation_elevation(input.elevation)
    .with_session_context(
        session
            .native_context
            .for_command(input.command_path.as_str()),
    );
    if let Some(renderer) = progress_renderer.as_ref() {
        context = context.with_progress_sink(renderer);
    }

    let execution =
        command
            .execute(input.args, &context)
            .map_err(|err| match err.downcast::<clap::Error>() {
                Ok(err) => {
                    mark_repl_command_rejected(accepted);
                    super::host::report_clap_error_with_context(
                        err,
                        "native command execution failed",
                        command.command(),
                        std::iter::once(command.command().get_name().to_string())
                            .chain(input.args.iter().cloned()),
                    )
                }
                Err(err) => {
                    crate::app::report_anyhow_with_context(err, "native command execution failed")
                }
            });
    drop(context);
    if let Some(renderer) = progress_renderer.as_ref() {
        renderer.clear();
    }

    match execution? {
        NativeCommandOutcome::Help(text) => {
            let mut document = guide_help(&text);
            let path = input
                .command_path
                .split_whitespace()
                .map(str::to_string)
                .collect::<Vec<_>>();
            runtime.auth.filter_help_document(&mut document, &path);
            Ok(CliCommandResult::document(document))
        }
        NativeCommandOutcome::Exit(code) => Ok(CliCommandResult::exit(code)),
        NativeCommandOutcome::Response(response) => render_native_response_with_context(
            *response,
            input.stages,
            session,
            runtime.context.terminal_kind() == TerminalKind::Repl,
        ),
        NativeCommandOutcome::ResponseWithExit {
            response,
            exit_code,
        } => {
            let mut result = render_native_response_with_context(
                *response,
                input.stages,
                session,
                runtime.context.terminal_kind() == TerminalKind::Repl,
            )?;
            result.exit_code = exit_code;
            Ok(result)
        }
    }
}

struct NativeProgressRenderer<'config, 'sink> {
    config: &'config crate::config::ResolvedConfig,
    ui: &'config UiState,
    stages: &'config [String],
    sink: RefCell<ProgressUiSink<'sink>>,
    pending: RefCell<Option<NativeProgressEvent>>,
    last_draw: std::cell::Cell<Option<std::time::Instant>>,
}

impl NativeProgressRenderer<'_, '_> {
    fn clear(&self) {
        self.pending.borrow_mut().take();
        self.sink.borrow_mut().clear_progress();
    }

    fn draw(&self, event: NativeProgressEvent, stages: &[String]) -> anyhow::Result<()> {
        let mut result =
            PreparedPluginOutput::from_data(event.data, &event.meta, &event.messages, stages);
        (result.output, result.format_hint) =
            apply_output_stages(result.output, stages, result.format_hint)
                .map_err(|err| anyhow::Error::from_boxed(err.into()))?;
        let mut sink = self.sink.borrow_mut();
        run_progress_command_with_ui(self.config, self.ui, result, &mut *sink)
            .map_err(|err| anyhow::Error::from_boxed(err.into()))?;
        Ok(())
    }
}

impl NativeProgressSink for NativeProgressRenderer<'_, '_> {
    fn present(&self, document: NativeProgressEvent) -> anyhow::Result<()> {
        self.flush()?;
        self.draw(document, &[])
    }

    fn emit(&self, event: NativeProgressEvent) -> anyhow::Result<()> {
        let tree = event.meta.progress_replace
            && self.stages.is_empty()
            && self.sink.borrow().stderr_is_terminal()
            && self.ui.render_settings.format != crate::core::output::OutputFormat::Json;
        if !tree {
            return self.draw(event, self.stages);
        }
        let replay = event.replay;
        *self.pending.borrow_mut() = Some(event);
        if replay {
            return Ok(());
        }
        if self
            .last_draw
            .get()
            .is_none_or(|last| last.elapsed() >= std::time::Duration::from_millis(250))
        {
            self.flush()?;
        }
        Ok(())
    }

    fn flush(&self) -> anyhow::Result<()> {
        if let Some(event) = self.pending.borrow_mut().take() {
            if let Some(delay) = self
                .last_draw
                .get()
                .and_then(|last| std::time::Duration::from_millis(250).checked_sub(last.elapsed()))
            {
                std::thread::sleep(delay);
            }
            self.draw(event, self.stages)?;
            self.last_draw.set(Some(std::time::Instant::now()));
        }
        Ok(())
    }
}

fn render_native_response_with_context(
    response: crate::core::plugin::ResponseV1,
    stages: &[String],
    session: &mut AppSession,
    show_pagination_hint: bool,
) -> Result<CliCommandResult> {
    let mut result = match cli_result_from_plugin_response(response, stages) {
        Ok(result) => result,
        Err(error) => {
            session.native_context.clear_pagination();
            return Err(error);
        }
    };
    if show_pagination_hint {
        add_native_pagination_hint(&mut result, &session.native_context);
    }
    Ok(result)
}

fn add_native_pagination_hint(result: &mut CliCommandResult, context: &NativeSessionContext) {
    let Some(pagination) = context.pagination() else {
        return;
    };
    let text = match (context.has_previous_page(), pagination.next.is_some()) {
        (true, true) => {
            "More results are available. Type `next` or press Shift+→ for the next page, `prev` or Shift+← for the previous."
        }
        (false, true) => {
            "More results are available. Type `next` or press Shift+→ for the next page."
        }
        (true, false) => "Type `prev` or press Shift+← for the previous page.",
        (false, false) => return,
    };
    result.messages.success(text);
}

/// Returns invocation flags that only appear after alias or quoted-line
/// expansion, so the host can apply them like flags typed on the command line.
fn expand_external_tokens(
    runtime: &AppRuntime,
    tokens: &[String],
) -> Result<(ParsedCommandLine, InvocationElevation)> {
    let (tokens, elevation) = parse_invocation_elevation(tokens)?;
    let parsed = parse_command_tokens_with_aliases(&tokens, runtime.config.resolved())?;
    Ok((parsed, elevation))
}

fn external_parse_context(tokens: &[String]) -> String {
    format!(
        "failed to parse external command invocation for `{}`",
        tokens.first().map(String::as_str).unwrap_or("external")
    )
}

fn parse_external_invocation(
    runtime: &AppRuntime,
    session: &AppSession,
    tokens: &[String],
    invocation: &ResolvedInvocation,
    stages: &[String],
) -> Result<ExternalParse> {
    expand_external_tokens(runtime, tokens)
        .and_then(|(mut parsed, elevation)| {
            parsed.tokens = scan_command_tokens(&parsed.tokens)?.tokens;
            parsed.stages.extend_from_slice(stages);
            plan_external_invocation(runtime, session, parsed, elevation, invocation)
        })
        .wrap_err_with(|| external_parse_context(tokens))
}

fn plan_external_invocation(
    runtime: &AppRuntime,
    _session: &AppSession,
    parsed: ParsedCommandLine,
    elevation: InvocationElevation,
    invocation: &ResolvedInvocation,
) -> Result<ExternalParse> {
    let apply_output_stages = runtime.context.terminal_kind() == TerminalKind::Cli;
    if parsed.tokens.is_empty() {
        return Err(miette!("missing external command"));
    }
    if let Some(spec) = parsed.stages.iter().find_map(|raw| {
        let parsed = crate::dsl::parse::pipeline::parse_stage(raw).ok()?;
        crate::cli::is_cli_help_stage(&parsed).then_some(parsed.spec)
    }) {
        return Ok(ExternalParse::Handled(Box::new(
            CliCommandResult::document(crate::dsl::verb_info::help_document(&spec)?),
        )));
    }

    let inline_command = match super::command_plan::parse_inline_command(
        &parsed.tokens,
        super::command_plan::CommandSurface::External,
    )? {
        super::command_plan::InlineCommandPlan::Command(command) => command,
        super::command_plan::InlineCommandPlan::Help(_) => {
            let (mut view, long_help) = crate::cli::inline_help_document(&parsed.tokens);
            extend_with_invocation_help(&mut view, invocation.help_level);
            let mut output = view
                .filtered_for_help_level(invocation.help_level)
                .to_output_result();
            output.meta.document_long_help = long_help;
            let result = super::command_output::apply_stages_to_cli_result(
                CliCommandResult::output(output, None),
                if apply_output_stages {
                    &parsed.stages
                } else {
                    &[]
                },
            )?;
            return Ok(ExternalParse::Handled(Box::new(result)));
        }
    };

    Ok(ExternalParse::Invocation(ParsedExternalInvocation {
        tokens: rewrite_shellable_root_help_tokens(
            parsed.tokens,
            runtime.config.resolved(),
            invocation.plugin_provider.as_deref(),
        ),
        stages: parsed.stages,
        apply_output_stages,
        inline_command,
        elevation,
    }))
}

fn parse_invocation_elevation(tokens: &[String]) -> Result<(Vec<String>, InvocationElevation)> {
    if tokens.first().map(String::as_str) != Some("sudo") {
        return Ok((tokens.to_vec(), InvocationElevation::Ordinary));
    }
    if tokens.get(1).is_none() {
        return Err(miette!("sudo prefix must be followed by a command"));
    }

    let mut command_tokens = tokens.to_vec();
    command_tokens.remove(0);
    Ok((command_tokens, InvocationElevation::Elevated))
}

fn rewrite_shellable_root_help_tokens(
    tokens: Vec<String>,
    config: &crate::config::ResolvedConfig,
    provider_override: Option<&str>,
) -> Vec<String> {
    if provider_override.is_some() {
        return tokens;
    }

    if tokens.len() == 1
        && let Some(command) = tokens.first()
        && is_repl_shellable_command(config, command)
    {
        return vec![command.clone(), "--help".to_string()];
    }
    tokens
}

fn run_external_plugin_command(
    runtime_state: &mut AppRuntime,
    session: &mut AppSession,
    clients: &AppClients,
    command: &str,
    parsed: &ParsedExternalInvocation,
    invocation: &ResolvedInvocation,
    guide_help: impl Fn(&str) -> Document,
    accepted: &mut Option<&mut bool>,
) -> Result<CliCommandResult> {
    let (_, args) = parsed
        .tokens
        .split_first()
        .ok_or_else(|| miette!("missing external command"))?;
    let (path, dispatch_auth, help_requested) = clients
        .plugins()
        .resolved_command_path_and_auth(command, args, invocation.plugin_provider.as_deref())
        .map_err(|error| miette!(error.to_string()))?;
    mark_repl_command_accepted(accepted);
    let access_requirement = if help_requested {
        ExternalPathAccessRequirement::Visible
    } else {
        ExternalPathAccessRequirement::Runnable
    };
    let passive_auth = runtime_state.auth.replace_command_auth(dispatch_auth);
    let result = (|| {
        before_external_run(runtime_state, &path, access_requirement, "plugin command")?;
        ensure_external_access(runtime_state, &path, access_requirement, "plugin command")?;
        let runtime = ExternalCommandRuntime::from_parts(runtime_state, clients);

        tracing::debug!(
            command = %path.as_slice().join(" "),
            "dispatching external command"
        );

        if access_requirement == ExternalPathAccessRequirement::Visible {
            let dispatch_context = plugin_dispatch_context_for(&runtime, Some(invocation));
            let raw = runtime
                .plugins
                .dispatch_passthrough(command, args, &dispatch_context)
                .map_err(enrich_dispatch_error)?;
            let mut result = if !raw.stdout.is_empty() {
                let mut document = guide_help(&raw.stdout);
                runtime_state
                    .auth
                    .filter_help_document(&mut document, path.as_slice());
                CliCommandResult::document(document)
            } else {
                CliCommandResult::exit(raw.status_code)
            };
            if !raw.stderr.is_empty() {
                result.stderr_text = Some(raw.stderr);
            }
            result.exit_code = raw.status_code;
            return Ok(result);
        }

        let dispatch_context = plugin_dispatch_context_for(&runtime, Some(invocation));
        let response = runtime
            .plugins
            .dispatch(command, args, &dispatch_context)
            .map_err(enrich_dispatch_error)?;

        if response.ok
            && let Some(entries) = response.context.as_ref()
        {
            let context = session
                .native_context
                .for_command(path.as_slice().join(" "));
            for (kind, value) in entries {
                context.set_context_value(kind.clone(), value.clone());
            }
        }

        cli_result_from_plugin_response(response, &parsed.stages)
    })();
    runtime_state.auth.replace_command_auth(passive_auth);
    result
}

#[cfg(test)]
mod tests {
    use super::{
        ExternalParse, add_native_pagination_hint, cli_result_from_plugin_response,
        parse_external_invocation, run_external_command_with_help_renderer,
    };
    use crate::app::{
        AppClients, AppRuntime, AppSession, AuthState, ConfigState, LaunchContext, RuntimeContext,
        TerminalKind, UiState, resolve_invocation_ui,
    };
    use crate::app::{BufferedUiSink, CliCommandResult, ReplCommandOutput};
    use crate::cli::invocation::InvocationOptions;
    use crate::config::{ConfigLayer, ConfigResolver, ResolveOptions};
    use crate::core::document::Document;
    use crate::core::output::OutputFormat;
    use crate::core::plugin::{
        DescribeCommandAuthV1, DescribeCommandV1, DescribeVisibilityModeV1, PLUGIN_PROTOCOL_V1,
        ResponseMessageLevelV1, ResponseMessageV1, ResponseMetaV1, ResponseV1,
    };
    use crate::native::{
        NativeCommand, NativeCommandContext, NativeCommandOutcome, NativeCommandRegistry,
        NativePagination, NativeProgressEvent, NativeSessionContext,
    };
    use crate::plugin::PluginManager;
    use crate::ui::RenderSettings;
    use crate::ui::messages::MessageLevel;
    use clap::Command;
    #[cfg(unix)]
    use std::fs;
    #[cfg(unix)]
    use std::os::unix::fs::PermissionsExt;
    #[cfg(unix)]
    use std::path::Path;
    #[cfg(all(unix, miri))]
    use std::sync::atomic::{AtomicU64, Ordering};
    #[cfg(all(unix, not(miri)))]
    use std::time::{SystemTime, UNIX_EPOCH};

    #[derive(Clone, Copy)]
    enum NativeOutcomeKind {
        Help,
        Exit,
        Response,
        ResponseWithExit,
    }

    struct TestNativeCommand {
        kind: NativeOutcomeKind,
    }

    impl NativeCommand for TestNativeCommand {
        fn command(&self) -> Command {
            Command::new("ldap").about("Directory lookup")
        }

        fn auth(&self) -> Option<DescribeCommandAuthV1> {
            Some(DescribeCommandAuthV1 {
                visibility: Some(DescribeVisibilityModeV1::Public),
                required_capabilities: Vec::new(),
                feature_flags: Vec::new(),
                ..DescribeCommandAuthV1::default()
            })
        }

        fn execute(
            &self,
            args: &[String],
            _context: &NativeCommandContext<'_>,
        ) -> anyhow::Result<NativeCommandOutcome> {
            Ok(match self.kind {
                NativeOutcomeKind::Help => NativeCommandOutcome::Help(format!(
                    "Usage: osp ldap\n\nArgs: {}\n",
                    args.join(" ")
                )),
                NativeOutcomeKind::Exit => NativeCommandOutcome::Exit(7),
                NativeOutcomeKind::Response => {
                    NativeCommandOutcome::Response(Box::new(ResponseV1 {
                        protocol_version: PLUGIN_PROTOCOL_V1,
                        ok: true,
                        data: serde_json::json!([{ "command": "ldap", "args": args }]),
                        error: None,
                        messages: vec![ResponseMessageV1 {
                            level: ResponseMessageLevelV1::Info,
                            text: "native ok".to_string(),
                        }],
                        context: None,
                        meta: ResponseMetaV1 {
                            format_hint: Some("json".to_string()),
                            columns: None,
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
                    }))
                }
                NativeOutcomeKind::ResponseWithExit => NativeCommandOutcome::ResponseWithExit {
                    response: Box::new(ResponseV1 {
                        protocol_version: PLUGIN_PROTOCOL_V1,
                        ok: true,
                        data: serde_json::json!({"status": "waiting_approval"}),
                        error: None,
                        messages: Vec::new(),
                        context: None,
                        meta: ResponseMetaV1 {
                            format_hint: Some("json".to_string()),
                            unix_timestamp_columns: Vec::new(),
                            display_rules: Vec::new(),
                            preserve_json_document: true,
                            ..ResponseMetaV1::default()
                        },
                    }),
                    exit_code: 3,
                },
            })
        }
    }

    struct ProgressNativeCommand;

    impl NativeCommand for ProgressNativeCommand {
        fn command(&self) -> Command {
            Command::new("progress").about("Emit progress before returning")
        }

        fn auth(&self) -> Option<DescribeCommandAuthV1> {
            Some(DescribeCommandAuthV1 {
                visibility: Some(DescribeVisibilityModeV1::Public),
                ..DescribeCommandAuthV1::default()
            })
        }

        fn execute(
            &self,
            _args: &[String],
            context: &NativeCommandContext<'_>,
        ) -> anyhow::Result<NativeCommandOutcome> {
            context.emit_progress(
                NativeProgressEvent::new(serde_json::json!({
                    "status": "running",
                    "message": "creating virtual machine",
                }))
                .with_meta(ResponseMetaV1 {
                    format_hint: Some("json".to_string()),
                    unix_timestamp_columns: Vec::new(),
                    display_rules: Vec::new(),
                    preserve_json_document: true,
                    ..ResponseMetaV1::default()
                }),
            )?;
            Ok(NativeCommandOutcome::Response(Box::new(ResponseV1 {
                protocol_version: PLUGIN_PROTOCOL_V1,
                ok: true,
                data: serde_json::json!({"status": "completed"}),
                error: None,
                messages: Vec::new(),
                context: None,
                meta: ResponseMetaV1 {
                    format_hint: Some("json".to_string()),
                    unix_timestamp_columns: Vec::new(),
                    display_rules: Vec::new(),
                    preserve_json_document: true,
                    ..ResponseMetaV1::default()
                },
            })))
        }
    }

    struct VerbosityNativeCommand;

    impl NativeCommand for VerbosityNativeCommand {
        fn command(&self) -> Command {
            Command::new("verbosity").about("Echo native runtime hints")
        }

        fn auth(&self) -> Option<DescribeCommandAuthV1> {
            Some(DescribeCommandAuthV1 {
                visibility: Some(DescribeVisibilityModeV1::Public),
                ..DescribeCommandAuthV1::default()
            })
        }

        fn execute(
            &self,
            _args: &[String],
            context: &NativeCommandContext<'_>,
        ) -> anyhow::Result<NativeCommandOutcome> {
            Ok(NativeCommandOutcome::Response(Box::new(ResponseV1 {
                protocol_version: PLUGIN_PROTOCOL_V1,
                ok: true,
                data: serde_json::json!({
                    "ui_verbosity": context.runtime_hints.ui_verbosity.as_str(),
                }),
                error: None,
                messages: Vec::new(),
                context: None,
                meta: ResponseMetaV1 {
                    format_hint: Some("json".to_string()),
                    columns: None,
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

    fn make_test_state_with_registry(
        native_commands: NativeCommandRegistry,
    ) -> (AppRuntime, AppSession, AppClients) {
        make_test_state_with_parts(
            PluginManager::new(Vec::new())
                .with_bundled_roots(false)
                .with_default_roots(false),
            native_commands,
        )
    }

    fn make_test_state_with_parts(
        plugins: PluginManager,
        native_commands: NativeCommandRegistry,
    ) -> (AppRuntime, AppSession, AppClients) {
        let mut defaults = ConfigLayer::default();
        defaults.set("profile.default", "default");
        defaults.set("theme.path", Vec::<String>::new());
        let mut resolver = ConfigResolver::default();
        resolver.set_defaults(defaults);
        let config = resolver
            .resolve(ResolveOptions::default())
            .expect("test config should resolve");
        let themes = crate::ui::theme_catalog::load_theme_catalog(&config);
        let mut auth = AuthState::default();
        auth.replace_command_auth(native_commands.command_auth());
        let runtime = AppRuntime::new(
            RuntimeContext::new(None, TerminalKind::Cli, None),
            ConfigState::new(config.clone()),
            UiState::new(
                RenderSettings::test_plain(OutputFormat::Json),
                MessageLevel::Success,
                0,
            ),
            auth,
            themes,
            LaunchContext::default(),
        );
        let session = AppSession::default();
        let clients = AppClients::new(plugins, native_commands);
        (runtime, session, clients)
    }

    fn make_test_state_with_native(
        kind: Option<NativeOutcomeKind>,
    ) -> (AppRuntime, AppSession, AppClients) {
        make_test_state_with_registry(
            kind.map(|kind| NativeCommandRegistry::new().with_command(TestNativeCommand { kind }))
                .unwrap_or_default(),
        )
    }

    struct NestedAuthNativeCommand;

    struct AliasedNestedAuthNativeCommand;

    impl NativeCommand for NestedAuthNativeCommand {
        fn command(&self) -> Command {
            Command::new("ldap")
                .about("Directory lookup")
                .arg(
                    clap::Arg::new("format")
                        .long("format")
                        .value_name("FORMAT")
                        .global(true),
                )
                .subcommand(Command::new("user").about("Look up one user"))
        }

        fn describe(&self) -> DescribeCommandV1 {
            let mut root = DescribeCommandV1::from_clap(self.command());
            root.auth = Some(DescribeCommandAuthV1 {
                visibility: Some(DescribeVisibilityModeV1::Public),
                required_capabilities: Vec::new(),
                feature_flags: Vec::new(),
                ..DescribeCommandAuthV1::default()
            });
            root.subcommands[0].auth = Some(DescribeCommandAuthV1 {
                visibility: Some(DescribeVisibilityModeV1::Authenticated),
                required_capabilities: vec!["ldap.user.read".to_string()],
                feature_flags: Vec::new(),
                ..DescribeCommandAuthV1::default()
            });
            root
        }

        fn execute(
            &self,
            args: &[String],
            _context: &NativeCommandContext<'_>,
        ) -> anyhow::Result<NativeCommandOutcome> {
            let _ = args;
            panic!("nested-auth native command should not execute when auth denies it")
        }
    }

    impl NativeCommand for AliasedNestedAuthNativeCommand {
        fn command(&self) -> Command {
            Command::new("lookup").subcommand(Command::new("user").visible_alias("u"))
        }

        fn describe(&self) -> DescribeCommandV1 {
            let mut root = DescribeCommandV1::from_clap(self.command());
            root.auth = Some(DescribeCommandAuthV1 {
                visibility: Some(DescribeVisibilityModeV1::Public),
                ..DescribeCommandAuthV1::default()
            });
            root.subcommands[0].auth = Some(DescribeCommandAuthV1 {
                visibility: Some(DescribeVisibilityModeV1::Authenticated),
                required_capabilities: vec!["lookup.user.read".to_string()],
                ..DescribeCommandAuthV1::default()
            });
            root
        }

        fn execute(
            &self,
            _args: &[String],
            _context: &NativeCommandContext<'_>,
        ) -> anyhow::Result<NativeCommandOutcome> {
            panic!("aliased nested command should not execute when auth denies it")
        }
    }

    #[cfg(unix)]
    fn make_temp_dir(prefix: &str) -> std::path::PathBuf {
        #[cfg(not(miri))]
        let unique = format!(
            "{prefix}-{}-{}",
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .expect("clock should be after epoch")
                .as_nanos()
        );
        #[cfg(miri)]
        let unique = {
            static NEXT_ID: AtomicU64 = AtomicU64::new(1);
            format!(
                "{prefix}-{}-{}",
                std::process::id(),
                NEXT_ID.fetch_add(1, Ordering::Relaxed)
            )
        };
        let dir = std::env::temp_dir().join(unique);
        fs::create_dir_all(&dir).expect("temp dir should be created");
        dir
    }

    #[cfg(unix)]
    fn write_provider_test_plugin(dir: &Path, plugin_id: &str, command: &str) {
        let plugin_path = dir.join(format!("osp-{plugin_id}"));
        let script = format!(
            r#"#!/bin/sh
if [ "$1" = "--describe" ]; then
  cat <<'JSON'
{{"protocol_version":1,"plugin_id":"{plugin_id}","plugin_version":"0.1.0","min_osp_version":null,"commands":[{{"name":"{command}","about":"{plugin_id} provider","args":[],"flags":{{}},"subcommands":[]}}]}}
JSON
  exit 0
fi

cat <<'JSON'
{{"protocol_version":1,"ok":true,"data":{{"provider":"{plugin_id}"}},"error":null,"messages":[],"meta":{{"format_hint":"json","columns":["provider"]}}}}
JSON
"#
        );
        fs::write(&plugin_path, script).expect("plugin script should be written");
        let mut perms = fs::metadata(&plugin_path)
            .expect("plugin metadata should be readable")
            .permissions();
        perms.set_mode(0o755);
        fs::set_permissions(&plugin_path, perms).expect("plugin should be executable");
    }

    #[test]
    fn external_builtin_help_passthrough_is_handled_unit() {
        let (runtime, session, _) = make_test_state_with_native(None);
        let tokens = ["config", "--help"]
            .into_iter()
            .map(str::to_string)
            .collect::<Vec<_>>();

        let invocation = crate::app::resolve_invocation_ui(
            runtime.config.resolved(),
            &runtime.ui,
            &Default::default(),
        );
        let parsed = parse_external_invocation(&runtime, &session, &tokens, &invocation, &[])
            .expect("help should parse");
        assert!(matches!(
            parsed,
            ExternalParse::Handled(result) if matches!(*result, CliCommandResult {
                exit_code: 0,
                output: Some(ReplCommandOutput::Output(_)),
                ..
            })
        ));
    }

    #[test]
    fn external_plugin_response_preserves_messages_unit() {
        let response = ResponseV1 {
            protocol_version: 1,
            ok: true,
            data: serde_json::json!({ "message": "hello" }),
            error: None,
            messages: vec![ResponseMessageV1 {
                level: ResponseMessageLevelV1::Warning,
                text: "warning from plugin".to_string(),
            }],
            context: None,
            meta: ResponseMetaV1 {
                format_hint: Some("json".to_string()),
                columns: None,
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

        let result =
            cli_result_from_plugin_response(response, &[]).expect("response should prepare");
        assert!(!result.messages.is_empty());
    }

    #[test]
    fn native_pagination_hint_describes_available_directions_unit() {
        let context = NativeSessionContext::default();
        let mut result = CliCommandResult::exit(0);
        add_native_pagination_hint(&mut result, &context);
        assert!(result.messages.is_empty());

        context.set_pagination(NativePagination {
            current: vec!["orch".to_string(), "1".to_string()],
            next: Some(vec!["orch".to_string(), "2".to_string()]),
        });
        add_native_pagination_hint(&mut result, &context);
        assert_eq!(
            result.messages.entries()[0].text,
            "More results are available. Type `next` or press Shift+→ for the next page."
        );

        let mut result = CliCommandResult::exit(0);
        context.step_page(true);
        context.set_pagination(NativePagination {
            current: vec!["orch".to_string(), "2".to_string()],
            next: Some(vec!["orch".to_string(), "3".to_string()]),
        });
        add_native_pagination_hint(&mut result, &context);
        assert!(result.messages.entries()[0].text.contains("`prev`"));
        assert!(result.messages.entries()[0].text.contains("`next`"));
    }

    #[test]
    fn external_native_command_help_exit_and_response_paths_unit() {
        for kind in [
            NativeOutcomeKind::Help,
            NativeOutcomeKind::Exit,
            NativeOutcomeKind::Response,
            NativeOutcomeKind::ResponseWithExit,
        ] {
            let (mut runtime, mut session, clients) = make_test_state_with_native(Some(kind));
            let invocation = resolve_invocation_ui(
                runtime.config.resolved(),
                &runtime.ui,
                &InvocationOptions::default(),
            );
            let result = run_external_command_with_help_renderer(
                &mut runtime,
                &mut session,
                &clients,
                &["ldap".to_string(), "user".to_string()],
                &invocation,
                |text| Document::from_help_text(&format!("HELP::{text}")),
                None,
            )
            .expect("native command should dispatch");

            match kind {
                NativeOutcomeKind::Help => {
                    assert!(matches!(
                        result.output,
                        Some(ReplCommandOutput::Output(guide))
                            if guide
                                .output.document.as_ref().expect("expected document").value.to_string().contains("HELP::Usage: osp ldap")
                    ));
                }
                NativeOutcomeKind::Exit => {
                    assert_eq!(result.exit_code, 7);
                    assert!(result.output.is_none());
                }
                NativeOutcomeKind::Response => {
                    assert_eq!(result.exit_code, 0);
                    assert!(!result.messages.is_empty());
                    assert!(matches!(result.output, Some(ReplCommandOutput::Output(_))));
                }
                NativeOutcomeKind::ResponseWithExit => {
                    assert_eq!(result.exit_code, 3);
                    assert!(matches!(result.output, Some(ReplCommandOutput::Output(_))));
                }
            }
        }
    }

    #[test]
    fn external_native_progress_renders_to_stderr_without_corrupting_final_output_unit() {
        let (mut runtime, mut session, clients) = make_test_state_with_registry(
            NativeCommandRegistry::new().with_command(ProgressNativeCommand),
        );
        let invocation = resolve_invocation_ui(
            runtime.config.resolved(),
            &runtime.ui,
            &InvocationOptions::default(),
        );
        let mut sink = BufferedUiSink::default();

        let result = run_external_command_with_help_renderer(
            &mut runtime,
            &mut session,
            &clients,
            &["progress".to_string()],
            &invocation,
            Document::from_help_text,
            Some(&mut sink),
        )
        .expect("native command should emit progress and return");

        assert!(sink.stdout.is_empty());
        assert!(sink.stderr.contains("creating virtual machine"));
        assert_eq!(result.exit_code, 0);
        assert!(matches!(result.output, Some(ReplCommandOutput::Output(_))));
    }

    #[test]
    fn external_native_command_receives_invocation_verbosity_unit() {
        let (mut runtime, mut session, clients) = make_test_state_with_registry(
            NativeCommandRegistry::new().with_command(VerbosityNativeCommand),
        );
        let options = InvocationOptions {
            verbose: 1,
            ..InvocationOptions::default()
        };
        let invocation = resolve_invocation_ui(runtime.config.resolved(), &runtime.ui, &options);

        let result = run_external_command_with_help_renderer(
            &mut runtime,
            &mut session,
            &clients,
            &["verbosity".to_string()],
            &invocation,
            Document::from_help_text,
            None,
        )
        .expect("native command should dispatch");

        let Some(ReplCommandOutput::Output(output)) = result.output else {
            panic!("expected structured output");
        };
        let rows = output.output.as_rows().expect("rows");
        assert_eq!(
            rows[0].get("ui_verbosity"),
            Some(&serde_json::json!("info"))
        );
    }

    #[test]
    fn external_bare_shellable_root_rewrites_to_help_unit() {
        let (mut runtime, mut session, clients) =
            make_test_state_with_native(Some(NativeOutcomeKind::Help));
        let invocation = resolve_invocation_ui(
            runtime.config.resolved(),
            &runtime.ui,
            &InvocationOptions::default(),
        );
        let result = run_external_command_with_help_renderer(
            &mut runtime,
            &mut session,
            &clients,
            &["ldap".to_string()],
            &invocation,
            |text| Document::from_help_text(&format!("HELP::{text}")),
            None,
        )
        .expect("bare shellable root should rewrite to help");

        assert!(matches!(
            result.output,
            Some(ReplCommandOutput::Output(guide))
                if guide
                    .output.document.as_ref().expect("expected document").value.to_string().contains("HELP::")
        ));
    }

    #[test]
    fn external_dispatch_matches_native_commands_case_insensitively_unit() {
        let (mut runtime, mut session, clients) =
            make_test_state_with_native(Some(NativeOutcomeKind::Help));
        let invocation = resolve_invocation_ui(
            runtime.config.resolved(),
            &runtime.ui,
            &InvocationOptions::default(),
        );
        let result = run_external_command_with_help_renderer(
            &mut runtime,
            &mut session,
            &clients,
            &["LDAP".to_string()],
            &invocation,
            |text| Document::from_help_text(&format!("HELP::{text}")),
            None,
        )
        .expect("mixed-case native command should dispatch");

        assert!(matches!(
            result.output,
            Some(ReplCommandOutput::Output(guide))
                if guide
                    .output.document.as_ref().expect("expected document").value.to_string().contains("HELP::Usage: osp ldap")
        ));
    }

    #[test]
    fn external_dispatch_suggests_nearby_native_command_unit() {
        let (mut runtime, mut session, clients) =
            make_test_state_with_native(Some(NativeOutcomeKind::Help));
        let invocation = resolve_invocation_ui(
            runtime.config.resolved(),
            &runtime.ui,
            &InvocationOptions::default(),
        );
        let err = run_external_command_with_help_renderer(
            &mut runtime,
            &mut session,
            &clients,
            &["lpad".to_string(), "user".to_string(), "oistes".to_string()],
            &invocation,
            Document::from_help_text,
            None,
        )
        .expect_err("misspelled command should fail with a suggestion");

        assert!(err.to_string().contains("unknown command `lpad`"));
        assert!(err.to_string().contains("Try: use command `ldap`"));
    }

    #[test]
    fn external_dispatch_enforces_nested_native_command_auth_unit() {
        let (mut runtime, mut session, clients) = make_test_state_with_registry(
            NativeCommandRegistry::new().with_command(NestedAuthNativeCommand),
        );

        let invocation = resolve_invocation_ui(
            runtime.config.resolved(),
            &runtime.ui,
            &InvocationOptions::default(),
        );
        let err = run_external_command_with_help_renderer(
            &mut runtime,
            &mut session,
            &clients,
            &[
                "ldap".to_string(),
                "--format".to_string(),
                "json".to_string(),
                "user".to_string(),
                "alice".to_string(),
            ],
            &invocation,
            Document::from_help_text,
            None,
        )
        .expect_err("nested auth should block native external dispatch");

        assert!(err.to_string().contains("command `ldap user`"));
        assert!(err.to_string().contains("requires authentication"));
        assert!(err.to_string().contains("Try: authenticate"));
    }

    #[test]
    fn native_path_uses_clap_canonical_subcommand_for_aliases_unit() {
        let (mut runtime, mut session, clients) = make_test_state_with_registry(
            NativeCommandRegistry::new().with_command(AliasedNestedAuthNativeCommand),
        );

        let invocation = resolve_invocation_ui(
            runtime.config.resolved(),
            &runtime.ui,
            &InvocationOptions::default(),
        );
        let err = run_external_command_with_help_renderer(
            &mut runtime,
            &mut session,
            &clients,
            &["lookup".to_string(), "u".to_string()],
            &invocation,
            Document::from_help_text,
            None,
        )
        .expect_err("the canonical aliased child policy should deny execution");

        assert!(err.to_string().contains("command `lookup user`"));
        assert!(err.to_string().contains("requires authentication"));
    }

    #[test]
    fn external_native_help_requires_visibility_but_not_runnability_unit() {
        let (mut runtime, mut session, clients) = make_test_state_with_registry(
            NativeCommandRegistry::new().with_command(NestedAuthNativeCommand),
        );
        let invocation = resolve_invocation_ui(
            runtime.config.resolved(),
            &runtime.ui,
            &InvocationOptions::default(),
        );

        let result = run_external_command_with_help_renderer(
            &mut runtime,
            &mut session,
            &clients,
            &["ldap".to_string(), "help".to_string(), "user".to_string()],
            &invocation,
            Document::from_help_text,
            None,
        )
        .expect("visible nested help should not require its run capability");

        assert!(matches!(result.output, Some(ReplCommandOutput::Output(_))));

        let denied = run_external_command_with_help_renderer(
            &mut runtime,
            &mut session,
            &clients,
            &["ldap".to_string(), "user".to_string(), "alice".to_string()],
            &invocation,
            Document::from_help_text,
            None,
        )
        .expect_err("real execution should still require runnability");
        assert!(denied.to_string().contains("requires authentication"));
    }

    #[test]
    fn external_native_help_obeys_product_visibility_unit() {
        struct ProductAccess;
        impl crate::app::AccessPolicy for ProductAccess {
            fn visibility(&self, path: &[String]) -> crate::app::Visibility {
                if path == ["ldap", "user"] {
                    crate::app::Visibility::Hidden(Some(
                        "Use the product's operator profile".into(),
                    ))
                } else {
                    crate::app::Visibility::Shown
                }
            }
        }
        let (mut runtime, mut session, clients) = make_test_state_with_registry(
            NativeCommandRegistry::new().with_command(NestedAuthNativeCommand),
        );
        runtime.set_access_policy(Some(std::sync::Arc::new(ProductAccess)));
        let invocation = resolve_invocation_ui(
            runtime.config.resolved(),
            &runtime.ui,
            &InvocationOptions::default(),
        );
        let error = run_external_command_with_help_renderer(
            &mut runtime,
            &mut session,
            &clients,
            &["ldap".into(), "user".into(), "--help".into()],
            &invocation,
            Document::from_help_text,
            None,
        )
        .expect_err("hidden command help must remain inaccessible");
        assert_eq!(error.to_string(), "Use the product's operator profile");
    }

    #[cfg(unix)]
    #[cfg_attr(miri, ignore = "external plugin filesystem/process test")]
    #[test]
    fn external_dispatch_rejects_native_plugin_source_collisions_unit() {
        let root = make_temp_dir("osp-cli-external-native-collision");
        let plugins_dir = root.join("plugins");
        fs::create_dir_all(&plugins_dir).expect("plugin dir should be created");
        write_provider_test_plugin(&plugins_dir, "alpha", "ldap");

        let (mut runtime, mut session, clients) = make_test_state_with_parts(
            PluginManager::new(vec![plugins_dir]),
            NativeCommandRegistry::new().with_command(TestNativeCommand {
                kind: NativeOutcomeKind::Response,
            }),
        );
        let invocation = resolve_invocation_ui(
            runtime.config.resolved(),
            &runtime.ui,
            &InvocationOptions::default(),
        );

        let err = run_external_command_with_help_renderer(
            &mut runtime,
            &mut session,
            &clients,
            &["ldap".to_string()],
            &invocation,
            Document::from_help_text,
            None,
        )
        .expect_err("native/plugin collision should fail");

        assert!(err.to_string().contains("ambiguous across command sources"));
        fs::remove_dir_all(root).ok();
    }

    #[cfg(unix)]
    #[cfg_attr(miri, ignore = "external plugin filesystem/process test")]
    #[test]
    fn external_dispatch_requires_provider_selection_before_plugin_run_unit() {
        let root = make_temp_dir("osp-cli-external-provider-selection");
        let plugins_dir = root.join("plugins");
        fs::create_dir_all(&plugins_dir).expect("plugin dir should be created");
        write_provider_test_plugin(&plugins_dir, "alpha", "shared");
        write_provider_test_plugin(&plugins_dir, "beta", "shared");

        let (mut runtime, mut session, clients) = make_test_state_with_parts(
            PluginManager::new(vec![plugins_dir]),
            NativeCommandRegistry::default(),
        );
        let invocation = resolve_invocation_ui(
            runtime.config.resolved(),
            &runtime.ui,
            &InvocationOptions::default(),
        );

        let err = run_external_command_with_help_renderer(
            &mut runtime,
            &mut session,
            &clients,
            &["shared".to_string()],
            &invocation,
            Document::from_help_text,
            None,
        )
        .expect_err("ambiguous plugin command should fail before dispatch");

        assert!(err.to_string().contains("requires provider selection"));
        fs::remove_dir_all(root).ok();
    }

    #[cfg(unix)]
    #[cfg_attr(miri, ignore = "external plugin filesystem/process test")]
    #[test]
    fn external_dispatch_provider_override_routes_plugin_over_native_unit() {
        let root = make_temp_dir("osp-cli-external-provider-override");
        let plugins_dir = root.join("plugins");
        fs::create_dir_all(&plugins_dir).expect("plugin dir should be created");
        write_provider_test_plugin(&plugins_dir, "alpha", "ldap");

        let (mut runtime, mut session, clients) = make_test_state_with_parts(
            PluginManager::new(vec![plugins_dir]),
            NativeCommandRegistry::new().with_command(TestNativeCommand {
                kind: NativeOutcomeKind::Response,
            }),
        );
        let invocation = resolve_invocation_ui(
            runtime.config.resolved(),
            &runtime.ui,
            &InvocationOptions {
                plugin_provider: Some("alpha".to_string()),
                ..InvocationOptions::default()
            },
        );

        let result = run_external_command_with_help_renderer(
            &mut runtime,
            &mut session,
            &clients,
            &["ldap".to_string()],
            &invocation,
            Document::from_help_text,
            None,
        )
        .expect("provider override should route to plugin");

        let rows = match result.output {
            Some(ReplCommandOutput::Output(output)) => output
                .output
                .as_rows()
                .expect("expected row output")
                .to_vec(),
            other => panic!("expected structured output, got {other:?}"),
        };
        assert_eq!(rows, vec![crate::row! { "provider" => "alpha" }]);
        fs::remove_dir_all(root).ok();
    }

    #[cfg(unix)]
    #[cfg_attr(miri, ignore = "external plugin filesystem/process test")]
    #[test]
    fn external_dispatch_matches_plugin_commands_case_insensitively_unit() {
        let root = make_temp_dir("osp-cli-external-plugin-case-insensitive");
        let plugins_dir = root.join("plugins");
        fs::create_dir_all(&plugins_dir).expect("plugin dir should be created");
        write_provider_test_plugin(&plugins_dir, "alpha", "shared");

        let (mut runtime, mut session, clients) = make_test_state_with_parts(
            PluginManager::new(vec![plugins_dir]),
            NativeCommandRegistry::default(),
        );
        let invocation = resolve_invocation_ui(
            runtime.config.resolved(),
            &runtime.ui,
            &InvocationOptions::default(),
        );

        let result = run_external_command_with_help_renderer(
            &mut runtime,
            &mut session,
            &clients,
            &["SHARED".to_string()],
            &invocation,
            Document::from_help_text,
            None,
        )
        .expect("mixed-case plugin command should dispatch");

        let rows = match result.output {
            Some(ReplCommandOutput::Output(output)) => output
                .output
                .as_rows()
                .expect("expected row output")
                .to_vec(),
            other => panic!("expected structured output, got {other:?}"),
        };
        assert_eq!(rows, vec![crate::row! { "provider" => "alpha" }]);
        fs::remove_dir_all(root).ok();
    }
}
