//! Top-level host entrypoint glue.
//!
//! This module ties together argv scanning, runtime/config bootstrap, dispatch
//! planning, sink rendering, and final exit-code classification. It is the
//! main "run the app like a program" surface above the lower-level runtime and
//! dispatch helpers.

use crate::config::ResolvedConfig;
use crate::core::output::OutputFormat;
use crate::native::NativeCommandRegistry;
use crate::repl;
use clap::{CommandFactory, FromArgMatches};
use miette::{Result, miette};
use nu_ansi_term::Style;

use crate::core::document::{DocBlock, Document, DocumentEntry, HelpLevel};
use crate::ui::{RenderBackend, RenderSettings};
use std::backtrace::Backtrace;
use std::borrow::Cow;
use std::ffi::OsString;
use std::fmt::{Debug, Formatter};
use std::time::Instant;

use super::help;
use super::help::help_level;
use crate::app::logging::{DeveloperLoggingConfig, init_developer_logging};
use crate::app::sink::{StdIoUiSink, UiSink};
use crate::app::{AppClients, AppState, AuthState, UiState};
use crate::cli::invocation::{
    InvocationOptions, ScannedCliArgs, extend_with_invocation_help, scan_cli_argv,
};
use crate::cli::{Cli, Commands};
use crate::plugin::CommandCatalogEntry;

#[cfg(test)]
pub(crate) use super::assembly::rebuild_repl_state;
pub(crate) use super::dispatch::{
    DispatchPlan, RunAction, build_dispatch_plan, normalize_profile_override,
};
use super::external::run_external_command;
pub(crate) use super::external::run_external_command_with_help_renderer;
pub(crate) use super::timing::{TimingSummary, format_timing_badge};
pub(crate) const CMD_PLUGINS: &str = "plugins";
pub(crate) const CMD_DOCTOR: &str = "doctor";
pub(crate) const CMD_CONFIG: &str = "config";
pub(crate) const CMD_THEME: &str = "theme";
pub(crate) const CMD_HISTORY: &str = "history";
pub(crate) const CMD_CONTEXT: &str = "context";
pub(crate) const CMD_INTRO: &str = "intro";
pub(crate) const CMD_HELP: &str = "help";
pub(crate) const CMD_LIST: &str = "list";
pub const EXIT_CODE_ERROR: i32 = 1;
pub const EXIT_CODE_USAGE: i32 = 2;
pub const EXIT_CODE_WAITING_APPROVAL: i32 = 3;
pub(crate) const DEFAULT_REPL_PROMPT: &str = "╭─{user}@{domain} {indicator}\n╰─{profile}> ";
pub(crate) const CURRENT_TERMINAL_SENTINEL: &str = "__current__";

#[derive(Debug, Clone)]
pub(crate) struct ReplCommandSpec {
    pub(crate) name: Cow<'static, str>,
    pub(crate) supports_dsl: bool,
}

#[derive(Debug, Clone)]
pub(crate) struct ResolvedInvocation {
    pub(crate) ui: UiState,
    pub(crate) help_level: HelpLevel,
}

struct PreparedHostRun {
    state: AppState,
    dispatch: DispatchPlan,
    invocation_ui: ResolvedInvocation,
}

#[derive(Debug, Clone)]
pub(crate) struct ErrorDisplaySettings {
    pub(crate) verbosity: crate::ui::messages::MessageLevel,
    pub(crate) debug_level: u8,
    pub(crate) render_settings: RenderSettings,
}

impl From<&UiState> for ErrorDisplaySettings {
    fn from(ui: &UiState) -> Self {
        Self {
            verbosity: ui.verbosity,
            debug_level: ui.debug_level,
            render_settings: ui.render_settings.clone(),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ErrorContext {
    NativeExecution,
    Config,
    Operation(&'static str),
}

impl std::fmt::Display for ErrorContext {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Self::NativeExecution => "native command execution failed",
            Self::Config => "config resolution failed",
            Self::Operation(text) => text,
        })
    }
}

#[derive(Debug)]
struct BoxedContextError {
    context: ErrorContext,
    source: Box<dyn std::error::Error + Send + Sync + 'static>,
    backtrace: Backtrace,
    command_path: Option<String>,
}

#[derive(Debug)]
struct ReportContextError {
    context: Option<ErrorContext>,
    source: miette::Report,
    error_display: Option<ErrorDisplaySettings>,
    backtrace: Option<Backtrace>,
}

#[derive(Clone, Copy)]
struct KnownErrorChain<'a> {
    boxed_context: Option<&'a BoxedContextError>,
    clap: Option<&'a clap::Error>,
    config: Option<&'a crate::config::ConfigError>,
}

impl<'a> KnownErrorChain<'a> {
    fn inspect(err: &'a miette::Report) -> Self {
        Self {
            boxed_context: find_error_in_chain::<BoxedContextError>(err),
            clap: find_error_in_chain::<clap::Error>(err),
            config: find_error_in_chain::<crate::config::ConfigError>(err),
        }
    }
}

impl std::fmt::Display for BoxedContextError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        std::fmt::Display::fmt(&self.context, f)
    }
}

