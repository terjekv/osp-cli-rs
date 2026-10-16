//! Runtime-scoped host state shared across invocations.
//!
//! This module exists to hold the long-lived state that belongs to the running
//! host rather than to any single command submission.
//!
//! High-level flow:
//!
//! - capture startup-time runtime context such as terminal kind and profile
//!   override
//! - keep the current resolved config and derived UI/plugin state together
//! - expose one place where host code can read the active runtime snapshot
//!
//! Contract:
//!
//! - runtime state here is broader-lived than session/request state
//! - per-command or per-REPL-line details should not accumulate here unless
//!   they truly affect the whole running host
//!
//! Public API shape:
//!
//! - these types model host machinery, not lightweight semantic payloads
//! - constructors/accessors are the preferred way to create and inspect them
//! - callers usually receive [`AppRuntime`] and [`AppClients`] from host
//!   bootstrap rather than assembling them field-by-field

use std::collections::BTreeMap;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Instant;

use super::access::{AccessPolicy, Visibility};
use crate::config::{ConfigLayer, ResolvedConfig, RuntimeLoadOptions};
use crate::core::command_policy::{
    AccessReason, CommandAccess, CommandPath, SessionFacts, evaluate_auth,
};
use crate::core::plugin::DescribeCommandAuthV1;
use crate::native::NativeCommandRegistry;
use crate::plugin::PluginManager;
use crate::ui::RenderSettings;
use crate::ui::messages::MessageLevel;
use crate::ui::theme_catalog::ThemeCatalog;

/// Identifies which top-level host surface is currently active.
///
/// This lets config selection and runtime behavior distinguish between
/// one-shot CLI execution and the long-lived REPL host.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TerminalKind {
    /// One-shot command execution.
    Cli,
    /// Interactive REPL execution.
    Repl,
}

impl TerminalKind {
    /// Returns the config key fragment used for this terminal mode.
    ///
    /// # Examples
    ///
    /// ```
    /// use osp_cli::app::TerminalKind;
    ///
    /// assert_eq!(TerminalKind::Cli.as_config_terminal(), "cli");
    /// assert_eq!(TerminalKind::Repl.as_config_terminal(), "repl");
    /// ```
    pub fn as_config_terminal(self) -> &'static str {
        match self {
            TerminalKind::Cli => "cli",
            TerminalKind::Repl => "repl",
        }
    }
}

/// Startup-time selection inputs that shape runtime config resolution.
///
/// This keeps the profile override and terminal identity together so later
/// runtime rebuilds can resolve config against the same host context.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RuntimeContext {
    profile_override: Option<String>,
    terminal_kind: TerminalKind,
    pub(crate) repl_start: bool,
    terminal_env: Option<String>,
    authenticated_subject: Option<String>,
}

impl RuntimeContext {
    /// Creates a runtime context, normalizing the optional profile override.
    ///
    /// # Examples
    ///
    /// ```
    /// use osp_cli::app::{RuntimeContext, TerminalKind};
    ///
    /// let ctx = RuntimeContext::new(
    ///     Some("  Work  ".to_string()),
    ///     TerminalKind::Repl,
    ///     Some("xterm-256color".to_string()),
    /// );
    ///
    /// assert_eq!(ctx.profile_override(), Some("work"));
    /// assert_eq!(ctx.terminal_kind(), TerminalKind::Repl);
    /// assert_eq!(ctx.terminal_env(), Some("xterm-256color"));
    /// ```
    pub fn new(
        profile_override: Option<String>,
        terminal_kind: TerminalKind,
        terminal_env: Option<String>,
    ) -> Self {
        Self {
            profile_override: profile_override
                .map(|value| value.trim().to_ascii_lowercase())
                .filter(|value| !value.is_empty()),
            terminal_kind,
            repl_start: terminal_kind == TerminalKind::Repl,
            terminal_env,
            authenticated_subject: None,
        }
    }

    /// Returns the normalized profile override, if one was supplied.
    pub fn profile_override(&self) -> Option<&str> {
        self.profile_override.as_deref()
    }

