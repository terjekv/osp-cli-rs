//! Host construction for executable state and config-derived presentation.
//!
//! Tolerant help requests do not create sessions or discover plugins. Full
//! requests share assembly while retaining startup and reload failure policies.
//! Startup hooks remain owned by the host; reload logging follows assembly.

use miette::Result;

use crate::config::ResolvedConfig;
use crate::plugin::PluginManager;
use crate::plugin::state::PluginCommandPreferences;
use crate::ui::RenderSettings;
use crate::ui::theme::DEFAULT_THEME_NAME;
use crate::ui::theme_catalog::ThemeCatalog;

use super::{
    AppSession, LaunchContext, RuntimeContext, UiState, build_logging_config, build_render_runtime,
    debug_level_from_config, plugin_path_discovery_enabled, plugin_process_timeout,
    resolve_known_theme_name, verbosity_from_config,
};

/// Render-settings baseline to use when deriving host-facing UI state.
pub(crate) enum RenderSettingsSeed {
    /// Start from the default auto-render baseline.
    DefaultAuto,
    /// Start from an existing settings baseline and layer config onto it.
    Existing(Box<RenderSettings>),
}

impl RenderSettingsSeed {
    fn into_settings(self, context: &RuntimeContext) -> RenderSettings {
        match self {
            Self::DefaultAuto => {
                let mut settings = RenderSettings {
                    format: crate::core::output::OutputFormat::Auto,
                    ..RenderSettings::default()
                };
                settings.runtime = build_render_runtime(context.terminal_env());
                settings
            }
            Self::Existing(mut settings) => {
                // Rebuild paths must preserve the editor/runtime facts already
                // observed for this host, otherwise a restart can silently
                // lose TTY/Unicode/color capability state.
                if settings.runtime.terminal.is_none() {
                    settings.runtime.terminal = context.terminal_env().map(str::to_owned);
                }
                *settings
            }
        }
    }
}

/// Host construction requests keep tolerant help separate from executable state.
pub(crate) enum HostBuildRequest<'a> {
    Startup {
        cli: &'a crate::cli::Cli,
        action: &'a super::RunAction,
        profile_override: Option<String>,
        terminal_kind: super::TerminalKind,
        started: std::time::Instant,
        definition: std::sync::Arc<super::AppDefinition>,
    },
    Rebuild(&'a super::AppState),
    State(Box<super::AppStateBuilder>),
    Presentation {
        context: &'a RuntimeContext,
        config: Option<&'a ResolvedConfig>,
        seed: RenderSettingsSeed,
        theme: Option<&'a str>,
        tolerant: bool,
    },
}

pub(crate) enum HostBuild {
    State(Box<super::AppState>),
    Presentation(Box<(UiState, ThemeCatalog)>),
}

impl HostBuild {
    pub(crate) fn state(self) -> super::AppState {
        match self {
            Self::State(state) => *state,
            Self::Presentation(..) => unreachable!("state request returns state"),
        }
    }

    pub(crate) fn presentation(self) -> (UiState, ThemeCatalog) {
        match self {
            Self::Presentation(parts) => *parts,
            Self::State(_) => unreachable!("presentation request returns presentation"),
        }
    }
}

