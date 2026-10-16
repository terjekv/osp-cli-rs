//! Derived REPL browse/completion surface.
//!
//! The REPL needs a compact view of what the operator can see and type at the
//! root: completion specs, overview entries, intro command suggestions, and
//! alias hints. This module builds that derived surface from auth, config,
//! command definitions, and plugin catalog state.

use crate::completion::{CompletionNode, SuggestionEntry, ValueType};
use crate::config::{ConfigSchema, SchemaValueType};
use crate::core::document::HelpLevel;
use std::collections::BTreeSet;

use crate::app::help::help_level;
use crate::app::{
    CMD_CONFIG, CMD_CONTEXT, CMD_DOCTOR, CMD_HISTORY, CMD_LIST, CMD_PLUGINS, CMD_SHOW, CMD_THEME,
    CMD_USE, CURRENT_TERMINAL_SENTINEL,
};
use crate::plugin::CommandCatalogEntry;
use crate::ui::{HelpLayout, help_layout_from_config};

use super::ReplViewContext;
use super::history;
use crate::cli::commands::{doctor as doctor_cmd, theme as theme_cmd};

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ReplOverviewEntry {
    pub(crate) name: String,
    pub(crate) summary: String,
}

#[derive(Debug, Clone)]
pub(crate) struct ReplAliasEntry {
    pub(crate) name: String,
    pub(crate) template: String,
    pub(crate) tooltip: String,
}

#[derive(Debug, Clone)]
pub(crate) struct ReplSurface {
    pub(crate) root_words: Vec<String>,
    pub(crate) intro_commands: Vec<String>,
    pub(crate) specs: Vec<CompletionNode>,
    pub(crate) aliases: Vec<ReplAliasEntry>,
    pub(crate) overview_entries: Vec<ReplOverviewEntry>,
}

pub(crate) fn build_repl_surface(
    view: ReplViewContext<'_>,
    catalog: &[CommandCatalogEntry],
) -> ReplSurface {
    let history_enabled = history::repl_history_enabled(view.config) && view.history_enabled;
    let aliases = collect_alias_entries(view.config);
    let help_layout = help_layout_from_config(view.config);
    let help_level = help_level(view.config, 0, 0);

    let mut root_words = catalog_completion_words(catalog);
    root_words.push("source".to_string());
    let mut specs = crate::cli::repl_only_commands()
        .iter()
        .map(|command| clap_command_spec(command, help_layout))
        .collect::<Vec<_>>();
    let mut overview_entries = vec![
        ReplOverviewEntry {
            name: "exit".to_string(),
            summary: "Exit application.".to_string(),
        },
        ReplOverviewEntry {
            name: "help".to_string(),
            summary: "Show this command overview.".to_string(),
        },
        ReplOverviewEntry {
            name: "last".to_string(),
            summary: "Replay the last result (--raw before pipes).".to_string(),
        },
        ReplOverviewEntry {
            name: "source".to_string(),
            summary: "Run commands from files.".to_string(),
        },
    ];
    if shows_invocation_options_overview(help_level) {
        overview_entries.push(ReplOverviewEntry {
            name: "options".to_string(),
            summary: crate::cli::invocation::invocation_overview_summary(),
        });
    }

    specs.extend(
        catalog
            .iter()
            .filter_map(command_spec_from_catalog)
            .collect::<Vec<_>>(),
    );

    root_words.extend([CMD_PLUGINS.to_string(), CMD_LIST.to_string()]);
    specs.push(plugins_command_spec(catalog, help_layout));
    overview_entries.push(ReplOverviewEntry {
        name: CMD_PLUGINS.to_string(),
        summary: "Inspect and manage plugin providers".to_string(),
    });

    let def = doctor_cmd::doctor_command();
    root_words.push(CMD_DOCTOR.to_string());
    specs.push(clap_command_spec(&def, help_layout));
    overview_entries.push(overview_entry_from_clap(&def));
    root_words.extend([
        CMD_THEME.to_string(),
        CMD_LIST.to_string(),
        CMD_SHOW.to_string(),
        CMD_USE.to_string(),
    ]);
    let def = theme_cmd::theme_command();
    let mut spec = clap_command_spec(&def, help_layout);
    theme_cmd::augment_theme_completion(&mut spec, view.themes);
    specs.push(spec);
    overview_entries.push(overview_entry_from_clap(&def));

    root_words.extend([
        CMD_CONFIG.to_string(),
        "alias".to_string(),
        "add".to_string(),
        "get".to_string(),
        "show".to_string(),
        "explain".to_string(),
        "set".to_string(),
        "remove".to_string(),
        "doctor".to_string(),
    ]);
    specs.push(config_command_spec(view));
    specs.push(alias_command_spec(view, &aliases));
    overview_entries.push(ReplOverviewEntry {
        name: CMD_CONFIG.to_string(),
        summary: "Inspect and edit runtime config".to_string(),
    });
    overview_entries.push(ReplOverviewEntry {
        name: "alias".to_string(),
        summary: "Create and manage command aliases".to_string(),
    });

    if history_enabled {
        root_words.extend([
            CMD_HISTORY.to_string(),
            CMD_LIST.to_string(),
            "prune".to_string(),
            "clear".to_string(),
        ]);
        let def = history::history_command();
        let mut spec = clap_command_spec(&def, help_layout);
        history::augment_history_completion(&mut spec);
        specs.push(spec);
        overview_entries.push(overview_entry_from_clap(&def));
    }
    root_words.push(CMD_CONTEXT.to_string());
    specs.push(context_command_spec(help_layout));
    overview_entries.push(ReplOverviewEntry {
        name: CMD_CONTEXT.to_string(),
        summary: "Inspect or clear remembered session values".to_string(),
    });

    overview_entries.extend(catalog.iter().map(|entry| {
        let mut overview = plugin_overview_entry(entry);
        if !view.auth.external_command_access(&entry.name).is_runnable() {
            overview.summary.push_str(" (restricted)");
        }
        overview
    }));

    root_words.extend(view.themes.ids());
    root_words.extend(aliases.iter().map(|entry| entry.name.clone()));
    normalize_root_words(&mut root_words);
    order_root_words(&mut root_words, help_layout);
    let intro_commands = root_words
        .iter()
        .filter(|word| root_word_can_appear_in_intro(word))
        .take(4)
        .cloned()
        .collect();

    ReplSurface {
        root_words,
        intro_commands,
        specs,
        aliases,
        overview_entries,
    }
}

