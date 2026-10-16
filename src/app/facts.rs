//! Pure host-side fact derivation shared across app surfaces.
//!
//! This module owns the small projections that several host paths depend on:
//! parsing positive integer config values, resolving render/theme defaults, and
//! lowering runtime/UI state into plugin dispatch hints. Keeping those rules
//! here lets `app::host` stay focused on orchestration instead of becoming a
//! grab bag of reusable derivation helpers.

use crate::config::{ConfigValue, ResolvedConfig};
use crate::core::runtime::{RuntimeHints, RuntimeTerminalKind};
use crate::plugin::config::{PluginConfigScope, config_value_to_plugin_env};
use crate::plugin::{DEFAULT_PLUGIN_PROCESS_TIMEOUT_MS, PluginDispatchContext};
use crate::ui::RenderRuntime;
use crate::ui::theme::normalize_theme_name;
use crate::ui::theme_catalog::ThemeCatalog;
use miette::{Result, miette};
use std::io::IsTerminal;
use terminal_size::{Width, terminal_size};

use super::external::ExternalCommandRuntime;
use super::{
    AppClients, AppRuntime, ConfigState, ResolvedInvocation, RuntimeContext, TerminalKind, UiState,
};

pub(crate) fn resolve_known_theme_name(value: &str, catalog: &ThemeCatalog) -> Result<String> {
    let normalized = normalize_theme_name(value);
    if catalog.resolve(&normalized).is_some() {
        return Ok(normalized);
    }

    let known = catalog.ids().join(", ");
    Err(miette!("unknown theme: {value}. available themes: {known}"))
}

pub(crate) fn config_usize(config: &ResolvedConfig, key: &str, fallback: usize) -> usize {
    match config.get(key).map(ConfigValue::reveal) {
        Some(ConfigValue::Integer(value)) if *value > 0 => *value as usize,
        Some(ConfigValue::String(raw)) => raw
            .trim()
            .parse::<usize>()
            .ok()
            .filter(|value| *value > 0)
            .unwrap_or(fallback),
        _ => fallback,
    }
}

pub(crate) fn plugin_process_timeout(config: &ResolvedConfig) -> std::time::Duration {
    std::time::Duration::from_millis(config_usize(
        config,
        "extensions.plugins.timeout_ms",
        DEFAULT_PLUGIN_PROCESS_TIMEOUT_MS,
    ) as u64)
}

pub(crate) fn plugin_path_discovery_enabled(config: &ResolvedConfig) -> bool {
    config
        .get_bool("extensions.plugins.discovery.path")
        .unwrap_or(false)
}

pub(crate) fn build_render_runtime(terminal_env: Option<&str>) -> RenderRuntime {
    RenderRuntime {
        command_scope: Default::default(),
        stdout_is_tty: std::io::stdout().is_terminal(),
        terminal: terminal_env.map(ToOwned::to_owned),
        no_color: std::env::var("NO_COLOR").is_ok(),
        width: detect_terminal_width().or_else(detect_columns_env),
        locale_utf8: locale_utf8_hint_from_env(),
    }
}

#[cfg(test)]
pub(crate) fn plugin_dispatch_context_for_runtime(
    runtime: &AppRuntime,
    clients: &AppClients,
    invocation: Option<&ResolvedInvocation>,
) -> PluginDispatchContext {
    build_plugin_dispatch_context(
        &runtime.context,
        &runtime.config,
        clients,
        &runtime.ui,
        invocation,
    )
}

pub(in crate::app) fn plugin_dispatch_context_for(
    runtime: &ExternalCommandRuntime<'_>,
    invocation: Option<&ResolvedInvocation>,
) -> PluginDispatchContext {
    build_plugin_dispatch_context(
        runtime.context,
        runtime.config_state,
        runtime.clients,
        runtime.ui,
        invocation,
    )
}

pub(in crate::app) fn runtime_hints_for_invocation(
    runtime: &AppRuntime,
    invocation: &ResolvedInvocation,
) -> RuntimeHints {
    runtime_hints(
        &runtime.context,
        runtime.config.resolved().active_profile(),
        &invocation.ui,
    )
}