    /// Returns the active terminal mode.
    pub fn terminal_kind(&self) -> TerminalKind {
        self.terminal_kind
    }

    /// Whether this process starts the interactive shell, rather than a one-shot
    /// command that merely uses REPL-scoped presentation configuration.
    pub fn is_repl(&self) -> bool {
        self.repl_start
    }

    /// Returns the detected terminal environment string, if available.
    pub fn terminal_env(&self) -> Option<&str> {
        self.terminal_env.as_deref()
    }

    /// Returns the subject of the credential currently projected into the
    /// running host, when the product has authenticated one.
    pub fn authenticated_subject(&self) -> Option<&str> {
        self.authenticated_subject.as_deref()
    }
}

/// Holds the current resolved config plus a monotonic in-memory revision.
///
/// The revision gives caches and rebuild logic a cheap way to notice when the
/// effective config actually changed.
pub struct ConfigState {
    resolved: ResolvedConfig,
    revision: u64,
}

impl ConfigState {
    /// Creates configuration state with an initial revision of `1`.
    ///
    /// # Examples
    ///
    /// ```
    /// use osp_cli::app::ConfigState;
    /// use osp_cli::config::{ConfigLayer, ConfigResolver, ResolveOptions};
    ///
    /// let mut defaults = ConfigLayer::default();
    /// defaults.set("profile.default", "default");
    /// defaults.set("theme.path", Vec::<String>::new());
    ///
    /// let mut resolver = ConfigResolver::default();
    /// resolver.set_defaults(defaults);
    /// let resolved = resolver.resolve(ResolveOptions::default()).unwrap();
    ///
    /// let mut state = ConfigState::new(resolved.clone());
    /// assert_eq!(state.revision(), 1);
    /// assert!(!state.replace_resolved(resolved));
    /// assert_eq!(state.revision(), 1);
    /// ```
    pub fn new(resolved: ResolvedConfig) -> Self {
        Self {
            resolved,
            revision: 1,
        }
    }

    /// Returns the current resolved configuration snapshot.
    pub fn resolved(&self) -> &ResolvedConfig {
        &self.resolved
    }

    /// Returns the current configuration revision.
    pub fn revision(&self) -> u64 {
        self.revision
    }

    /// Replaces the resolved configuration and bumps the revision when it changes.
    pub fn replace_resolved(&mut self, next: ResolvedConfig) -> bool {
        if self.resolved == next {
            return false;
        }

        self.resolved = next;
        self.revision += 1;
        true
    }

    /// Applies a configuration transform atomically against the current snapshot.
    pub fn transaction<F, E>(&mut self, mutator: F) -> Result<bool, E>
    where
        F: FnOnce(&ResolvedConfig) -> Result<ResolvedConfig, E>,
    {
        let current = self.resolved.clone();
        let candidate = mutator(&current)?;
        Ok(self.replace_resolved(candidate))
    }
}

/// Derived presentation/runtime state for the active config snapshot.
///
/// This is cached host state, not a second source of truth. Recompute it when
/// the resolved config changes so renderers and message surfaces all read the
/// same derived values.
#[derive(Debug, Clone)]
#[non_exhaustive]
#[must_use]
pub struct UiState {
    /// Render settings derived from the current config snapshot.
    pub render_settings: RenderSettings,
    /// Default message verbosity derived from the current runtime config.
    pub verbosity: MessageLevel,
    /// Numeric debug verbosity used for trace-style host output.
    pub debug_level: u8,
}