impl std::error::Error for BoxedContextError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        Some(self.source.as_ref())
    }
}

impl miette::Diagnostic for BoxedContextError {}

impl std::fmt::Display for ReportContextError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self.context {
            Some(context) => std::fmt::Display::fmt(&context, f),
            None => std::fmt::Display::fmt(&self.source, f),
        }
    }
}

impl std::error::Error for ReportContextError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        Some(self.source.as_ref())
    }
}
impl miette::Diagnostic for ReportContextError {
    fn code<'a>(&'a self) -> Option<Box<dyn std::fmt::Display + 'a>> {
        self.source.code()
    }

    fn severity(&self) -> Option<miette::Severity> {
        self.source.severity()
    }

    fn help<'a>(&'a self) -> Option<Box<dyn std::fmt::Display + 'a>> {
        self.source.help()
    }

    fn url<'a>(&'a self) -> Option<Box<dyn std::fmt::Display + 'a>> {
        self.source.url()
    }

    fn source_code(&self) -> Option<&dyn miette::SourceCode> {
        self.source.source_code()
    }

    fn labels(&self) -> Option<Box<dyn Iterator<Item = miette::LabeledSpan> + '_>> {
        self.source.labels()
    }

    fn related<'a>(&'a self) -> Option<Box<dyn Iterator<Item = &'a dyn miette::Diagnostic> + 'a>> {
        self.source.related()
    }

    fn diagnostic_source(&self) -> Option<&dyn miette::Diagnostic> {
        Some(self.source.as_ref())
    }
}

/// Runs the top-level CLI entrypoint from an argv-like iterator.
///
/// This is the library-friendly wrapper around the binary entrypoint and
/// returns the process exit code that should be reported to the caller.
pub fn run_from<I, T>(args: I) -> Result<i32>
where
    I: IntoIterator<Item = T>,
    T: Into<std::ffi::OsString> + Clone,
{
    crate::startup_trace::initialize();
    let mut sink = StdIoUiSink;
    let result = run_from_with_sink(args, &mut sink);
    crate::startup_trace::finish();
    result
}

pub(crate) fn run_from_with_sink<I, T>(args: I, sink: &mut dyn UiSink) -> Result<i32>
where
    I: IntoIterator<Item = T>,
    T: Into<std::ffi::OsString> + Clone,
{
    run_from_with_sink_and_app(args, sink, &std::sync::Arc::default())
}

pub(crate) fn run_from_with_sink_and_app<I, T>(
    args: I,
    sink: &mut dyn UiSink,
    app: &std::sync::Arc<super::AppDefinition>,
) -> Result<i32>
where
    I: IntoIterator<Item = T>,
    T: Into<std::ffi::OsString> + Clone,
{
    crate::startup_trace::initialize();
    let argv = {
        let _phase = tracing::info_span!(target: "osp_startup", "argv collection").entered();
        args.into_iter().map(Into::into).collect::<Vec<OsString>>()
    };
    let pipe = argv.iter().position(|arg| arg == "|").unwrap_or(argv.len());
    let mut scan = scan_cli_argv(&argv[..pipe]);
    run_scanned_with_sink_and_app(
        &argv[pipe..],
        &mut scan,
        sink,
        app,
        &mut TimingSummary::default(),
    )
}