/// Constructs host snapshots while leaving startup hooks to the caller.
pub(crate) fn build_host(request: HostBuildRequest<'_>) -> Result<HostBuild> {
    use super::{AppStateBuilder, RuntimeConfigRequest, resolve_runtime_config};
    use miette::WrapErr;
    let startup = matches!(&request, HostBuildRequest::Startup { .. });
    let builder = match request {
        HostBuildRequest::Presentation {
            context,
            config,
            seed,
            theme,
            tolerant,
        } => {
            let Some(config) = config else {
                let settings = RenderSettings {
                    runtime: build_render_runtime(context.terminal_env()),
                    ..RenderSettings::default()
                };
                return Ok(HostBuild::Presentation(Box::new((
                    UiState::new(settings, Default::default(), 0),
                    ThemeCatalog::default(),
                ))));
            };
            let themes = crate::ui::theme_catalog::load_theme_catalog(config);
            let settings = render_settings(context, config, &themes, seed, theme, tolerant)?;
            return Ok(HostBuild::Presentation(Box::new((
                UiState::new(
                    settings,
                    verbosity_from_config(config),
                    debug_level_from_config(config),
                ),
                themes,
            ))));
        }
        HostBuildRequest::State(builder) => *builder,
        HostBuildRequest::Startup {
            cli,
            action,
            profile_override,
            terminal_kind,
            started,
            definition,
        } => {
            let _inputs =
                tracing::info_span!(target: "osp_startup", "runtime config and host inputs")
                    .entered();
            let mut context = super::build_runtime_context(profile_override, terminal_kind);
            context.repl_start = matches!(action, super::RunAction::Repl);
            let session_layer = super::build_cli_session_layer(cli);
            let launch = LaunchContext::default()
                .with_plugin_dirs(cli.plugin_dirs.clone())
                .with_runtime_load(cli.runtime_load_options())
                .with_startup_started_at(started);
            let request = RuntimeConfigRequest::new(
                context.profile_override().map(ToOwned::to_owned),
                Some(context.terminal_kind().as_config_terminal()),
            )
            .with_runtime_load(launch.runtime_load)
            .with_product_defaults(definition.product_defaults.clone())
            .with_session_layer(session_layer.clone());
            let config = if action.uses_raw_config() {
                super::bootstrap::resolve_raw_runtime_config(request)
            } else {
                resolve_runtime_config(request)
            }
            .map_err(|err| {
                super::host::report_report_with_context(
                    err,
                    super::host::ErrorContext::Operation(
                        "failed to resolve config with session layer",
                    ),
                )
            })?;
            let themes = crate::ui::theme_catalog::load_theme_catalog(&config);
            let plugins = build_plugin_manager(&config, &launch, None);
            let ui = derive_ui_state(
                &context,
                &config,
                &themes,
                RenderSettingsSeed::DefaultAuto,
                None,
            )
            .map_err(|err| {
                super::host::report_report_with_context(
                    err,
                    super::host::ErrorContext::Operation(
                        "failed to derive host runtime inputs for startup",
                    ),
                )
            })?;
            AppStateBuilder {
                context,
                config,
                ui,
                launch,
                plugins: Some(plugins),
                native_commands: definition.native_commands.clone(),
                session: Some(
                    AppSession::default().with_config_overrides(session_layer.unwrap_or_default()),
                ),
                themes: Some(themes),
                definition,
            }
        }
        HostBuildRequest::Rebuild(current) => {
            let runtime = &current.runtime;
            tracing::debug!(profile_override = ?runtime.context.profile_override(), scoped = !current.session.scope.is_root(), "rebuilding REPL state after config/theme change");
            let session_layer = (!current.session.config_overrides.entries().is_empty())
                .then(|| current.session.config_overrides.clone());
            let config = resolve_runtime_config(
                RuntimeConfigRequest::new(
                    runtime.context.profile_override().map(ToOwned::to_owned),
                    Some(runtime.context.terminal_kind().as_config_terminal()),
                )
                .with_runtime_load(runtime.launch.runtime_load)
                .with_product_defaults(runtime.product_defaults().clone())
                .with_session_layer(session_layer),
            )
            .wrap_err("failed to resolve config for REPL rebuild")?;
            let mut settings = runtime.ui.render_settings.clone();
            if !settings.format_explicit && config.get_string("ui.format").is_none() {
                settings.format = crate::core::output::OutputFormat::Auto;
            }
            let preferences = current
                .clients
                .plugins()
                .command_preferences_snapshot()
                .rebased(
                    &PluginCommandPreferences::from_resolved(runtime.config.resolved()),
                    PluginCommandPreferences::from_resolved(&config),
                );
            let themes = crate::ui::theme_catalog::load_theme_catalog(&config);
            let plugins = build_plugin_manager(&config, &runtime.launch, Some(&preferences));
            let ui = derive_ui_state(
                &runtime.context,
                &config,
                &themes,
                RenderSettingsSeed::Existing(Box::new(settings)),
                None,
            )
            .wrap_err("failed to derive host runtime inputs for REPL rebuild")?;
            AppStateBuilder {
                context: runtime.context.clone(),
                config,
                ui,
                launch: runtime.launch.clone(),
                plugins: Some(plugins),
                native_commands: current.clients.native_commands().clone(),
                session: Some(current.session.clone()),
                themes: Some(themes),
                definition: runtime.definition.clone(),
            }
        }
    };
    let super::AppStateBuilder {
        context,
        config,
        ui,
        launch,
        plugins,
        native_commands,
        session,
        themes,
        definition,
    } = builder;
    let assembly_span = startup
        .then(|| tracing::info_span!(target: "osp_startup", "host state assembly").entered());
    let should_log_theme_issues = themes.is_none();
    let themes = themes.unwrap_or_else(|| crate::ui::theme_catalog::load_theme_catalog(&config));
    let plugins = plugins.unwrap_or_else(|| build_plugin_manager(&config, &launch, None));
    if should_log_theme_issues {
        crate::ui::theme_catalog::log_theme_issues(&themes.issues);
    }
    {
        let _phase =
            tracing::info_span!(target: "osp_startup", "native command configure").entered();
        native_commands.configure(&config);
    }
    let clients = super::AppClients::new(plugins, native_commands);
    let auth = {
        let _phase =
            tracing::info_span!(target: "osp_startup", "auth policy catalog setup").entered();
        super::AuthState::from_commands(clients.plugins(), clients.native_commands())
    };
    let mut runtime = super::AppRuntime::new(
        context,
        super::ConfigState::new(config),
        ui,
        auth,
        themes,
        launch,
    );
    runtime.definition = definition;
    drop(assembly_span);
    {
        let _phase =
            tracing::info_span!(target: "osp_startup", "auth policy context setup").entered();
        runtime.set_access_policy(runtime.definition.access_policy.clone());
    }
    Ok(HostBuild::State(Box::new(super::AppState {
        runtime,
        session: session.unwrap_or_default(),
        clients,
    })))
}