fn build_plugin_dispatch_context(
    context: &RuntimeContext,
    config: &ConfigState,
    clients: &AppClients,
    runtime_ui: &UiState,
    invocation: Option<&ResolvedInvocation>,
) -> PluginDispatchContext {
    let config_env = clients.plugin_config_env(config);
    let ui = invocation.map(|value| &value.ui).unwrap_or(runtime_ui);
    let provider_override = invocation.and_then(|value| value.plugin_provider.clone());
    let shared_entries = config_env.effective_entries("");
    let shared_env_issues = shared_entries
        .iter()
        .filter(|entry| entry.scope == PluginConfigScope::Shared)
        .filter_map(plugin_env_issue_text)
        .collect::<Vec<_>>();

    PluginDispatchContext::new(runtime_hints(
        context,
        config.resolved().active_profile(),
        ui,
    ))
    .with_shared_env(
        shared_entries
            .iter()
            .filter(|entry| entry.scope == PluginConfigScope::Shared && entry.issue.is_none())
            .map(|entry| {
                (
                    entry.env_key.clone(),
                    config_value_to_plugin_env(&entry.value),
                )
            })
            .collect::<Vec<_>>(),
    )
    .with_shared_env_issues(shared_env_issues)
    .with_plugin_env(
        config_env
            .by_plugin_id
            .keys()
            .map(|plugin_id| {
                let entries = config_env.effective_entries(plugin_id);
                (
                    plugin_id.clone(),
                    entries
                        .into_iter()
                        .filter(|entry| {
                            entry.scope == PluginConfigScope::Plugin && entry.issue.is_none()
                        })
                        .map(|entry| (entry.env_key, config_value_to_plugin_env(&entry.value)))
                        .collect(),
                )
            })
            .collect(),
    )
    .with_plugin_env_issues(
        config_env
            .by_plugin_id
            .keys()
            .filter_map(|plugin_id| {
                let issues = config_env
                    .effective_entries(plugin_id)
                    .iter()
                    .filter(|entry| entry.scope == PluginConfigScope::Plugin)
                    .filter_map(plugin_env_issue_text)
                    .collect::<Vec<_>>();
                (!issues.is_empty()).then_some((plugin_id.clone(), issues))
            })
            .collect(),
    )
    .with_provider_override(provider_override)
}

fn plugin_env_issue_text(entry: &crate::plugin::config::PluginConfigEntry) -> Option<String> {
    entry
        .issue
        .as_ref()
        .map(|issue| format!("{} from {}: {}", entry.env_key, entry.config_key, issue))
}

fn runtime_hints(context: &RuntimeContext, active_profile: &str, ui: &UiState) -> RuntimeHints {
    RuntimeHints::new(
        ui.verbosity,
        ui.debug_level.min(3),
        ui.render_settings.format,
        ui.render_settings.color,
        ui.render_settings.unicode,
    )
    .with_profile(Some(active_profile.to_string()))
    .with_terminal(context.terminal_env().map(ToOwned::to_owned))
    .with_terminal_kind(match context.terminal_kind() {
        TerminalKind::Cli => RuntimeTerminalKind::Cli,
        TerminalKind::Repl => RuntimeTerminalKind::Repl,
    })
}

fn detect_terminal_width() -> Option<usize> {
    if !std::io::stdout().is_terminal() {
        return None;
    }
    terminal_size()
        .map(|(Width(columns), _)| columns as usize)
        .filter(|value| *value > 0)
}

fn detect_columns_env() -> Option<usize> {
    std::env::var("COLUMNS")
        .ok()
        .and_then(|value| value.trim().parse::<usize>().ok())
        .filter(|value| *value > 0)
}

fn locale_utf8_hint_from_env() -> Option<bool> {
    for key in ["LC_ALL", "LC_CTYPE", "LANG"] {
        if let Ok(value) = std::env::var(key) {
            let lower = value.to_ascii_lowercase();
            if lower.contains("utf-8") || lower.contains("utf8") {
                return Some(true);
            }
            return Some(false);
        }
    }
    None
}

/// Refreshes reader facts before the intro and each line, including live aliases.
pub(crate) fn set_command_scope(runtime: &mut AppRuntime, scope: &super::ReplScopeStack) {
    runtime.ui.render_settings.runtime.command_scope = match runtime.context.terminal_kind() {
        TerminalKind::Cli => crate::core::CommandScope::OneShot,
        TerminalKind::Repl => match scope.display_label() {
            None => crate::core::CommandScope::ReplRoot,
            Some(root) => crate::core::CommandScope::ReplShell {
                root,
                root_words: crate::repl::input::REPL_GLOBAL_COMMANDS
                    .iter()
                    .map(|word| (*word).to_string())
                    .chain(
                        runtime
                            .config
                            .resolved()
                            .aliases()
                            .keys()
                            .filter_map(|key| key.strip_prefix("alias."))
                            .map(|name| name.trim().to_string()),
                    )
                    .collect(),
            },
        },
    };
}
