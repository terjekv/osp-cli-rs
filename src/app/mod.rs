//! CLI and REPL startup through the public [`App`] entrypoints.
//!
//! This is the process-facing layer of the crate. It wires together CLI
//! parsing, config loading, plugin/catalog setup, rendering, and REPL startup
//! into the public [`App`] entrypoints. Lower-level modules like
//! [`crate::config`], [`crate::ui`], and [`crate::repl`] stay reusable because
//! this module is where the product-level orchestration happens.
//!
//! Contract:
//!
//! - this is allowed to depend on the rest of the crate because it is the host
//!   composition layer
//! - the dependency should not point the other way; lower-level modules should
//!   not import [`crate::app`] to get work done
//!
//! Public API shape:
//!
//! - most callers should start with [`App`] or [`App::builder`]
//! - embedders may inspect runtime/session state, but the preferred
//!   construction path still flows through a small number of builders and
//!   constructors here such as [`crate::app::AppStateBuilder`]
//! - lower-level semantic payloads live in modules like [`crate::core::document`] and
//!   [`crate::completion`]; this module assembles the host
//!
//! Use this module when you want:
//!
//! - the full CLI / REPL host in-process
//! - the same dispatch/help/completion/config behavior as `osp`
//! - a wrapper crate that injects native commands or product defaults
//!
//! Skip this module and start lower if you only need:
//!
//! - rendering rows/documents: [`crate::ui`]
//! - pure completion trees or guide payloads: [`crate::completion`] or
//!   [`crate::core::document`]
//!
//! Broad-strokes host flow:
//!
//! ```text
//! argv / REPL request
//!      │
//!      ▼ [ app ]    build host state, load config, assemble command catalog
//!      ▼ [ dispatch ] choose native or plugin command, run it
//!      ▼ [ dsl ]    apply trailing pipeline stages to command output
//!      ▼ [ ui ]     render text to a UiSink or process stdio
//! ```
//!
//! Most callers only need one of these shapes:
//!
//! - [`App::run_from`] when they want a structured `Result<i32>`
//! - [`App::run_process`] when they want process-style exit code conversion
//! - [`App::with_sink`] or [`App::builder`] plus
//!   [`AppBuilder::build_with_sink`] when a test or outer host wants captured
//!   host-rendered one-shot stdout/stderr; REPL input and prompts still use the
//!   process terminal
//! Downstream product-wrapper pattern:
//!
//! - keep site-specific auth, policy, and integration state in the wrapper
//!   crate rather than in [`crate::app`]
//! - build a [`crate::NativeCommandRegistry`] for product-specific commands
//! - build one product-owned defaults layer under `extensions.<site>.*`
//! - inject both through [`App::builder`], then
//!   [`AppBuilder::with_native_commands`] and
//!   [`AppBuilder::with_product_defaults`]
//! - expose a thin product-level `run_process` or `builder()` API on top
//!   instead of forking generic host behavior

use crate::config::ConfigLayer;
use crate::core::output::OutputFormat;
use crate::native::NativeCommandRegistry;
use crate::ui::messages::{MessageBuffer, MessageLevel, adjust_verbosity};
use crate::ui::{MessageRequest, render_messages};
use std::ffi::OsString;
use std::sync::Arc;

pub mod access;
pub(crate) mod assembly;
pub(crate) mod bootstrap;
pub(crate) mod builtin;
pub(crate) mod command_output;
pub(crate) mod command_plan;
pub(crate) mod config_explain;
pub(crate) mod dispatch;
pub(crate) mod external;
pub(crate) mod facts;
pub(crate) mod help;
pub(crate) mod host;
pub(crate) mod logging;
pub(crate) mod rebuild;
pub(crate) mod repl_lifecycle;
pub(crate) mod runtime;
pub(crate) mod session;
/// UI sink abstractions used by the host entrypoints.
pub(crate) mod sink;
#[cfg(test)]
mod tests;
pub(crate) mod timing;