impl UiState {
    /// Derives UI state from a resolved config snapshot and runtime context.
    ///
    /// # Examples
    ///
    /// ```
    /// use osp_cli::app::{RuntimeContext, TerminalKind, UiState};
    /// use osp_cli::config::{ConfigLayer, ConfigResolver, ResolveOptions};
    ///
    /// let mut defaults = ConfigLayer::default();
    /// defaults.set("profile.default", "default");
    /// defaults.set("theme.path", Vec::<String>::new());
    /// defaults.set("ui.verbosity", "info");
    ///
    /// let mut resolver = ConfigResolver::default();
    /// resolver.set_defaults(defaults);
    /// let config = resolver.resolve(ResolveOptions::new().with_terminal("cli")).unwrap();
    ///
    /// let ui = UiState::from_resolved_config(
    ///     &RuntimeContext::new(None, TerminalKind::Cli, Some("xterm-256color".to_string())),
    ///     &config,
    /// )
    /// .unwrap();
    ///
    /// assert_eq!(ui.verbosity.as_str(), "info");
    /// assert_eq!(ui.render_settings.runtime.terminal.as_deref(), Some("xterm-256color"));
    /// ```
    pub fn from_resolved_config(
        context: &RuntimeContext,
        config: &ResolvedConfig,
    ) -> miette::Result<Self> {
        super::assembly::build_host(super::assembly::HostBuildRequest::Presentation {
            context,
            config: Some(config),
            seed: super::assembly::RenderSettingsSeed::DefaultAuto,
            theme: None,
            tolerant: false,
        })
        .map(|built| built.presentation().0)
    }

    /// Creates the UI state snapshot used for one resolved config revision.
    ///
    /// # Examples
    ///
    /// ```
    /// use osp_cli::app::UiState;
    /// use osp_cli::ui::RenderSettings;
    /// use osp_cli::ui::messages::MessageLevel;
    /// use osp_cli::core::output::OutputFormat;
    ///
    /// let ui = UiState::new(
    ///     RenderSettings::test_plain(OutputFormat::Json),
    ///     MessageLevel::Success,
    ///     2,
    /// );
    ///
    /// assert_eq!(ui.verbosity, MessageLevel::Success);
    /// assert_eq!(ui.debug_level, 2);
    /// ```
    pub fn new(render_settings: RenderSettings, verbosity: MessageLevel, debug_level: u8) -> Self {
        Self {
            render_settings,
            verbosity,

            debug_level,
        }
    }
}

/// Startup inputs used to assemble runtime services and locate on-disk state.
///
/// This is launch-time provenance for the running host. It is kept separate
/// from [`RuntimeContext`] because callers may need to rebuild caches or plugin
/// services from the same startup inputs after config changes.
#[derive(Debug, Clone)]
#[non_exhaustive]
#[must_use]
pub struct LaunchContext {
    /// Explicit plugin directories requested by the caller at launch time.
    pub plugin_dirs: Vec<PathBuf>,
    /// Optional config-root override for runtime config discovery.
    pub config_root: Option<PathBuf>,
    /// Optional cache-root override for runtime state and caches.
    pub cache_root: Option<PathBuf>,
    /// Flags controlling which runtime config sources are consulted.
    pub runtime_load: RuntimeLoadOptions,
    /// Timestamp captured before startup work begins.
    pub startup_started_at: Instant,
}

impl LaunchContext {
    /// Creates launch-time provenance for one host bootstrap attempt.
    pub fn new(
        plugin_dirs: Vec<PathBuf>,
        config_root: Option<PathBuf>,
        cache_root: Option<PathBuf>,
        runtime_load: RuntimeLoadOptions,
    ) -> Self {
        Self {
            plugin_dirs,
            config_root,
            cache_root,
            runtime_load,
            startup_started_at: Instant::now(),
        }
    }

    /// Appends one explicit plugin directory.
    pub fn with_plugin_dir(mut self, plugin_dir: impl Into<PathBuf>) -> Self {
        self.plugin_dirs.push(plugin_dir.into());
        self
    }

    /// Replaces the explicit plugin directory list.
    pub fn with_plugin_dirs(mut self, plugin_dirs: impl IntoIterator<Item = PathBuf>) -> Self {
        self.plugin_dirs = plugin_dirs.into_iter().collect();
        self
    }

    /// Sets the config root override.
    pub fn with_config_root(mut self, config_root: Option<PathBuf>) -> Self {
        self.config_root = config_root;
        self
    }

    /// Sets the cache root override.
    pub fn with_cache_root(mut self, cache_root: Option<PathBuf>) -> Self {
        self.cache_root = cache_root;
        self
    }