fn shows_invocation_options_overview(help_level: HelpLevel) -> bool {
    help_level >= HelpLevel::Verbose
}

fn normalize_root_words(root_words: &mut Vec<String>) {
    root_words.sort();
    root_words.dedup();
}

fn order_root_words(root_words: &mut [String], help_layout: HelpLayout) {
    if matches!(help_layout, HelpLayout::Full) {
        return;
    }

    root_words.sort_by(|left, right| {
        root_word_priority(left)
            .cmp(&root_word_priority(right))
            .then_with(|| left.cmp(right))
    });
}

fn root_word_priority(word: &str) -> (u8, u8) {
    match word {
        "help" => (0, 0),
        "last" => (0, 1),
        "exit" => (0, 2),
        "quit" => (0, 3),
        CMD_CONFIG => (1, 0),
        CMD_THEME => (1, 1),
        CMD_PLUGINS => (1, 2),
        CMD_DOCTOR => (1, 3),
        CMD_HISTORY => (1, 4),
        CMD_CONTEXT => (1, 5),
        "alias" => (1, 6),
        "|" | "F" | "P" | "V" => (4, 0),
        _ => {
            if word.starts_with('-') {
                (5, 0)
            } else {
                (2, 0)
            }
        }
    }
}

fn root_word_can_appear_in_intro(word: &str) -> bool {
    !matches!(word, "exit" | "quit")
        && !word.starts_with('-')
        && !matches!(word, "|" | "F" | "P" | "V")
}

fn command_sort_key(name: &str, help_layout: HelpLayout) -> String {
    let (tier, order) = if matches!(help_layout, HelpLayout::Full) {
        expressive_command_priority(name)
    } else {
        compact_command_priority(name)
    };
    format!("{}{:02}", tier, order)
}

fn expressive_command_priority(name: &str) -> (u8, u8) {
    match name {
        "help" => (0, 0),
        "last" => (0, 1),
        "exit" => (0, 2),
        "quit" => (0, 3),
        _ => (9, 0),
    }
}

fn compact_command_priority(name: &str) -> (u8, u8) {
    match name {
        "help" => (0, 0),
        "exit" => (0, 1),
        "quit" => (0, 2),
        CMD_CONFIG => (1, 0),
        CMD_THEME => (1, 1),
        CMD_PLUGINS => (1, 2),
        CMD_DOCTOR => (1, 3),
        CMD_HISTORY => (1, 4),
        CMD_CONTEXT => (1, 5),
        _ => (9, 0),
    }
}