/// Derives UI state from resolved config and a settings baseline.
pub(crate) fn derive_render_settings(
    context: &RuntimeContext,
    config: &ResolvedConfig,
    themes: &ThemeCatalog,
    render_seed: RenderSettingsSeed,
    theme_name_override: Option<&str>,
) -> Result<RenderSettings> {
    render_settings(
        context,
        config,
        themes,
        render_seed,
        theme_name_override,
        false,
    )
}

#[cfg(test)]
pub(crate) fn derive_render_settings_or_fallback(
    context: &RuntimeContext,
    config: &ResolvedConfig,
    themes: &ThemeCatalog,
    render_seed: RenderSettingsSeed,
    theme_name_override: Option<&str>,
) -> RenderSettings {
    render_settings(
        context,
        config,
        themes,
        render_seed,
        theme_name_override,
        true,
    )
    .expect("fallback theme is available")
}

fn render_settings(
    context: &RuntimeContext,
    config: &ResolvedConfig,
    themes: &ThemeCatalog,
    seed: RenderSettingsSeed,
    theme: Option<&str>,
    tolerant: bool,
) -> Result<RenderSettings> {
    let mut settings = derive_base_render_settings(context, config, seed);
    let selected = resolve_known_theme_name(selected_theme_name(config, theme), themes);
    settings.theme_name = if tolerant {
        selected.unwrap_or_else(|_| DEFAULT_THEME_NAME.to_string())
    } else {
        selected?
    };
    settings.theme = themes
        .resolve(&settings.theme_name)
        .map(|entry| entry.theme.clone());
    Ok(settings)
}