pub use access::{AccessPolicy, Visibility};
pub(crate) use bootstrap::*;
pub(crate) use builtin::*;
pub(crate) use command_output::*;
pub(crate) use config_explain::{
    config_explain_json, config_explain_result, config_value_to_json_for_key,
    explain_runtime_config, format_scope, is_sensitive_key, push_missing_config_key_messages,
    render_config_explain_text,
};
pub(crate) use facts::*;
pub use host::ErrorDetail;
pub use host::run_from;
pub(crate) use host::*;
pub(crate) use repl_lifecycle::rebuild_repl_in_place;
pub use runtime::{
    AppClients, AppRuntime, AuthState, ConfigState, LaunchContext, RuntimeContext, TerminalKind,
    UiState,
};
#[cfg(test)]
pub(crate) use session::AppStateInit;
pub use session::{
    AppSession, AppState, AppStateBuilder, DebugTimingBadge, DebugTimingState, LastFailure,
    ReplScopeFrame, ReplScopeStack,
};
pub use sink::{BufferedUiSink, StdIoUiSink, StderrSpinner, UiSink};

type ProductAction = dyn Fn(&mut AppRuntime) -> miette::Result<String> + Send + Sync;

/// Optional action-flag argument placed in the invocation configuration layer.
#[derive(Clone, Copy)]
pub struct ActionFlagValue {
    /// Argument name displayed in help.
    pub value_name: &'static str,
    /// Configuration key receiving a supplied argument.
    pub config_key: &'static str,
}

#[derive(Clone)]
struct ProductActionFlag {
    name: &'static str,
    help: &'static str,
    optional_value: Option<ActionFlagValue>,
    run: Arc<ProductAction>,
}

#[derive(Clone, Default)]
pub(crate) struct AppDefinition {
    native_commands: NativeCommandRegistry,
    product_defaults: ConfigLayer,
    product_help_options: Vec<(String, String)>,
    action_flags: Vec<ProductActionFlag>,
    config_flags: Vec<(&'static str, &'static str, &'static str, &'static str)>,
    access_policy: Option<Arc<dyn AccessPolicy>>,
}

impl AppDefinition {
    fn with_native_commands(mut self, native_commands: NativeCommandRegistry) -> Self {
        self.native_commands = native_commands;
        self
    }

    fn with_product_defaults(mut self, product_defaults: ConfigLayer) -> Self {
        self.product_defaults = product_defaults;
        self
    }

    fn with_product_help_option(
        mut self,
        syntax: impl Into<String>,
        help: impl Into<String>,
    ) -> Self {
        self.product_help_options.push((syntax.into(), help.into()));
        self
    }

    fn with_access_policy(mut self, policy: Arc<dyn AccessPolicy>) -> Self {
        self.access_policy = Some(policy);
        self
    }
}

#[derive(Clone, Default)]
/// Top-level application entrypoint for CLI and REPL execution.
///
/// Most embedders should start here or with [`AppBuilder`] instead of trying
/// to assemble runtime/session machinery directly.
#[must_use]
pub struct App {
    definition: AppDefinition,
}

impl App {
    /// Starts the canonical public builder for host construction.
    pub fn builder() -> AppBuilder {
        AppBuilder::default()
    }

    /// Creates an application with the default native command registry and no
    /// wrapper-owned defaults.
    pub fn new() -> Self {
        Self::builder().build()
    }

    /// Replaces the native command registry used for command dispatch.
    ///
    /// Use this when an embedder wants extra in-process commands to participate
    /// in the same command surface as the built-in host commands. When omitted,
    /// the application uses the crate's default native-command registry.
    ///
    /// This is the main extension seam for downstream product crates that wrap
    /// `osp-cli` and add site-specific native commands while keeping the rest
    /// of the host/runtime behavior unchanged.
    ///
    /// # Examples
    ///
    /// ```
    /// use anyhow::Result;
    /// use clap::Command;
    /// use osp_cli::app::BufferedUiSink;
    /// use osp_cli::{
    ///     App, NativeCommand, NativeCommandContext, NativeCommandOutcome, NativeCommandRegistry,
    /// };
    ///
    /// struct VersionCommand;
    ///
    /// impl NativeCommand for VersionCommand {
    ///     fn command(&self) -> Command {
    ///         Command::new("version").about("Show custom version")
    ///     }
    ///
    ///     fn execute(
    ///         &self,
    ///         _args: &[String],
    ///         _context: &NativeCommandContext<'_>,
    ///     ) -> Result<NativeCommandOutcome> {
    ///         Ok(NativeCommandOutcome::Exit(0))
    ///     }
    /// }
    ///
    /// let app = App::new().with_native_commands(
    ///     NativeCommandRegistry::new().with_command(VersionCommand),
    /// );
    /// let mut sink = BufferedUiSink::default();
    /// let exit = app.run_process_with_sink(["osp", "--help"], &mut sink);
    ///
    /// assert_eq!(exit, 0);
    /// assert!(sink.stdout.contains("version"));
    /// ```
    pub fn with_native_commands(mut self, native_commands: NativeCommandRegistry) -> Self {
        self.definition = self.definition.with_native_commands(native_commands);
        self
    }