fn context_command_spec(help_layout: HelpLayout) -> CompletionNode {
    builtin_command_spec(crate::cli::builtin_command(CMD_CONTEXT), help_layout)
}

pub(crate) fn catalog_completion_words(catalog: &[CommandCatalogEntry]) -> Vec<String> {
    let mut words = vec![
        "help".to_string(),
        "last".to_string(),
        "exit".to_string(),
        "quit".to_string(),
        "P".to_string(),
        "F".to_string(),
        "V".to_string(),
        "|".to_string(),
    ];
    for entry in catalog {
        words.extend(spec_completion_words(&entry.completion));
    }
    words.sort();
    words.dedup();
    words
}

pub(crate) fn collect_alias_entries(config: &crate::config::ResolvedConfig) -> Vec<ReplAliasEntry> {
    let mut out = Vec::new();
    for (key, entry) in config.aliases() {
        let Some(name) = key.strip_prefix("alias.") else {
            continue;
        };
        let name = name.trim();
        if name.is_empty() {
            continue;
        }
        let template = entry.raw_value.to_string();
        out.push(ReplAliasEntry {
            name: name.to_string(),
            tooltip: format!("alias: {template}"),
            template,
        });
    }
    out
}

fn command_spec_from_catalog(entry: &CommandCatalogEntry) -> Option<CompletionNode> {
    if matches!(
        entry.name.as_str(),
        "help" | "last" | "exit" | "quit" | CMD_PLUGINS | CMD_THEME | CMD_CONFIG | CMD_HISTORY
    ) {
        return None;
    }

    let mut spec = entry.completion.clone();
    if let Some(auth_hint) = entry.auth_hint() {
        let base = spec.tooltip.as_deref().unwrap_or("Plugin command");
        spec.tooltip = Some(format!("{base} [{auth_hint}]"));
    }
    if entry.conflicted || entry.requires_selection {
        spec.tooltip = Some(provider_selection_summary(entry, spec.tooltip.as_deref()));
    }

    Some(spec)
}

fn plugin_overview_entry(entry: &CommandCatalogEntry) -> ReplOverviewEntry {
    let summary = if entry.about.trim().is_empty() {
        "Plugin command".to_string()
    } else if entry.conflicted || entry.requires_selection {
        provider_selection_summary(entry, Some(&entry.about))
    } else {
        entry.about.clone()
    };
    ReplOverviewEntry {
        name: entry.name.clone(),
        summary,
    }
}

fn provider_selection_summary(entry: &CommandCatalogEntry, base: Option<&str>) -> String {
    let base = base
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .unwrap_or("Plugin command");
    if entry.requires_selection {
        return format!(
            "{base} (provider selection required; available: {}; use --plugin-provider <plugin-id> or `plugins select-provider {} <plugin-id>`)",
            entry.providers.join(", "),
            entry.name,
        );
    }

    let selected_label = match (&entry.provider, entry.source.plugin_source()) {
        (Some(provider), Some(source)) => format!("{provider} ({source})"),
        _ => return base.to_string(),
    };
    let alternatives = entry
        .providers
        .iter()
        .filter(|label| label.as_str() != selected_label.as_str())
        .cloned()
        .collect::<Vec<_>>();
    let selection_reason = if entry.selected_explicitly {
        "selected explicitly"
    } else {
        "resolved uniquely"
    };

    if alternatives.is_empty() {
        format!("{base} (using {selected_label}; {selection_reason})")
    } else {
        format!(
            "{base} (using {selected_label}; {selection_reason}; alternatives: {})",
            alternatives.join(", ")
        )
    }
}

fn overview_entry_from_clap(command: &clap::Command) -> ReplOverviewEntry {
    let about = command.get_about().map(ToString::to_string);
    let summary = about
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .unwrap_or("Command")
        .to_string();
    ReplOverviewEntry {
        name: command.get_name().to_string(),
        summary,
    }
}

fn clap_command_spec(command: &clap::Command, help_layout: HelpLayout) -> CompletionNode {
    let mut spec =
        CompletionNode::from_clap(command).sort(command_sort_key(command.get_name(), help_layout));
    for child in spec.children.values_mut() {
        if let Some(grammar) = command.find_subcommand(&child.name) {
            child.sort = Some(grammar.get_display_order().to_string());
        }
    }
    spec
}