    /// Replaces the runtime-load flags carried by the launch context.
    pub fn with_runtime_load(mut self, runtime_load: RuntimeLoadOptions) -> Self {
        self.runtime_load = runtime_load;
        self
    }

    /// Replaces the captured startup timestamp.
    pub fn with_startup_started_at(mut self, startup_started_at: Instant) -> Self {
        self.startup_started_at = startup_started_at;
        self
    }
}

impl Default for LaunchContext {
    fn default() -> Self {
        Self::new(Vec::new(), None, None, RuntimeLoadOptions::default())
    }
}

/// Long-lived client registries shared across command execution.
///
/// This bundles expensive or stateful clients so they do not have to be
/// recreated on every command dispatch.
///
/// Public API note: this is intentionally constructor/accessor driven. The
/// internal registries stay private so the host can grow additional cached
/// machinery without breaking callers.
#[non_exhaustive]
#[must_use]
pub struct AppClients {
    /// Plugin manager used for discovery, dispatch, and provider metadata.
    plugins: PluginManager,
    /// In-process registry of native commands.
    native_commands: NativeCommandRegistry,
}

impl AppClients {
    /// Creates the shared client registry used by the application.
    pub fn new(plugins: PluginManager, native_commands: NativeCommandRegistry) -> Self {
        Self {
            plugins,
            native_commands,
        }
    }

    /// Returns the shared plugin manager.
    pub fn plugins(&self) -> &PluginManager {
        &self.plugins
    }

    /// Returns the shared registry of native commands.
    pub fn native_commands(&self) -> &NativeCommandRegistry {
        &self.native_commands
    }
}

/// Runtime-scoped application state shared across commands.
///
/// This is the assembled host snapshot that command and REPL code read while
/// the process is running. The fields here are intended to move together: when
/// config changes, callers should rebuild the derived UI/auth/theme state
/// rather than mixing old and new snapshots.
///
/// Public API note: this is a host snapshot you usually receive from app
/// bootstrap, not a semantic DTO meant for arbitrary external construction.
#[non_exhaustive]
pub struct AppRuntime {
    /// Startup-time runtime identity used for config selection and rebuilds.
    pub(crate) context: RuntimeContext,
    /// Authoritative resolved config snapshot and its in-memory revision.
    pub(crate) config: ConfigState,
    /// UI-facing state derived from the current resolved config.
    pub(crate) ui: UiState,
    /// Authorization and command-visibility policy state derived from config.
    pub(crate) auth: AuthState,
    pub(crate) themes: ThemeCatalog,
    /// Launch-time inputs used to assemble caches and external services.
    pub(crate) launch: LaunchContext,
    pub(crate) definition: Arc<super::AppDefinition>,
}

impl AppRuntime {
    /// Creates the runtime snapshot shared across CLI and REPL execution.
    pub(crate) fn new(
        context: RuntimeContext,
        config: ConfigState,
        ui: UiState,
        auth: AuthState,
        themes: ThemeCatalog,
        launch: LaunchContext,
    ) -> Self {
        Self {
            context,
            config,
            ui,
            auth,
            themes,
            launch,
            definition: Arc::default(),
        }
    }

    /// Returns the runtime context used for config selection and rebuilds.
    pub fn context(&self) -> &RuntimeContext {
        &self.context
    }

    /// Returns the authoritative resolved-config state.
    pub fn config_state(&self) -> &ConfigState {
        &self.config
    }

    /// Returns the UI state derived from the current config snapshot.
    pub fn ui(&self) -> &UiState {
        &self.ui
    }

    /// Returns the command-visibility/auth state.
    pub fn auth(&self) -> &AuthState {
        &self.auth
    }

    /// Projects the authenticated subject into the host context used by
    /// prompts and product defaults without re-resolving invocation config.
    pub fn set_authenticated_subject(&mut self, subject: Option<&str>) {
        self.context.authenticated_subject = subject
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(ToOwned::to_owned);
    }

    /// Returns the launch-time provenance used to assemble the runtime.
    pub fn launch(&self) -> &LaunchContext {
        &self.launch
    }