    /// Replaces the product-owned defaults layered into runtime bootstrap.
    ///
    /// Use this when a wrapper crate owns extension keys such as
    /// `extensions.<site>.*` and wants them resolved through the normal host
    /// bootstrap path instead of maintaining a side-channel config helper.
    ///
    /// When omitted, the application uses only the built-in runtime defaults.
    pub fn with_product_defaults(mut self, product_defaults: ConfigLayer) -> Self {
        self.definition = self.definition.with_product_defaults(product_defaults);
        self
    }

    /// Adds one wrapper-owned option to the root help surface.
    ///
    /// Product wrappers may consume bootstrap options before the generic host
    /// parser runs. Registering them here keeps those real options visible
    /// without teaching the generic grammar product-specific behavior.
    pub fn with_product_help_option(
        mut self,
        syntax: impl Into<String>,
        help: impl Into<String>,
    ) -> Self {
        self.definition = self.definition.with_product_help_option(syntax, help);
        self
    }

    /// Registers an exclusive product action that exits after returning its message.
    ///
    /// An optional [`ActionFlagValue`] accepts zero or one value and places
    /// a supplied value in the invocation's config layer before policy startup.
    /// The action receives the assembled CLI runtime; it never enters the REPL.
    pub fn with_action_flag<F>(
        mut self,
        name: &'static str,
        help: &'static str,
        optional_value: Option<ActionFlagValue>,
        run: F,
    ) -> Self
    where
        F: Fn(&mut AppRuntime) -> miette::Result<String> + Send + Sync + 'static,
    {
        self.definition.action_flags.push(ProductActionFlag {
            name,
            help,
            optional_value,
            run: Arc::new(run),
        });
        self
    }

    /// Registers a root flag that sets one invocation-only config value.
    /// Flags targeting the same key are mutually exclusive.
    pub fn with_config_flag(
        mut self,
        name: &'static str,
        help: &'static str,
        key: &'static str,
        value: &'static str,
    ) -> Self {
        self.definition.config_flags.push((name, help, key, value));
        self
    }