fn spec_completion_words(spec: &CompletionNode) -> Vec<String> {
    let mut words = vec![spec.name.clone()];
    for flag in spec.flags.keys() {
        words.push(flag.clone());
    }
    for subcommand in spec.children.values() {
        words.extend(spec_completion_words(subcommand));
    }
    words
}

fn plugins_command_spec(
    catalog: &[CommandCatalogEntry],
    help_layout: HelpLayout,
) -> CompletionNode {
    let plugin_ids = catalog
        .iter()
        .flat_map(|entry| {
            entry
                .provider
                .iter()
                .cloned()
                .chain(entry.providers.iter().filter_map(|label| {
                    label
                        .split_once(" (")
                        .map(|(plugin_id, _)| plugin_id.to_string())
                }))
        })
        .collect::<BTreeSet<_>>()
        .into_iter()
        .map(SuggestionEntry::value)
        .collect::<Vec<_>>();
    let command_names = catalog
        .iter()
        .filter(|entry| matches!(entry.source, crate::plugin::CommandSource::Plugin(_)))
        .map(|entry| SuggestionEntry::value(entry.name.clone()))
        .collect::<Vec<_>>();

    let mut spec = builtin_command_spec(crate::cli::builtin_command(CMD_PLUGINS), help_layout);
    for child in spec.children.values_mut() {
        // Plugin mutations intentionally expose only their target arguments.
        child.flags.clear();
        for arg in &mut child.args {
            if matches!(child.name.as_str(), "enable" | "disable") {
                arg.name = Some("plugin_id".to_string());
            }
            arg.suggestions = if arg.name.as_deref() == Some("plugin_id") {
                plugin_ids.clone()
            } else {
                command_names.clone()
            };
        }
    }
    spec
}

fn config_command_spec(view: ReplViewContext<'_>) -> CompletionNode {
    let mut spec = builtin_command_spec(
        crate::cli::builtin_command(CMD_CONFIG),
        help_layout_from_config(view.config),
    );
    config_write_suggestions(&mut spec, view.config);
    for child in spec.children.values_mut() {
        if child.name == "set" {
            // Config keys and their values are injected as a schema-derived tree.
            child.args.clear();
            child.flags.remove("--user");
            child.flags.remove("-u");
        } else {
            for arg in &mut child.args {
                arg.suggestions = config_key_suggestions();
            }
        }
    }
    spec
}

fn alias_command_spec(view: ReplViewContext<'_>, aliases: &[ReplAliasEntry]) -> CompletionNode {
    let mut spec = builtin_command_spec(
        crate::cli::builtin_command("alias"),
        help_layout_from_config(view.config),
    );
    config_write_suggestions(&mut spec, view.config);
    for child in spec.children.values_mut() {
        if child.name == "remove" {
            for arg in &mut child.args {
                arg.suggestions = aliases
                    .iter()
                    .map(|alias| {
                        SuggestionEntry::value(alias.name.clone()).meta(alias.tooltip.clone())
                    })
                    .collect();
            }
        }
        if let Some(flag) = child.flags.get_mut("--sources") {
            flag.tooltip = Some("Include config source and scope".to_string());
        }
    }
    spec
}

fn config_write_suggestions(spec: &mut CompletionNode, config: &crate::config::ResolvedConfig) {
    for child in spec.children.values_mut() {
        if let Some(flag) = child.flags.get_mut("--profile") {
            flag.suggestions = config
                .known_profiles()
                .iter()
                .map(SuggestionEntry::value)
                .collect();
        }
        if let Some(flag) = child.flags.get_mut("--terminal") {
            flag.suggestions = [CURRENT_TERMINAL_SENTINEL, "cli", "repl"]
                .into_iter()
                .map(SuggestionEntry::value)
                .collect();
        }
    }
}

