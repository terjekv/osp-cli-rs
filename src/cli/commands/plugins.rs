//! Executable inventory and persistent enablement.

use crate::app::{AppClients, AppRuntime, CommandOutput};
use crate::cli::{PluginsArgs, PluginsCommands};
use crate::config::{ConfigValue, RuntimeConfigPaths, Scope, TomlStoreEditOptions, set_scoped_value_in_toml};
use crate::core::row::Row;
use miette::{Result, WrapErr, miette};

pub(crate) fn run_plugins_command(runtime: &AppRuntime, clients: &AppClients, args: PluginsArgs) -> Result<CommandOutput> {
    match args.command {
        PluginsCommands::List => Ok(super::rows_with_empty_info(plugin_rows(clients, false), "No plugins discovered.")),
        PluginsCommands::Refresh => {
            clients.plugins().refresh();
            let mut result = CommandOutput::exit(0);
            result.messages.success("refreshed plugin discovery cache");
            Ok(result)
        }
        PluginsCommands::Enable { name } => set_enabled(runtime, clients, &name, true),
        PluginsCommands::Disable { name } => set_enabled(runtime, clients, &name, false),
    }
}

pub(crate) fn plugin_rows(clients: &AppClients, issues_only: bool) -> Vec<Row> {
    let native_names = clients.native_commands().catalog().into_iter().map(|entry| entry.name).collect::<std::collections::HashSet<_>>();
    clients.plugins().list().into_iter().filter_map(|plugin| {
        let status = if crate::cli::builtin_command_names().contains(&plugin.name.as_str()) {
            format!("shadowed by builtin {}", plugin.name)
        } else if native_names.contains(&plugin.name) {
            format!("shadowed by native {}", plugin.name)
        } else { plugin.status };
        if issues_only && status == "ok" { return None }
        Some(crate::row! {
            "name" => plugin.name,
            "about" => plugin.describe.map(|d| d.about).unwrap_or_default(),
            "source" => plugin.source.to_string(),
            "path" => plugin.path.display().to_string(),
            "status" => status,
        })
    }).collect()
}

fn set_enabled(runtime: &AppRuntime, clients: &AppClients, name: &str, enabled: bool) -> Result<CommandOutput> {
    if !clients.plugins().list().iter().any(|plugin| plugin.name == name) {
        return Err(miette!("unknown plugin `{name}`"));
    }
    if !runtime.launch.runtime_load.include_config_file {
        return Err(miette!("config file writes are disabled for this session"));
    }
    let config = crate::app::resolve_runtime_config(
        crate::app::RuntimeConfigRequest::new(runtime.context.profile_override().map(ToOwned::to_owned), Some(runtime.context.terminal_kind().as_config_terminal()))
            .with_runtime_load(runtime.launch.runtime_load).with_product_defaults(runtime.product_defaults().clone()),
    )?;
    let mut disabled = config.get_string_list("extensions.plugins.disabled").unwrap_or_default();
    disabled.retain(|entry| entry != name);
    if !enabled { disabled.push(name.to_string()); }
    let path = RuntimeConfigPaths::discover().config_file.ok_or_else(|| miette!("failed to resolve config.toml path for plugin config"))?;
    set_scoped_value_in_toml(&path, "extensions.plugins.disabled", &ConfigValue::from(disabled.clone()), &Scope::global(), TomlStoreEditOptions::new())
        .wrap_err("failed to persist disabled plugins")?;
    clients.plugins().set_disabled(disabled);
    let mut result = CommandOutput::exit(0);
    result.messages.success(format!("{} plugin: {name}", if enabled { "enabled" } else { "disabled" }));
    Ok(result)
}