pub(crate) fn run_scanned_with_sink_and_app(
    suffix: &[OsString],
    scanned: &mut ScannedCliArgs,
    sink: &mut dyn UiSink,
    app: &std::sync::Arc<super::AppDefinition>,
    timing: &mut TimingSummary,
) -> Result<i32> {
    {
        let _phase =
            tracing::info_span!(target: "osp_startup", "developer logging bootstrap").entered();
        init_developer_logging(DeveloperLoggingConfig::new(scanned.invocation.debug));
    }
    let (suffix, stages) = {
        let _phase = tracing::info_span!(target: "osp_startup", "argv/invocation scan").entered();
        if let Some(error) = scanned.error.take() {
            return Err(error);
        }
        let suffix = suffix
            .iter()
            .map(|arg| arg.to_string_lossy().into_owned())
            .collect::<Vec<_>>();
        let mut stage_tokens = vec!["command".to_string()];
        stage_tokens.extend(suffix.iter().cloned());
        let stages = crate::cli::pipeline::split_command_tokens(&stage_tokens).stages;
        crate::cli::validate_cli_dsl_stages(&stages)?;
        (suffix, stages)
    };
    let (parsed, help_command) = {
        let _phase = tracing::info_span!(target: "osp_startup", "clap parse").entered();
        let mut command = Cli::command();
        for &(name, help, key, _) in &app.config_flags {
            let conflicts = app
                .config_flags
                .iter()
                .filter(|&&(other, _, other_key, _)| other != name && other_key == key)
                .map(|&(other, _, _, _)| other);
            command = command.arg(
                clap::Arg::new(name)
                    .long(name)
                    .help(help)
                    .action(clap::ArgAction::SetTrue)
                    .conflicts_with_all(conflicts),
            );
        }
        for flag in &app.action_flags {
            let mut arg = clap::Arg::new(flag.name).long(flag.name).help(flag.help);
            arg = if let Some(value) = flag.optional_value {
                arg.value_name(value.value_name)
                    .num_args(0..=1)
                    .default_missing_value("")
            } else {
                arg.action(clap::ArgAction::SetTrue)
            };
            command = command.arg(arg);
        }
        let parsed = command
            .try_get_matches_from_mut(scanned.argv.iter().cloned())
            .and_then(|matches| {
                let mut cli = Cli::from_arg_matches(&matches)?;
                cli.product_overrides = product_config_overrides(app, &matches);
                for flag in &app.action_flags {
                    if matches.value_source(flag.name) == Some(clap::parser::ValueSource::CommandLine) {
                        if cli.product_action.is_some() || cli.command.is_some() {
                            return Err(clap::Error::raw(clap::error::ErrorKind::ArgumentConflict,
                                "product action flags cannot be combined with commands or other actions"));
                        }
                        cli.product_action = Some(flag.name.to_string());
                    }
                }
                Ok(cli)
            });
        (parsed, command)
    };
    match parsed {
        Ok(Cli {
            command: Some(Commands::Completions(args)),
            ..
        }) => {
            if !stages.is_empty() {
                return Err(miette!(
                    "shell completion scripts do not support DSL pipeline stages"
                ));
            }
            let _phase = tracing::info_span!(target: "osp_startup", "command execution").entered();
            sink.write_stdout(&crate::cli::render_shell_completions(args));
            Ok(0)
        }
        Ok(mut cli) => {
            if let Some(Commands::External(tokens)) = cli.command.as_mut() {
                tokens.extend(suffix);
                run(cli, scanned.invocation.clone(), &[], sink, app, timing)
            } else {
                run(cli, scanned.invocation.clone(), &stages, sink, app, timing)
            }
        }
        Err(err) => handle_clap_parse_error(scanned, &help_command, err, &stages, sink, app),
    }
}

fn product_config_overrides(
    app: &std::sync::Arc<super::AppDefinition>,
    matches: &clap::ArgMatches,
) -> crate::config::ConfigLayer {
    let mut overrides = crate::config::ConfigLayer::default();
    for &(name, _, key, value) in &app.config_flags {
        if matches.get_flag(name) {
            overrides.set(key, value);
        }
    }
    for flag in &app.action_flags {
        if let Some(argument) = flag.optional_value
            && let Some(value) = matches
                .get_one::<String>(flag.name)
                .filter(|v| !v.is_empty())
        {
            overrides.set(argument.config_key, value.clone());
        }
    }
    overrides
}

fn handle_clap_parse_error(
    scanned: &ScannedCliArgs,
    command: &clap::Command,
    err: clap::Error,
    stages: &[String],
    sink: &mut dyn UiSink,
    app: &std::sync::Arc<super::AppDefinition>,
) -> Result<i32> {
    match err.kind() {
        clap::error::ErrorKind::DisplayHelp
        | clap::error::ErrorKind::DisplayHelpOnMissingArgumentOrSubcommand => {
            let _phase = tracing::info_span!(target: "osp_startup", "command execution").entered();
            let tokens = scanned
                .argv
                .iter()
                .map(|arg| arg.to_string_lossy().into_owned())
                .collect::<Vec<_>>();
            let product_overrides = crate::cli::passive_command(command.clone())
                .try_get_matches_from(crate::cli::help_parse_tokens(&tokens))
                .map(|matches| product_config_overrides(app, &matches))
                .unwrap_or_default();
            let help_context = help::render_settings_for_help_with_scan(
                scanned,
                &app.product_defaults,
                &product_overrides,
            );
            if let Some(config) = &help_context.config {
                let _phase = tracing::info_span!(target: "osp_startup", "native command configure")
                    .entered();
                app.native_commands.configure(config);
            }
            let (mut body, root_help, long_help) = crate::cli::command_help_document(
                command.clone(),
                &tokens,
                &crate::core::CommandScope::OneShot,
            );
            extend_with_invocation_help(&mut body, help_context.help_level);
            if root_help {
                add_product_option_help(&mut body, &app.product_help_options);
                let auth =
                    AuthState::for_policy(app.access_policy.clone(), help_context.config.as_ref());
                add_native_command_help(&mut body, &app.native_commands, &auth);
                if let Some(config) = &help_context.config {
                    let matches = crate::cli::passive_command(command.clone())
                        .try_get_matches_from(crate::cli::help_parse_tokens(&tokens))
                        .ok();
                    let mut launch = super::LaunchContext::default();
                    if let Some(matches) = &matches {
                        launch.plugin_dirs = matches
                            .try_get_many::<std::path::PathBuf>("plugin_dirs")
                            .ok()
                            .flatten()
                            .into_iter()
                            .flatten()
                            .cloned()
                            .collect();
                        let flag = |name| {
                            matches
                                .try_get_one::<bool>(name)
                                .ok()
                                .flatten()
                                .copied()
                                .unwrap_or(false)
                        };
                        launch.runtime_load = crate::cli::runtime_load_options_from_flags(
                            flag("no_env"),
                            flag("no_config_file"),
                            flag("defaults_only"),
                        );
                    }
                    let plugins = super::assembly::build_plugin_manager(config, &launch);
                    let natives = app
                        .native_commands
                        .catalog()
                        .into_iter()
                        .map(|e| e.name)
                        .collect::<std::collections::HashSet<_>>();
                    let entries = plugins
                        .command_catalog()
                        .into_iter()
                        .filter(|entry| {
                            !natives.contains(&entry.name)
                                && !crate::cli::builtin_command_names().contains(&entry.name)
                                && auth.is_external_command_visible(&entry.name)
                        })
                        .map(|entry| DocumentEntry {
                            name: entry.name,
                            description: entry.about,
                        })
                        .collect::<Vec<_>>();
                    if !entries.is_empty() {
                        body.blocks.push(DocBlock::section(
                            "Plugins",
                            vec![DocBlock::Entries { entries }],
                        ));
                    }
                }
            }
            let mut filtered = body.filtered_for_help_level(help_context.help_level);
            let dsl_help = stages.iter().find_map(|raw| {
                let parsed = crate::dsl::parse::pipeline::parse_stage(raw).ok()?;
                crate::cli::is_cli_help_stage(&parsed).then_some(parsed.spec)
            });
            if let Some(spec) = &dsl_help {
                filtered = crate::dsl::verb_info::help_document(spec)?;
            }
            let stages = if dsl_help.is_some() { &[][..] } else { stages };
            let mut output = crate::dsl::apply_output_pipeline(filtered.to_output_result(), stages)
                .map_err(|error| miette!("failed to transform help output: {error}"))?;
            output.meta.document_long_help = long_help;
            let rendered = crate::ui::render(
                &output,
                &crate::ui::RenderRequest {
                    layout: Some(help_context.layout),
                    ..crate::ui::RenderRequest::new(&help_context.settings)
                },
            );
            sink.write_stdout(&rendered);
            Ok(0)
        }
        clap::error::ErrorKind::DisplayVersion => {
            if !stages.is_empty() {
                return Err(miette!("version text does not support DSL pipeline stages"));
            }
            let _phase = tracing::info_span!(target: "osp_startup", "command execution").entered();
            sink.write_stdout(&err.to_string());
            Ok(0)
        }
        _ => Err(report_clap_error_with_context(
            err,
            crate::app::host::ErrorContext::Operation("failed to parse CLI arguments"),
            command.clone(),
            scanned.argv.clone(),
        )),
    }
}