    /// Installs product visibility, startup, acquisition and live session facts.
    ///
    /// ```
    /// use osp_cli::App;
    /// use osp_cli::app::access::{AccessPolicy, Visibility};
    /// struct ProductAccess;
    /// impl AccessPolicy for ProductAccess {
    ///     fn visibility(&self, path: &[String]) -> Visibility {
    ///         if path.first().is_some_and(|name| name == "internal") {
    ///             Visibility::Hidden(Some("Use the product's operator profile".into()))
    ///         } else {
    ///             Visibility::Shown
    ///         }
    ///     }
    /// }
    /// let app = App::new().with_access_policy(ProductAccess);
    /// ```
    pub fn with_access_policy<P: AccessPolicy + 'static>(mut self, policy: P) -> Self {
        self.definition = self.definition.with_access_policy(Arc::new(policy));
        self
    }

    /// Runs the application and returns a structured exit status.
    ///
    /// Use this when your caller wants ordinary Rust error handling instead of
    /// the process-style exit code conversion performed by
    /// [`App::run_process`].
    ///
    /// # Examples
    ///
    /// ```
    /// use osp_cli::App;
    ///
    /// let exit = App::new().run_from(["osp", "--help"])?;
    ///
    /// assert_eq!(exit, 0);
    /// # Ok::<(), miette::Report>(())
    /// ```
    pub fn run_from<I, T>(&self, args: I) -> miette::Result<i32>
    where
        I: IntoIterator<Item = T>,
        T: Into<OsString> + Clone,
    {
        crate::startup_trace::initialize();
        let result = host::run_from_with_sink_and_app(args, &mut StdIoUiSink, &self.definition);
        crate::startup_trace::finish();
        result
    }

    /// Binds the application to a specific UI sink for repeated invocations.
    ///
    /// Prefer this in tests, editor integrations, or foreign hosts that need
    /// the same host behavior as `osp` but want host-rendered one-shot output
    /// captured in a buffer. REPL input and prompts still use the process
    /// terminal. Each invocation creates fresh runtime and session state.
    ///
    /// # Examples
    ///
    /// ```
    /// use osp_cli::app::BufferedUiSink;
    /// use osp_cli::App;
    ///
    /// let mut sink = BufferedUiSink::default();
    /// let exit = App::new()
    ///     .with_sink(&mut sink)
    ///     .run_process(["osp", "--help"]);
    ///
    /// assert_eq!(exit, 0);
    /// assert!(!sink.stdout.is_empty());
    /// assert!(sink.stderr.is_empty());
    /// ```
    pub fn with_sink<'a>(self, sink: &'a mut dyn UiSink) -> AppRunner<'a> {
        AppRunner { app: self, sink }
    }

    /// Runs the application with the provided UI sink.
    ///
    /// Prefer this over [`App::with_sink`] when the caller only needs one
    /// invocation. Use [`App::with_sink`] and [`AppRunner`] when the same sink
    /// should be reused across multiple calls.
    /// REPL input and prompts still use the process terminal.
    ///
    /// # Examples
    ///
    /// ```
    /// use osp_cli::App;
    /// use osp_cli::app::BufferedUiSink;
    ///
    /// let mut sink = BufferedUiSink::default();
    /// let exit = App::new().run_with_sink(["osp", "--help"], &mut sink)?;
    ///
    /// assert_eq!(exit, 0);
    /// assert!(!sink.stdout.is_empty());
    /// assert!(sink.stderr.is_empty());
    /// # Ok::<(), miette::Report>(())
    /// ```
    pub fn run_with_sink<I, T>(&self, args: I, sink: &mut dyn UiSink) -> miette::Result<i32>
    where
        I: IntoIterator<Item = T>,
        T: Into<OsString> + Clone,
    {
        crate::startup_trace::initialize();
        let result = host::run_from_with_sink_and_app(args, sink, &self.definition);
        crate::startup_trace::finish();
        result
    }

    /// Runs the application and converts execution failures into process exit
    /// codes.
    ///
    /// Use this when the caller wants `osp`-style process behavior rather than
    /// structured error propagation. User-facing failures are rendered to the
    /// process stdio streams before the exit code is returned.
    ///
    /// # Examples
    ///
    /// ```
    /// use osp_cli::App;
    ///
    /// let exit = App::new().run_process(["osp", "--help"]);
    ///
    /// assert_eq!(exit, 0);
    /// ```
    pub fn run_process<I, T>(&self, args: I) -> i32
    where
        I: IntoIterator<Item = T>,
        T: Into<OsString> + Clone,
    {
        crate::startup_trace::initialize();
        let mut sink = StdIoUiSink;
        sink::take_stdout_write_failure();
        let code = self.run_process_with_sink(args, &mut sink);
        if sink::take_stdout_write_failure() && code == 0 {
            1
        } else {
            code
        }
    }

    /// Runs the application with the provided sink and returns a process exit
    /// code.
    ///
    /// This mirrors [`App::run_process`] and sends host-rendered one-shot output
    /// and user-facing errors through the supplied sink. Interactive REPL input
    /// and prompts still use the process terminal; native commands may also
    /// manage their own I/O.
    ///
    /// # Examples
    ///
    /// ```
    /// use osp_cli::App;
    /// use osp_cli::app::BufferedUiSink;
    ///
    /// let mut sink = BufferedUiSink::default();
    /// let exit = App::new().run_process_with_sink(["osp", "--help"], &mut sink);
    ///
    /// assert_eq!(exit, 0);
    /// assert!(!sink.stdout.is_empty());
    /// assert!(sink.stderr.is_empty());
    /// ```
    pub fn run_process_with_sink<I, T>(&self, args: I, sink: &mut dyn UiSink) -> i32
    where
        I: IntoIterator<Item = T>,
        T: Into<OsString> + Clone,
    {
        crate::startup_trace::initialize();
        let args = {
            let _phase =
                tracing::info_span!(target: "osp_startup", "process argv collection").entered();
            args.into_iter().map(Into::into).collect::<Vec<OsString>>()
        };
        let pipe = args.iter().position(|arg| arg == "|").unwrap_or(args.len());
        let mut scan = crate::cli::invocation::scan_cli_argv(&args[..pipe]);
        let result =
            host::run_scanned_with_sink_and_app(&args[pipe..], &mut scan, sink, &self.definition);
        let code = match result {
            Ok(code) => code,
            Err(err) => {
                let (error_detail, message_verbosity, error_render_settings) = {
                    let _phase = tracing::info_span!(target: "osp_startup", "process error display preparation").entered();
                    if let Some(ui) = host::error_display_settings(&err) {
                        (
                            ui.error_detail,
                            ui.message_verbosity,
                            ui.render_settings.clone(),
                        )
                    } else {
                        let (verbose, quiet) = (scan.invocation.verbose, scan.invocation.quiet);
                        let error_detail = adjust_error_detail(ErrorDetail::Terse, verbose, quiet);
                        let message_verbosity =
                            adjust_verbosity(MessageLevel::Success, verbose, quiet);
                        let error_render_settings = help::render_settings_for_help_with_scan(
                            &scan,
                            &self.definition.product_defaults,
                            &crate::config::ConfigLayer::default(),
                        )
                        .settings;
                        (error_detail, message_verbosity, error_render_settings)
                    }
                };
                let rendered = render_process_error(
                    &err,
                    error_detail,
                    &error_render_settings,
                    message_verbosity,
                );
                if error_render_settings.format == OutputFormat::Json {
                    sink.write_stdout(&rendered);
                } else {
                    sink.write_stderr(&rendered);
                }
                classify_exit_code(&err)
            }
        };
        crate::startup_trace::finish();
        code
    }
}