    pub(crate) fn product_defaults(&self) -> &ConfigLayer {
        &self.definition.product_defaults
    }

    pub(crate) fn access_policy(&self) -> Option<Arc<dyn AccessPolicy>> {
        self.auth.access_policy.clone()
    }

    pub(crate) fn set_access_policy(&mut self, policy: Option<Arc<dyn AccessPolicy>>) {
        Arc::make_mut(&mut self.definition).access_policy = policy.clone();
        self.auth
            .set_access_policy(policy, Some(self.config.resolved()));
        if self.auth.access_policy.is_some() && !self.auth.session_facts().authenticated {
            self.set_authenticated_subject(None);
        }
    }
}

/// Declared external authentication and the shared product access hook.
#[derive(Default)]
pub struct AuthState {
    access_policy: Option<Arc<dyn AccessPolicy>>,
    command_auth: BTreeMap<CommandPath, DescribeCommandAuthV1>,
}

impl AuthState {
    pub(crate) fn for_policy(
        policy: Option<Arc<dyn AccessPolicy>>,
        config: Option<&ResolvedConfig>,
    ) -> Self {
        let mut auth = Self::default();
        auth.set_access_policy(policy, config);
        auth
    }

    fn set_access_policy(
        &mut self,
        policy: Option<Arc<dyn AccessPolicy>>,
        config: Option<&ResolvedConfig>,
    ) {
        if let (Some(policy), Some(config)) = (&policy, config) {
            policy.configure(config);
        }
        self.access_policy = policy;
    }

    pub(crate) fn from_commands(
        plugins: &PluginManager,
        native_commands: &NativeCommandRegistry,
    ) -> Self {
        let builtins = crate::cli::builtin_command_names();
        let mut command_auth = plugins.command_auth();
        command_auth.retain(|path, _| {
            path.as_slice().first().is_none_or(|root| {
                !builtins.contains(root) && native_commands.command(root).is_none()
            })
        });
        command_auth.extend(native_commands.command_auth());
        Self {
            command_auth,
            access_policy: None,
        }
    }

    /// Reads live product facts without acquiring credentials.
    pub fn session_facts(&self) -> SessionFacts {
        self.access_policy
            .as_ref()
            .map(|policy| policy.session_facts())
            .unwrap_or_default()
    }
    pub(crate) fn refresh_native_auth(
        &mut self,
        description: &crate::core::plugin::DescribeCommandV1,
    ) {
        let root = description.name.trim().to_ascii_lowercase();
        self.command_auth
            .retain(|path, _| path.as_slice().first() != Some(&root));
        crate::plugin::catalog::collect_command_auth(&mut self.command_auth, description, &[]);
    }

    /// Evaluates an external command root.
    pub fn external_command_access(&self, command: &str) -> CommandAccess {
        self.external_command_path_access(&CommandPath::new([command]))
    }

    /// Combines product visibility with declared command auth for all readers.
    pub fn external_command_path_access(&self, path: &CommandPath) -> CommandAccess {
        self.external_command_access_with_auth(path, self.command_auth.get(path))
    }

    pub(crate) fn external_command_access_with_auth(
        &self,
        path: &CommandPath,
        auth: Option<&DescribeCommandAuthV1>,
    ) -> CommandAccess {
        // A hidden parent hides its descendants, including direct dispatch.
        if let Some(policy) = &self.access_policy {
            for end in 1..=path.as_slice().len() {
                if let Visibility::Hidden(message) = policy.visibility(&path.as_slice()[..end]) {
                    return CommandAccess::Hidden(AccessReason::HiddenByProduct(message));
                }
            }
        }
        evaluate_auth(auth, &self.session_facts())
    }