// Keep the top-level CLI entrypoint readable as a table of contents:
// normalize input -> bootstrap runtime state -> hand off to the selected mode.
fn run(
    mut cli: Cli,
    invocation: InvocationOptions,
    stages: &[String],
    sink: &mut dyn UiSink,
    app: &std::sync::Arc<super::AppDefinition>,
    timing: &mut TimingSummary,
) -> Result<i32> {
    let run_started = Instant::now();
    let PreparedHostRun {
        mut state,
        dispatch,
        mut invocation_ui,
    } = {
        let _phase = tracing::info_span!(target: "osp_startup", "host startup").entered();
        prepare_host_run(&mut cli, &invocation, app, run_started)?
    };

    if let Some(help) = crate::repl::completion::maybe_render_dsl_help(
        crate::repl::ReplViewContext::from_parts(&state.runtime, &state.session),
        stages,
    ) {
        sink.write_stdout(&help);
        return Ok(0);
    }
    if !stages.is_empty()
        && matches!(
            dispatch.action,
            RunAction::Repl | RunAction::ProductAction(_)
        )
    {
        return Err(miette!("a pipeline requires a command"));
    }

    let action_started = Instant::now();
    let is_repl = matches!(dispatch.action, RunAction::Repl);
    let action = dispatch.action;
    let result = {
        let _phase = (!is_repl)
            .then(|| tracing::info_span!(target: "osp_startup", "command execution").entered());
        match action {
            RunAction::ProductAction(name) => {
                let flag = app
                    .action_flags
                    .iter()
                    .find(|flag| flag.name == name)
                    .ok_or_else(|| miette!("unknown product action: {name}"))?;
                (flag.run)(&mut state.runtime).map(|message| {
                    if !message.is_empty() {
                        sink.write_stdout(&format!("{message}\n"));
                    }
                    0
                })
            }
            RunAction::Repl => {
                state.runtime.ui = invocation_ui.ui.clone();
                repl::run_plugin_repl(&mut state)
            }
            RunAction::External(tokens) => {
                run_external_command(&mut state, &tokens, &invocation, &mut invocation_ui, sink)
            }
            RunAction::Builtin(command) => super::run_cli_builtin_command_parts(
                &mut state.runtime,
                &mut state.session,
                &state.clients,
                &invocation_ui,
                command,
                stages,
                sink,
            ),
        }
    };

    if !is_repl {
        let total = run_started.elapsed();
        let startup = action_started.saturating_duration_since(run_started);
        *timing = TimingSummary {
            total,
            parse: Some(startup),
            execute: Some(total.saturating_sub(startup)),
            render: None,
        };
    }

    result.map_err(|err| report_with_invocation(err, ErrorDisplaySettings::from(&invocation_ui.ui)))
}