fn derive_base_render_settings(
    context: &RuntimeContext,
    config: &ResolvedConfig,
    render_seed: RenderSettingsSeed,
) -> RenderSettings {
    let mut render_settings = render_seed.into_settings(context);
    crate::ui::settings::apply_render_config_overrides(&mut render_settings, config);
    render_settings
}

fn selected_theme_name<'a>(
    config: &'a ResolvedConfig,
    theme_name_override: Option<&'a str>,
) -> &'a str {
    theme_name_override
        .or_else(|| config.get_string("theme.name"))
        .unwrap_or(DEFAULT_THEME_NAME)
}

/// Derives UI state from resolved config and a settings baseline.
pub(crate) fn derive_ui_state(
    context: &RuntimeContext,
    config: &ResolvedConfig,
    themes: &ThemeCatalog,
    render_seed: RenderSettingsSeed,
    theme_name_override: Option<&str>,
) -> Result<UiState> {
    let render_settings =
        derive_render_settings(context, config, themes, render_seed, theme_name_override)?;

    Ok(UiState::new(
        render_settings,
        verbosity_from_config(config),
        debug_level_from_config(config),
    ))
}

/// Builds the config-derived plugin manager for one launch context.
pub(crate) fn build_plugin_manager(
    config: &ResolvedConfig,
    launch: &LaunchContext,
    preferences_override: Option<&PluginCommandPreferences>,
) -> PluginManager {
    let manager = PluginManager::new(launch.plugin_dirs.clone())
        .with_roots(launch.config_root.clone(), launch.cache_root.clone())
        .with_default_roots(!launch.runtime_load.is_defaults_only())
        .with_bundled_roots(!launch.runtime_load.is_defaults_only())
        .with_process_timeout(plugin_process_timeout(config))
        .with_path_discovery(plugin_path_discovery_enabled(config))
        .with_command_preferences(
            crate::plugin::state::PluginCommandPreferences::from_resolved(config),
        );
    if let Some(preferences) = preferences_override {
        manager.replace_command_preferences(preferences.clone());
    }
    manager
}

/// Applies side effects associated with an already-derived runtime snapshot.
pub(crate) fn apply_runtime_side_effects(
    config: &ResolvedConfig,
    debug_level: u8,
    themes: &ThemeCatalog,
) {
    super::logging::init_developer_logging(build_logging_config(config, debug_level));
    crate::ui::theme_catalog::log_theme_issues(&themes.issues);
}

pub(crate) fn rebuild_repl_in_place(state: &mut super::AppState) -> Result<()> {
    *state = rebuild_repl_state(state)?;
    Ok(())
}

pub(crate) fn rebuild_repl_state(current: &super::AppState) -> Result<super::AppState> {
    let next = build_host(HostBuildRequest::Rebuild(current))?.state();
    next.session.sync_history_shell_context();
    apply_runtime_side_effects(
        next.runtime.config.resolved(),
        next.runtime.ui.debug_level,
        &next.runtime.themes,
    );
    Ok(next)
}

#[cfg(test)]
mod tests {
    use super::{
        RenderSettingsSeed, build_plugin_manager, derive_render_settings_or_fallback,
        derive_ui_state,
    };
    use crate::app::{LaunchContext, RuntimeContext, TerminalKind};
    use crate::config::{ConfigLayer, ConfigResolver, ResolveOptions};

    fn resolved(entries: &[(&str, &str)]) -> crate::config::ResolvedConfig {
        let mut defaults = ConfigLayer::default();
        defaults.set("profile.default", "default");
        defaults.set("theme.path", Vec::<String>::new());
        for (key, value) in entries {
            defaults.set(*key, *value);
        }

        let mut resolver = ConfigResolver::default();
        resolver.set_defaults(defaults);
        resolver
            .resolve(ResolveOptions::default().with_terminal("cli"))
            .expect("test config should resolve")
    }