/// Reusable runner that keeps an [`App`] paired with a borrowed UI sink.
///
/// Prefer [`App::run_with_sink`] for one-shot calls. This type exists for
/// scoped reuse when the same sink should back multiple invocations.
/// Each invocation creates fresh runtime and session state; the runner does
/// not retain an interactive session between calls.
///
/// # Lifetime
///
/// `'a` is the lifetime of the mutable borrow of the sink passed to
/// [`App::with_sink`]. That means the runner cannot outlive the borrowed sink
/// and is mainly useful as a stack-scoped helper. It is not a good cross-thread
/// or async handoff type in its current form because it stores `&'a mut dyn
/// UiSink`.
///
/// # Examples
///
/// ```
/// use osp_cli::App;
/// use osp_cli::app::BufferedUiSink;
///
/// let mut sink = BufferedUiSink::default();
/// let mut runner = App::new().with_sink(&mut sink);
///
/// let first = runner.run_from(["osp", "--help"])?;
/// let second = runner.run_process(["osp", "--help"]);
///
/// assert_eq!(first, 0);
/// assert_eq!(second, 0);
/// assert!(!sink.stdout.is_empty());
/// assert!(sink.stderr.is_empty());
/// # Ok::<(), miette::Report>(())
/// ```
#[must_use = "AppRunner only has an effect when you call run_from or run_process on it"]
pub struct AppRunner<'a> {
    app: App,
    sink: &'a mut dyn UiSink,
}