// Startup is phase-based:
// 1. choose the action from argv alone
// 2. assemble the runtime/session layer for the chosen action
// 3. apply startup-time side effects before handing off to execution
fn prepare_host_run(
    cli: &mut Cli,
    invocation: &InvocationOptions,
    app: &std::sync::Arc<super::AppDefinition>,
    run_started: Instant,
) -> Result<PreparedHostRun> {
    let dispatch = {
        let _phase = tracing::info_span!(target: "osp_startup", "dispatch planning").entered();
        build_dispatch_plan(cli)?
    };
    tracing::debug!(
        action = %dispatch.action.name(),
        profile_override = ?dispatch.profile_override,
        "built dispatch plan"
    );

    let terminal_kind = dispatch.action.terminal_kind();
    let mut state = {
        super::assembly::build_host(super::assembly::HostBuildRequest::Startup {
            cli,
            action: &dispatch.action,
            profile_override: dispatch.profile_override.clone(),
            terminal_kind,
            started: run_started,
            definition: app.clone(),
        })?
        .state()
    };
    {
        let _phase = tracing::info_span!(target: "osp_startup", "startup hook").entered();
        if let Some(policy) = &app.access_policy {
            policy.startup(&mut state.runtime)?;
        }
    }
    let invocation_ui = resolve_invocation_ui(
        state.runtime.config.resolved(),
        &state.runtime.ui,
        invocation,
    );
    super::assembly::apply_runtime_side_effects(
        state.runtime.config.resolved(),
        invocation_ui.ui.debug_level,
        &state.runtime.themes,
    );
    tracing::debug!(
        debug_level = invocation_ui.ui.debug_level,
        "developer logging initialized"
    );
    tracing::info!(
        profile = %state.runtime.config.resolved().active_profile(),
        terminal = %state.runtime.context.terminal_kind().as_config_terminal(),
        action = %dispatch.action.name(),
        plugin_timeout_ms = super::plugin_process_timeout(state.runtime.config.resolved()).as_millis(),
        "osp session initialized"
    );

    Ok(PreparedHostRun {
        state,
        dispatch,
        invocation_ui,
    })
}

pub(crate) fn authorized_command_catalog_for(
    auth: &AuthState,
    clients: &AppClients,
    native_catalog: Vec<CommandCatalogEntry>,
) -> Vec<CommandCatalogEntry> {
    let mut all = native_catalog;
    let mut names = all
        .iter()
        .map(|entry| entry.name.clone())
        .collect::<std::collections::HashSet<_>>();
    all.extend(
        clients
            .plugins()
            .command_catalog()
            .into_iter()
            .filter(|entry| {
                !crate::cli::builtin_command_names().contains(&entry.name)
                    && names.insert(entry.name.clone())
            }),
    );
    all.sort_by(|left, right| left.name.cmp(&right.name));
    all.into_iter()
        .filter(|entry| auth.is_external_command_visible(&entry.name))
        .map(|mut entry| {
            auth.filter_command_spec(&mut entry.completion, &[]);
            entry.subcommands.retain(|name| {
                entry
                    .completion
                    .children
                    .values()
                    .any(|child| child.name == *name)
            });
            entry
        })
        .collect()
}

pub(crate) fn resolve_invocation_ui(
    config: &ResolvedConfig,
    ui: &UiState,
    invocation: &InvocationOptions,
) -> ResolvedInvocation {
    let mut render_settings = ui.render_settings.clone();
    // A process-level format flag remains explicit until this command overrides it.
    render_settings.format_explicit =
        ui.render_settings.format_explicit || invocation.format.is_some();
    if let Some(format) = invocation.format {
        render_settings.format = format;
    }
    if let Some(mode) = invocation.mode {
        render_settings.mode = mode;
    }
    if let Some(color) = invocation.color {
        render_settings.color = color;
    }
    if let Some(unicode) = invocation.unicode {
        render_settings.unicode = unicode;
    }

    ResolvedInvocation {
        ui: UiState::new(
            render_settings,
            crate::ui::messages::adjust_verbosity(
                ui.verbosity,
                invocation.verbose,
                invocation.quiet,
            ),
            ui.debug_level.saturating_add(invocation.debug).min(3),
        ),
        help_level: help_level(config, invocation.verbose, invocation.quiet),
    }
}
/// Maps a reported error to the CLI exit code family used by OSP.
pub fn classify_exit_code(err: &miette::Report) -> i32 {
    let known = KnownErrorChain::inspect(err);
    if known.clap.is_some() {
        EXIT_CODE_USAGE
    } else {
        EXIT_CODE_ERROR
    }
}