    #[test]
    fn derive_ui_state_layers_config_runtime_and_theme_selection_unit() {
        let config = resolved(&[("theme.name", "plain"), ("ui.margin", "3")]);
        let context = RuntimeContext::new(None, TerminalKind::Cli, Some("xterm".to_string()));
        let themes = crate::ui::theme_catalog::load_theme_catalog(&config);

        let ui = derive_ui_state(
            &context,
            &config,
            &themes,
            RenderSettingsSeed::DefaultAuto,
            None,
        )
        .expect("ui state should derive");

        assert_eq!(ui.render_settings.margin, 3);
        assert_eq!(ui.render_settings.theme_name, "plain");
        assert_eq!(
            ui.render_settings.runtime.terminal.as_deref(),
            Some("xterm")
        );
    }

    #[test]
    fn derive_ui_state_uses_the_same_margin_default_in_cli_and_repl_unit() {
        let config = resolved(&[]);
        let themes = crate::ui::theme_catalog::load_theme_catalog(&config);

        let repl = derive_ui_state(
            &RuntimeContext::new(None, TerminalKind::Repl, Some("xterm".to_string())),
            &config,
            &themes,
            RenderSettingsSeed::DefaultAuto,
            None,
        )
        .expect("repl ui state should derive");
        let cli = derive_ui_state(
            &RuntimeContext::new(None, TerminalKind::Cli, Some("xterm".to_string())),
            &config,
            &themes,
            RenderSettingsSeed::DefaultAuto,
            None,
        )
        .expect("cli ui state should derive");

        assert_eq!(repl.render_settings.margin, 0);
        assert_eq!(cli.render_settings.margin, 0);
    }

    #[test]
    fn host_inputs_derivation_reuses_one_config_path_for_ui_plugins_and_session_unit() {
        let config = resolved(&[("extensions.plugins.timeout_ms", "42")]);
        let context = RuntimeContext::new(None, TerminalKind::Cli, None);
        let launch = LaunchContext::default();

        let state = crate::app::AppStateBuilder::from_resolved_config(context, config)
            .expect("host inputs should derive")
            .with_launch(launch)
            .build();
        assert_eq!(state.runtime.ui.debug_level, 0);
        assert_eq!(state.clients.plugins().process_timeout().as_millis(), 42);
        assert!(state.session.history_enabled);
    }

    #[test]
    fn build_plugin_manager_applies_launch_roots_and_preference_override_unit() {
        let config = resolved(&[]);
        let launch = LaunchContext::default();
        let mut preferences = crate::plugin::state::PluginCommandPreferences::default();
        preferences.set_provider("ldap user", "demo");

        let manager = build_plugin_manager(&config, &launch, Some(&preferences));
        assert_eq!(
            manager
                .command_preferences_snapshot()
                .preferred_provider_for("ldap user"),
            Some("demo")
        );
    }

    #[test]
    fn derive_render_settings_or_fallback_preserves_existing_seed_runtime_facts_unit() {
        let config = resolved(&[("theme.name", "missing-theme")]);
        let context = RuntimeContext::new(None, TerminalKind::Cli, Some("xterm".to_string()));
        let themes = crate::ui::theme_catalog::load_theme_catalog(&config);
        let mut existing = crate::ui::RenderSettings::default();
        existing.runtime.terminal = Some("preserved-terminal".to_string());

        let derived = derive_render_settings_or_fallback(
            &context,
            &config,
            &themes,
            RenderSettingsSeed::Existing(Box::new(existing)),
            None,
        );

        assert_eq!(
            derived.runtime.terminal.as_deref(),
            Some("preserved-terminal")
        );
        assert_eq!(derived.theme_name, crate::ui::DEFAULT_THEME_NAME);
    }
}