impl<'a> AppRunner<'a> {
    /// Runs the application and returns a structured exit status.
    ///
    /// This is the bound-sink counterpart to [`App::run_with_sink`]. The
    /// borrowed sink stays attached so later calls on the same runner append to
    /// the same buffered or redirected output destination.
    pub fn run_from<I, T>(&mut self, args: I) -> miette::Result<i32>
    where
        I: IntoIterator<Item = T>,
        T: Into<OsString> + Clone,
    {
        self.app.run_with_sink(args, self.sink)
    }

    /// Runs the application and converts execution failures into process exit
    /// codes.
    ///
    /// This is the bound-sink counterpart to [`App::run_process_with_sink`].
    /// User-facing failures are rendered into the already-bound sink before the
    /// numeric exit code is returned.
    pub fn run_process<I, T>(&mut self, args: I) -> i32
    where
        I: IntoIterator<Item = T>,
        T: Into<OsString> + Clone,
    {
        self.app.run_process_with_sink(args, self.sink)
    }
}

#[derive(Clone, Default)]
/// Builder for configuring an [`App`] before construction.
///
/// This is the canonical public composition surface for host-level setup.
///
/// # Examples
///
/// Minimal embedder `main.rs`:
///
/// ```no_run
/// use osp_cli::App;
///
/// fn main() {
///     std::process::exit(
///         App::builder().build().run_process(std::env::args_os()),
///     );
/// }
/// ```
#[must_use]
pub struct AppBuilder {
    definition: AppDefinition,
}

impl AppBuilder {
    /// Replaces the native command registry used by the built application.
    ///
    /// This is the builder-friendly way to extend the host with extra native
    /// commands before calling [`AppBuilder::build`]. When omitted, the built
    /// app uses the crate's default native-command registry.
    ///
    /// This is the builder-side equivalent of [`App::with_native_commands`].
    /// Prefer it when a wrapper crate wants to finish command registration
    /// before deciding whether to build an owned [`App`] or a sink-bound
    /// [`AppRunner`].
    pub fn with_native_commands(mut self, native_commands: NativeCommandRegistry) -> Self {
        self.definition = self.definition.with_native_commands(native_commands);
        self
    }

    /// Replaces the product-owned defaults layered into runtime bootstrap.
    ///
    /// Wrapper crates should put site-owned keys under `extensions.<site>.*`
    /// and inject that layer here so native commands, `config get`, `config
    /// explain`, help rendering, and REPL rebuilds all see the same resolved
    /// config.
    ///
    /// If omitted, the built app uses only the crate's built-in runtime
    /// defaults.
    pub fn with_product_defaults(mut self, product_defaults: ConfigLayer) -> Self {
        self.definition = self.definition.with_product_defaults(product_defaults);
        self
    }

    /// Adds one wrapper-owned bootstrap option to the root help surface.
    pub fn with_product_help_option(
        mut self,
        syntax: impl Into<String>,
        help: impl Into<String>,
    ) -> Self {
        self.definition = self.definition.with_product_help_option(syntax, help);
        self
    }

    /// Registers an exclusive product action that exits after returning its message.
    ///
    /// An optional [`ActionFlagValue`] accepts zero or one value and places
    /// a supplied value in the invocation's config layer before policy startup.
    /// The action receives the assembled CLI runtime; it never enters the REPL.
    pub fn with_action_flag<F>(
        mut self,
        name: &'static str,
        help: &'static str,
        optional_value: Option<ActionFlagValue>,
        run: F,
    ) -> Self
    where
        F: Fn(&mut AppRuntime) -> miette::Result<String> + Send + Sync + 'static,
    {
        self.definition.action_flags.push(ProductActionFlag {
            name,
            help,
            optional_value,
            run: Arc::new(run),
        });
        self
    }

    /// Registers a root flag that sets one invocation-only config value.
    /// Flags targeting the same key are mutually exclusive.
    pub fn with_config_flag(
        mut self,
        name: &'static str,
        help: &'static str,
        key: &'static str,
        value: &'static str,
    ) -> Self {
        self.definition.config_flags.push((name, help, key, value));
        self
    }