/// Renders a concise failure below `Info`, or one detailed report at `Info` and above.
/// Backtraces are diagnostics emitted separately by the debug axis.
pub fn render_report_message(
    err: &miette::Report,
    level: crate::core::MessageLevel,
    settings: &RenderSettings,
) -> String {
    use crate::core::MessageLevel;
    let known = KnownErrorChain::inspect(err);
    let detailed = level >= MessageLevel::Info;
    let mut rendered = if let Some(clap) = known.clap {
        if detailed {
            clap.to_string()
        } else {
            terse_clap_error(clap)
        }
    } else {
        base_error_message(err, &known)
    };
    if !detailed {
        if let Some(hint) = known_error_hint(&known, &settings.runtime.command_scope)
            && !rendered.contains(&hint)
        {
            rendered.push_str("\nTry: ");
            rendered.push_str(&hint);
        }
        return rendered;
    }

    let mut seen = vec![rendered.clone()];
    let mut current: Option<&(dyn std::error::Error + 'static)> = Some(err.as_ref());
    while let Some(source) = current {
        // Presentation metadata and backend data are not additional causes.
        let metadata = source
            .downcast_ref::<ReportContextError>()
            .is_some_and(|context| context.context.is_none());
        let backend = source.is::<super::command_output::CommandBackendDetails>();
        let native = source
            .downcast_ref::<BoxedContextError>()
            .is_some_and(|context| context.context == ErrorContext::NativeExecution);
        let text = source.to_string();
        if !metadata
            && !backend
            && !native
            && !text.is_empty()
            && !seen.iter().any(|previous| previous.contains(&text))
        {
            rendered.push_str("\nCaused by: ");
            rendered.push_str(&text);
            seen.push(text);
        }
        current = source.source();
    }
    if let Some(config) = known.config
        && <crate::config::ConfigError as miette::Diagnostic>::source_code(config).is_some()
    {
        rendered.push_str("\n\n");
        let mut snippet = render_diagnostic_with_settings(&SourceSnippet(config), settings);
        // Graphical handlers can omit zero-width labels at the end of a file.
        if let Some(labels) = <crate::config::ConfigError as miette::Diagnostic>::labels(config) {
            for label in labels {
                if label.is_empty()
                    && let Some(text) = label.label()
                    && !snippet.contains(text)
                {
                    snippet.push_str(&format!("\n{text}\n"));
                }
            }
        }
        rendered.push_str(&snippet);
    }
    if let Some(backend) = find_error_in_chain::<super::command_output::CommandBackendDetails>(err)
        && !backend.error.details.is_null()
        && backend
            .error
            .details
            .as_object()
            .is_none_or(|object| !object.is_empty())
    {
        let mut record_settings = settings.clone();
        record_settings.format = OutputFormat::Mreg;
        record_settings.format_explicit = true;
        let output = crate::core::output_model::OutputResult::from_rows(
            crate::core::output_model::rows_from_value(backend.error.details.clone()),
        );
        rendered.push_str("\nBackend details:\n");
        rendered.push_str(&crate::ui::render(
            &output,
            &crate::ui::RenderRequest::new(&record_settings),
        ));
    }
    rendered
}

// Only source annotations are added here; the message and causes were rendered above.
#[derive(Debug)]
struct SourceSnippet<'a>(&'a crate::config::ConfigError);
impl std::fmt::Display for SourceSnippet<'_> {
    fn fmt(&self, _f: &mut Formatter<'_>) -> std::fmt::Result {
        Ok(())
    }
}
impl std::error::Error for SourceSnippet<'_> {}
impl miette::Diagnostic for SourceSnippet<'_> {
    fn source_code(&self) -> Option<&dyn miette::SourceCode> {
        self.0.source_code()
    }
    fn labels(&self) -> Option<Box<dyn Iterator<Item = miette::LabeledSpan> + '_>> {
        self.0.labels()
    }
}

pub(crate) fn has_report_detail(
    err: &miette::Report,
    level: crate::core::MessageLevel,
    settings: &RenderSettings,
) -> bool {
    if level >= crate::core::MessageLevel::Info {
        return false;
    }
    let summary = render_report_message(err, crate::core::MessageLevel::Success, settings);
    let detail = render_report_message(err, crate::core::MessageLevel::Info, settings);
    detail.trim() != summary.split("\nTry:").next().unwrap_or(&summary).trim()
}

pub(crate) fn capture_missing_report_backtrace(err: &miette::Report) -> Option<Backtrace> {
    report_backtrace(err)
        .is_none()
        .then(Backtrace::force_capture)
}