    pub(crate) fn filter_help_document(
        &self,
        document: &mut crate::core::document::Document,
        path: &[String],
    ) {
        fn filter(
            auth: &AuthState,
            blocks: &mut [crate::core::document::DocBlock],
            path: &[String],
            commands: bool,
        ) {
            use crate::core::document::DocBlock;
            for block in blocks {
                match block {
                    DocBlock::Section { title, blocks, .. } => {
                        filter(auth, blocks, path, title.eq_ignore_ascii_case("commands"))
                    }
                    DocBlock::Entries { entries } if commands => entries.retain(|entry| {
                        let Some(name) = entry.name.split_whitespace().next() else {
                            return true;
                        };
                        let child = CommandPath::new(
                            path.iter()
                                .cloned()
                                .chain(std::iter::once(name.to_string())),
                        );
                        auth.external_command_path_access(&child).is_visible()
                    }),
                    _ => {}
                }
            }
        }
        filter(self, &mut document.blocks, path, false);
    }

    pub(crate) fn visible_help_command(
        &self,
        command: clap::Command,
        prefix: &[String],
    ) -> clap::Command {
        let mut path = prefix.to_vec();
        path.push(command.get_name().to_string());
        command.mut_subcommands(|child| {
            let child_path = CommandPath::new(
                path.iter()
                    .cloned()
                    .chain(std::iter::once(child.get_name().to_string())),
            );
            let hidden =
                child.is_hide_set() || !self.external_command_path_access(&child_path).is_visible();
            self.visible_help_command(child, &path).hide(hidden)
        })
    }

    pub(crate) fn filter_command_spec(
        &self,
        command: &mut crate::completion::CompletionNode,
        prefix: &[String],
    ) {
        let mut path = prefix.to_vec();
        path.push(command.name.clone());
        command.children.retain(|_, child| {
            let path = CommandPath::new(
                path.iter()
                    .cloned()
                    .chain(std::iter::once(child.name.clone())),
            );
            self.external_command_path_access(&path).is_visible()
        });
        for child in command.children.values_mut() {
            self.filter_command_spec(child, &path);
        }
    }

    /// Whether an external root is offered by the combined access decision.
    pub fn is_external_command_visible(&self, command: &str) -> bool {
        self.external_command_access(command).is_visible()
    }
}

#[cfg(test)]
mod tests {
    use super::ConfigState;
    use crate::config::{ConfigLayer, ConfigResolver, LoadedLayers, ResolveOptions};
    fn resolved_with(entries: &[(&str, &str)]) -> crate::config::ResolvedConfig {
        let mut file = ConfigLayer::default();
        for (key, value) in entries {
            file.set(*key, (*value).to_string());
        }
        ConfigResolver::from_loaded_layers_with_schema(
            LoadedLayers {
                file,
                ..LoadedLayers::default()
            },
            crate::config::ConfigSchema::default(),
        )
        .resolve(ResolveOptions::default())
        .expect("config should resolve")
    }

    #[test]
    fn runtime_context_normalizes_inputs() {
        let context = super::RuntimeContext::new(
            Some("  Dev  ".to_string()),
            super::TerminalKind::Repl,
            Some("xterm-256color".to_string()),
        );
        assert_eq!(context.profile_override(), Some("dev"));
        assert_eq!(context.terminal_kind(), super::TerminalKind::Repl);
        assert_eq!(context.terminal_env(), Some("xterm-256color"));
    }
    #[test]
    fn config_state_tracks_noops_changes_and_transaction_errors() {
        let resolved = resolved_with(&[]);
        let mut state = ConfigState::new(resolved.clone());
        assert_eq!(state.revision(), 1);
        assert!(!state.replace_resolved(resolved.clone()));
        assert_eq!(state.revision(), 1);

        let changed = resolved_with(&[("ui.format", "json")]);
        assert!(state.replace_resolved(changed));
        assert_eq!(state.revision(), 2);

        let changed = state
            .transaction(|current| {
                let _ = current;
                Ok::<_, &'static str>(resolved_with(&[("ui.format", "mreg")]))
            })
            .expect("transaction should succeed");
        assert!(changed);
        assert_eq!(state.revision(), 3);

        let err = state
            .transaction(|_| Err::<crate::config::ResolvedConfig, _>("boom"))
            .expect_err("transaction error should propagate");
        assert_eq!(err, "boom");
        assert_eq!(state.revision(), 3);
    }
}