    /// Installs product visibility, startup, acquisition and live session facts.
    pub fn with_access_policy<P: AccessPolicy + 'static>(mut self, policy: P) -> Self {
        self.definition = self.definition.with_access_policy(Arc::new(policy));
        self
    }

    /// Builds an [`App`] from the current builder state.
    ///
    /// Choose this when you want an owned application value that can be reused
    /// across many calls. Use [`AppBuilder::build_with_sink`] when binding the
    /// output sink is part of the setup.
    ///
    /// # Examples
    ///
    /// ```
    /// use osp_cli::App;
    ///
    /// let app = App::builder().build();
    /// let exit = app.run_process(["osp", "--help"]);
    ///
    /// assert_eq!(exit, 0);
    /// ```
    pub fn build(self) -> App {
        App {
            definition: self.definition,
        }
    }

    /// Builds an [`AppRunner`] bound to the provided UI sink.
    ///
    /// This is the shortest path for tests and embedders that want one sink
    /// binding plus the full host behavior.
    ///
    /// # Examples
    ///
    /// ```
    /// use osp_cli::App;
    /// use osp_cli::app::BufferedUiSink;
    ///
    /// let mut sink = BufferedUiSink::default();
    /// let exit = App::builder()
    ///     .build_with_sink(&mut sink)
    ///     .run_process(["osp", "--help"]);
    ///
    /// assert_eq!(exit, 0);
    /// assert!(!sink.stdout.is_empty());
    /// assert!(sink.stderr.is_empty());
    /// ```
    pub fn build_with_sink<'a>(self, sink: &'a mut dyn UiSink) -> AppRunner<'a> {
        self.build().with_sink(sink)
    }
}

/// Runs the default application instance and returns a process exit code.
///
/// This is shorthand for building [`App::new`] and calling
/// [`App::run_process`], using process stdio for rendered output and
/// user-facing errors.
///
/// # Examples
///
/// ```
/// let exit = osp_cli::app::run_process(["osp", "--help"]);
///
/// assert_eq!(exit, 0);
/// ```
pub fn run_process<I, T>(args: I) -> i32
where
    I: IntoIterator<Item = T>,
    T: Into<OsString> + Clone,
{
    crate::startup_trace::initialize();
    App::new().run_process(args)
}

/// Runs the default application instance with the provided sink.
///
/// This is shorthand for building [`App::new`] and calling
/// [`App::run_process_with_sink`].
///
/// # Examples
///
/// ```
/// use osp_cli::app::{BufferedUiSink, run_process_with_sink};
///
/// let mut sink = BufferedUiSink::default();
/// let exit = run_process_with_sink(["osp", "--help"], &mut sink);
///
/// assert_eq!(exit, 0);
/// assert!(!sink.stdout.is_empty());
/// assert!(sink.stderr.is_empty());
/// ```
pub fn run_process_with_sink<I, T>(args: I, sink: &mut dyn UiSink) -> i32
where
    I: IntoIterator<Item = T>,
    T: Into<OsString> + Clone,
{
    App::new().run_process_with_sink(args, sink)
}

pub(crate) fn render_process_error(
    err: &miette::Report,
    detail: ErrorDetail,
    settings: &crate::ui::RenderSettings,
    message_verbosity: MessageLevel,
) -> String {
    let message = render_report_message(err, detail, settings);
    if settings.format == OutputFormat::Json {
        let exit_code = classify_exit_code(err);
        let code = match exit_code {
            host::EXIT_CODE_USAGE => "usage_error",
            _ => "command_error",
        };
        return command_output::render_json_failure(
            command_output::json_failure_error(Some(err), code, message),
            exit_code,
            Vec::new(),
            serde_json::Value::Null,
        );
    }

    let mut messages = MessageBuffer::default();
    messages.push_titled(
        MessageLevel::Error,
        if classify_exit_code(err) == host::EXIT_CODE_USAGE {
            "Invalid command"
        } else {
            "Command failed"
        },
        crate::ui::visible_inline_text(&message),
    );
    render_messages(&messages, &{
        let mut request = MessageRequest::new(settings, message_verbosity);
        request.settings.chrome_frame = crate::ui::section_chrome::SectionFrameStyle::TopBottom;
        request.ruled_policy = crate::ui::section_chrome::RuledSectionPolicy::Shared;
        request.settings.width = request.settings.width.or(Some(12));
        request
    })
}