fn report_backtrace(err: &miette::Report) -> Option<&Backtrace> {
    let mut current: Option<&(dyn std::error::Error + 'static)> = Some(err.as_ref());
    let mut captured = None;
    while let Some(source) = current {
        if let Some(error) = source.downcast_ref::<BoxedContextError>() {
            return Some(&error.backtrace);
        }
        if let Some(error) = source.downcast_ref::<super::command_output::CommandBackendDetails>() {
            return Some(&error.backtrace);
        }
        if let Some(error) = source.downcast_ref::<ReportContextError>()
            && error.backtrace.is_some()
        {
            captured = error.backtrace.as_ref();
        }
        current = source.source();
    }
    captured
}

pub(crate) fn render_backtrace(err: &miette::Report, settings: &RenderSettings) -> Option<String> {
    report_backtrace(err).map(|trace| format_backtrace(&trace.to_string(), settings))
}

fn render_diagnostic_with_settings(
    diagnostic: &dyn miette::Diagnostic,
    settings: &RenderSettings,
) -> String {
    let handler = build_miette_handler(settings);
    format!(
        "{:?}",
        HandlerDebug {
            handler: &handler,
            diagnostic,
        }
    )
}

fn build_miette_handler(settings: &RenderSettings) -> miette::MietteHandler {
    let resolved = settings.resolve_render_settings();
    let mut opts = miette::MietteHandlerOpts::new()
        .with_cause_chain()
        .context_lines(2);

    if let Some(width) = resolved.width {
        opts = opts.width(width);
    }

    match resolved.backend {
        RenderBackend::Rich => opts
            .force_graphical(true)
            .color(resolved.color)
            .unicode(resolved.unicode)
            .build(),
        RenderBackend::Plain => opts
            .force_graphical(true)
            .color(false)
            .unicode(false)
            .build(),
    }
}

fn format_backtrace(backtrace: &str, settings: &RenderSettings) -> String {
    let resolved = settings.resolve_render_settings();
    let heading = if resolved.color {
        Style::new().bold().paint("Stack backtrace").to_string()
    } else {
        "Stack backtrace".to_string()
    };
    format!("{heading}:\n{backtrace}")
}

struct HandlerDebug<'a> {
    handler: &'a dyn miette::ReportHandler,
    diagnostic: &'a dyn miette::Diagnostic,
}

impl Debug for HandlerDebug<'_> {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        self.handler.debug(self.diagnostic, f)
    }
}

fn known_error_hint(
    known: &KnownErrorChain<'_>,
    scope: &crate::core::CommandScope,
) -> Option<String> {
    if let Some(config_err) = known.config {
        return Some(match config_err {
            crate::config::ConfigError::UnknownProfile { .. } => {
                format!(
                    "run `{}` or choose a known profile",
                    scope.command("config explain profile.default")
                )
            }
            crate::config::ConfigError::InsecureSecretsPermissions { .. } => {
                "restrict the secrets file permissions to 0600".to_string()
            }
            _ => format!(
                "run `{}` to inspect config provenance",
                scope.command("config explain <key>")
            ),
        });
    }

    if let Some(clap) = known.clap {
        let path = known
            .boxed_context
            .and_then(|context| context.command_path.as_deref());
        return Some(clap_error_action(clap, path, scope));
    }

    None
}

fn terse_clap_error(error: &clap::Error) -> String {
    let rendered = error.to_string();
    let mut lines = rendered
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty());
    let first = lines.next().unwrap_or("command usage is invalid");
    let mut terse = first
        .strip_prefix("error:")
        .unwrap_or(first)
        .trim()
        .to_string();
    if error.kind() == clap::error::ErrorKind::MissingRequiredArgument {
        for argument in lines
            .take_while(|line| !line.starts_with("Usage:"))
            .filter(|line| line.starts_with(['<', '-']))
        {
            terse.push(' ');
            terse.push_str(argument);
        }
    }
    for tip in rendered
        .lines()
        .map(str::trim)
        .filter(|line| line.starts_with("tip:"))
    {
        terse.push('\n');
        terse.push_str(tip);
    }
    terse
}

fn clap_error_action(
    error: &clap::Error,
    command_path: Option<&str>,
    scope: &crate::core::CommandScope,
) -> String {
    let rendered = error.to_string();
    let suggestions = rendered
        .lines()
        .find_map(|line| {
            let line = line.trim();
            line.strip_prefix("tip: a similar subcommand exists:")
                .or_else(|| line.strip_prefix("tip: some similar subcommands exist:"))
        })
        .into_iter()
        .flat_map(|value| value.split(','))
        .map(|value| value.trim().trim_matches(['`', '\'', '"']))
        .filter(|value| !value.is_empty())
        .collect::<Vec<_>>();
    let prefix = command_path.filter(|path| !path.is_empty());
    match (prefix, suggestions.as_slice()) {
        (Some(prefix), [suggestion]) => {
            format!("use `{}`", scope.command(&format!("{prefix} {suggestion}")))
        }
        (Some(prefix), suggestions) if !suggestions.is_empty() => format!(
            "use one of {}; run `{}` to see every subcommand",
            suggestions
                .iter()
                .map(|suggestion| format!("`{}`", scope.command(&format!("{prefix} {suggestion}"))))
                .collect::<Vec<_>>()
                .join(" or "),
            scope.command(&format!("{prefix} --help"))
        ),
        (None, [suggestion]) => format!("use subcommand `{suggestion}`"),
        (Some(prefix), _) => format!("use `{}`", scope.command(&format!("{prefix} --help"))),
        _ => "rerun this command with `--help` to inspect accepted values and options".to_string(),
    }
}