fn builtin_command_spec(command: clap::Command, help_layout: HelpLayout) -> CompletionNode {
    let mut spec = clap_command_spec(&command, help_layout);
    // Preserve the REPL wording and historical menu order over clap structure.
    let (tooltip, children): (&str, &[(&str, &str)]) = match command.get_name() {
        CMD_CONTEXT => (
            "Show or clear session context",
            &[("clear", "Remove context entries")],
        ),
        CMD_PLUGINS => (
            "Inspect and manage plugin providers",
            &[
                ("list", "List available plugins"),
                ("commands", "Show plugin command catalog"),
                ("config", "Show projected plugin config"),
                ("refresh", "Refresh plugin discovery cache"),
                ("doctor", "Run plugin diagnostics"),
                ("enable", "Enable plugin by id"),
                ("disable", "Disable plugin by id"),
                ("clear-state", "Clear persisted state for one command"),
                ("select-provider", "Select provider for one command"),
                ("clear-provider", "Clear selected provider for one command"),
            ],
        ),
        CMD_CONFIG => (
            "Inspect and edit runtime config",
            &[
                ("show", "Show current config"),
                ("get", "Get one config key"),
                ("explain", "Explain one config key"),
                ("set", "Set config value"),
                ("unset", "Remove one config key"),
                ("doctor", "Show config diagnostics"),
            ],
        ),
        _ => (
            "Create and manage command aliases",
            &[
                ("list", "List aliases in the active scope"),
                ("add", "Add or replace an alias"),
                ("remove", "Remove an alias"),
            ],
        ),
    };
    spec.tooltip = Some(tooltip.to_string());
    for child in spec.children.values_mut() {
        if let Some((index, (_, tooltip))) = children
            .iter()
            .enumerate()
            .find(|(_, (name, _))| *name == child.name)
        {
            child.tooltip = Some((*tooltip).to_string());
            child.sort = Some((index + 10).to_string());
        }

        for arg in &mut child.args {
            arg.name = arg.name.take().map(|name| name.to_ascii_lowercase());
            arg.required = false;
            arg.tooltip = None;
        }
        for (name, flag) in &mut child.flags {
            flag.tooltip = match name.as_str() {
                "--sources" => Some("Include source layers"),
                "--raw" => Some("Show raw values"),
                "--show-secrets" => Some("Reveal secret values"),
                "--global" => Some("Write to the global config store"),
                "--profile" => Some("Write to one named profile"),
                "--profile-all" => Some("Write to every known profile"),
                "--terminal" => Some("Write to one terminal context"),
                "--session" => Some("Change only this in-memory session"),
                "--config" => Some("Use the regular config store"),
                "--secrets" => Some("Use the secrets store"),
                "--permanent" | "--save" => Some("Persist immediately after validation"),
                "--dry-run" => Some("Show the write plan without changing config"),
                "--yes" => Some("Skip interactive confirmation"),
                "--explain" => Some("Explain the resolved write targets"),
                "--from-file" => None,
                _ => flag.tooltip.as_deref(),
            }
            .map(str::to_string);
        }
    }
    for child in spec.children.values_mut() {
        if let Some(flag) = child.flags.get_mut("--from-file") {
            // PathBuf's inferred parser does not supply a clap value hint.
            flag.value_type = Some(ValueType::Path);
            flag.suggestions = vec![SuggestionEntry::value("-")];
        }
    }
    spec
}

fn config_key_suggestions() -> Vec<SuggestionEntry> {
    let schema = ConfigSchema::default();
    schema
        .entries()
        .map(|(key, _)| SuggestionEntry::value(key.to_string()))
        .collect()
}

pub(crate) fn config_set_key_specs(
    view: ReplViewContext<'_>,
) -> Vec<crate::completion::ConfigKeySpec> {
    let schema = ConfigSchema::default();
    schema
        .entries()
        .map(|(key, entry)| {
            let value_suggestions = if key == "theme.name" {
                view.themes
                    .ids()
                    .into_iter()
                    .map(SuggestionEntry::value)
                    .collect::<Vec<_>>()
            } else if key == "repl.tab_mode" {
                [
                    "tab",
                    "always",
                    "after_1_letter",
                    "after_2_letters",
                    "after_3_letters",
                    "after_4_letters",
                    "after_5_letters",
                ]
                .into_iter()
                .map(SuggestionEntry::value)
                .collect()
            } else if let Some(allowed) = entry.allowed_values() {
                allowed
                    .iter()
                    .map(|value| SuggestionEntry::value(value.clone()))
                    .collect::<Vec<_>>()
            } else if matches!(entry.value_type(), SchemaValueType::Bool) {
                vec![
                    SuggestionEntry::value("true"),
                    SuggestionEntry::value("false"),
                ]
            } else {
                Vec::new()
            };

            let mut spec =
                crate::completion::ConfigKeySpec::new(key).value_suggestions(value_suggestions);
            if let Some(doc) = entry.doc() {
                spec = spec.tooltip(doc);
            }
            spec
        })
        // Open namespaces (`extensions.*`, per-user display names, aliases)
        // have no schema entries; offer the keys this session actually has.
        .chain(
            view.config
                .values()
                .keys()
                .filter(|key| schema.entries().all(|(known, _)| known != key.as_str()))
                .map(crate::completion::ConfigKeySpec::new),
        )
        .collect()
}