fn base_error_message(err: &miette::Report, known: &KnownErrorChain<'_>) -> String {
    if let Some(config_err) = known.config {
        return config_err.to_string();
    }

    if let Some(clap_err) = known.clap {
        return clap_err.to_string();
    }

    if let Some(context) = known.boxed_context
        && context.context == ErrorContext::NativeExecution
    {
        return context.source.to_string();
    }

    if let Some(details) =
        find_error_in_chain::<crate::app::command_output::CommandBackendDetails>(err)
        && let Some(source) = std::error::Error::source(details)
    {
        return source.to_string();
    }

    let outer = err.to_string();
    let mut deepest_source = None;
    let mut current = err.source();
    while let Some(source) = current {
        let text = source.to_string();
        if !text.is_empty() {
            deepest_source = Some(text);
        }
        current = source.source();
    }

    match deepest_source {
        // Context wrappers are still useful for verbose output, but at default
        // message levels users usually need the concrete actionable cause.
        Some(source) if find_error_in_chain::<ReportContextError>(err).is_some() => source,
        _ => outer,
    }
}

pub(crate) fn report_clap_error_with_context<I, T>(
    err: clap::Error,
    context: ErrorContext,
    command: clap::Command,
    args: I,
) -> miette::Report
where
    I: IntoIterator<Item = T>,
    T: Into<OsString> + Clone,
{
    let mut path = if command.get_name() == "osp" {
        Vec::new()
    } else {
        vec![command.get_name().to_string()]
    };
    if let Ok(matches) = crate::cli::passive_command(command).try_get_matches_from(args) {
        let mut current = &matches;
        while let Some((name, child)) = current.subcommand() {
            path.push(name.to_string());
            current = child;
        }
    }
    miette::Report::new(BoxedContextError {
        context,
        source: Box::new(err),
        backtrace: Backtrace::force_capture(),
        command_path: Some(path.join(" ")),
    })
}

pub(crate) fn report_std_error_with_context<E>(err: E, context: ErrorContext) -> miette::Report
where
    E: std::error::Error + Send + Sync + 'static,
{
    miette::Report::new(BoxedContextError {
        context,
        source: Box::new(err),
        backtrace: Backtrace::force_capture(),
        command_path: None,
    })
}

pub(crate) fn report_anyhow_with_context(
    err: anyhow::Error,
    context: ErrorContext,
) -> miette::Report {
    miette::Report::new(BoxedContextError {
        context,
        source: err.into_boxed_dyn_error(),
        backtrace: Backtrace::force_capture(),
        command_path: None,
    })
}

pub(crate) fn report_report_with_context(
    err: miette::Report,
    context: ErrorContext,
) -> miette::Report {
    miette::Report::new(ReportContextError {
        context: Some(context),
        source: err,
        error_display: None,
        backtrace: Some(Backtrace::force_capture()),
    })
}

pub(crate) fn report_with_invocation(
    err: miette::Report,
    settings: ErrorDisplaySettings,
) -> miette::Report {
    let backtrace = capture_missing_report_backtrace(&err);
    miette::Report::new(ReportContextError {
        context: None,
        source: err,
        error_display: Some(settings),
        backtrace,
    })
}

pub(crate) fn error_display_settings(err: &miette::Report) -> Option<&ErrorDisplaySettings> {
    find_error_in_chain::<ReportContextError>(err)
        .and_then(|context| context.error_display.as_ref())
}

pub(super) fn find_error_in_chain<E>(err: &miette::Report) -> Option<&E>
where
    E: std::error::Error + 'static,
{
    let mut current: Option<&(dyn std::error::Error + 'static)> = Some(err.as_ref());
    while let Some(source) = current {
        if let Some(found) = source.downcast_ref::<E>() {
            return Some(found);
        }
        current = source.source();
    }
    None
}

fn add_native_command_help(
    view: &mut Document,
    native_commands: &NativeCommandRegistry,
    auth: &AuthState,
) {
    let entries = native_commands
        .catalog()
        .into_iter()
        .filter(|entry| auth.is_external_command_visible(&entry.name))
        .map(|entry| DocumentEntry {
            name: entry.name,
            description: entry.about.trim().to_string(),
        })
        .collect::<Vec<_>>();
    if entries.is_empty() {
        return;
    }
    if let Some(DocBlock::Section { blocks, .. }) = view
        .blocks
        .iter_mut()
        .find(|block| matches!(block, DocBlock::Section { title, .. } if title == "Commands"))
    {
        if let Some(DocBlock::Entries { entries: commands }) = blocks.first_mut() {
            commands.splice(0..0, entries);
        }
    } else {
        view.blocks.push(DocBlock::section(
            "Commands",
            vec![DocBlock::Entries { entries }],
        ));
    }
}

fn add_product_option_help(view: &mut Document, options: &[(String, String)]) {
    if !options.is_empty() {
        view.blocks.push(DocBlock::section(
            "Product options",
            vec![DocBlock::Entries {
                entries: options
                    .iter()
                    .map(|(name, description)| DocumentEntry {
                        name: name.clone(),
                        description: description.clone(),
                    })
                    .collect(),
            }],
        ));
    }
}

pub(crate) fn resolve_render_settings_with_hint(
    settings: &RenderSettings,
    format_hint: Option<OutputFormat>,
) -> RenderSettings {
    if matches!(settings.format, OutputFormat::Auto)
        && let Some(format) = format_hint
    {
        let mut effective = settings.clone();
        effective.format = format;
        return effective;
    }
    settings.clone()
}
