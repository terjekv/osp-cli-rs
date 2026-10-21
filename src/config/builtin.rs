//! Builtin key declarations own schema metadata, documentation, bootstrap rules,
//! and literal defaults. Computed defaults and runtime fallbacks live in `defaults`.

use super::defaults::{
    DEFAULT_PROFILE_NAME, DEFAULT_REPL_HISTORY_MAX_ENTRIES, DEFAULT_REPL_HISTORY_MENU_ROWS,
    DEFAULT_UI_WIDTH_MAX,
};
use super::{BootstrapPhase, BootstrapScopeRule, BootstrapValueRule, ConfigValue, SchemaEntry};

const LOG_LEVEL_NAMES: &[&str] = &["error", "warn", "info", "debug", "trace"];

pub(super) struct BuiltinKey {
    pub(super) key: &'static str,
    pub(super) schema: SchemaEntry,
    pub(super) default: Option<ConfigValue>,
}

pub(super) fn builtin_keys() -> &'static [BuiltinKey] {
    static KEYS: std::sync::OnceLock<Vec<BuiltinKey>> = std::sync::OnceLock::new();
    KEYS.get_or_init(|| {
        let mut keys = vec![
            BuiltinKey {
                key: "secrets.backend",
                schema: SchemaEntry::string()
                    .with_allowed_values(
                        super::SecretBackendKind::ALL
                            .iter()
                            .map(|value| value.as_str()),
                    )
                    .bootstrap(BootstrapPhase::Path, BootstrapScopeRule::GlobalOnly)
                    .with_doc(
                        "Persistent secrets backend selected before secret values are loaded",
                    ),
                default: Some(ConfigValue::from("toml")),
            },
            BuiltinKey {
                key: "profile.default",
                schema: SchemaEntry::string()
                    .bootstrap_only(
                        BootstrapPhase::Profile,
                        BootstrapScopeRule::GlobalOrTerminal,
                    )
                    .with_bootstrap_value_rule(BootstrapValueRule::NonEmptyString)
                    .with_doc("Default profile selected when no override is provided"),
                default: Some(ConfigValue::from(DEFAULT_PROFILE_NAME)),
            },
            BuiltinKey {
                key: "profile.active",
                schema: SchemaEntry::string()
                    .required()
                    .read_only()
                    .with_doc("Active profile derived during resolution"),
                default: None,
            },
            BuiltinKey {
                key: "theme.name",
                schema: SchemaEntry::string().with_doc("Name of the active color theme"),
                default: None,
            },
            BuiltinKey {
                key: "theme.path",
                schema: SchemaEntry::string_list().with_doc("Extra theme search paths"),
                default: None,
            },
            BuiltinKey {
                key: "context.seed",
                schema: SchemaEntry::string_list()
                    .with_doc("Context kinds seeded from cached session defaults; [] disables"),
                default: Some(ConfigValue::from(vec!["account".into(), "netgroup".into()])),
            },
            BuiltinKey {
                key: "user.name",
                schema: SchemaEntry::string()
                    .with_doc("Short user name used in prompts and interpolation"),
                default: None,
            },
            BuiltinKey {
                key: "domain",
                schema: SchemaEntry::string()
                    .with_doc("Default domain name used in prompts and interpolation"),
                default: None,
            },
            BuiltinKey {
                key: "ui.format",
                schema: SchemaEntry::string()
                    .with_allowed_values(
                        crate::core::output::OutputFormat::ALL
                            .iter()
                            .map(|value| value.as_str()),
                    )
                    .with_doc("Default output format"),
                default: None,
            },
            BuiltinKey {
                key: "ui.guide.default_format",
                schema: SchemaEntry::string()
                    .with_allowed_values(["guide", "inherit", "none"])
                    .with_doc("Document rendering preference for help-like output"),
                default: Some(ConfigValue::from("guide")),
            },
            BuiltinKey {
                key: "ui.mode",
                schema: SchemaEntry::string()
                    .with_allowed_values(
                        crate::core::output::RenderMode::ALL
                            .iter()
                            .map(|value| value.as_str()),
                    )
                    .with_doc("Preferred render mode"),
                default: None,
            },
            BuiltinKey {
                key: "ui.presentation",
                schema: SchemaEntry::string()
                    .with_allowed_values(["expressive", "compact", "austere", "gammel-og-bitter"])
                    .with_doc("UI presentation preset"),
                default: Some(ConfigValue::from("expressive")),
            },
            BuiltinKey {
                key: "ui.pager",
                schema: SchemaEntry::string()
                    .with_allowed_values(["auto", "never"])
                    .with_doc("Paging policy for tall one-shot terminal output"),
                default: Some(ConfigValue::from("auto")),
            },
            BuiltinKey {
                key: "ui.color.mode",
                schema: SchemaEntry::string()
                    .with_allowed_values(
                        crate::core::output::ColorMode::ALL
                            .iter()
                            .map(|value| value.as_str()),
                    )
                    .with_doc("Color rendering policy"),
                default: None,
            },
            BuiltinKey {
                key: "ui.unicode.mode",
                schema: SchemaEntry::string()
                    .with_allowed_values(
                        crate::core::output::UnicodeMode::ALL
                            .iter()
                            .map(|value| value.as_str()),
                    )
                    .with_doc("Unicode rendering policy"),
                default: None,
            },
            BuiltinKey {
                key: "ui.width",
                schema: SchemaEntry::positive_integer().with_doc("Default render width hint"),
                default: Some(ConfigValue::from(72)),
            },
            BuiltinKey {
                key: "ui.width-max",
                schema: SchemaEntry::integer()
                    .with_doc("Maximum render width; -1 disables the maximum"),
                default: Some(ConfigValue::from(DEFAULT_UI_WIDTH_MAX)),
            },
            BuiltinKey {
                key: "config.default-target",
                schema: SchemaEntry::string().with_doc(
                    "Default config write destination: session, global, or a profile name",
                ),
                default: None,
            },
            BuiltinKey {
                key: "ui.margin",
                schema: SchemaEntry::integer().with_doc("Left margin used when rendering output"),
                default: Some(ConfigValue::from(0)),
            },
            BuiltinKey {
                key: "ui.help.level",
                schema: SchemaEntry::string()
                    .with_allowed_values(["none", "tiny", "normal", "verbose"])
                    .with_doc("Base help detail level, adjusted by -v and -q"),
                default: Some(ConfigValue::from("normal")),
            },
            BuiltinKey {
                key: "ui.messages.layout",
                schema: SchemaEntry::string()
                    .with_allowed_values([
                        "full", "compact", "austere", "plain", "none", "grouped", "minimal",
                    ])
                    .with_doc("Message layout style"),
                default: Some(ConfigValue::from("grouped")),
            },
            BuiltinKey {
                key: "ui.chrome.frame",
                schema: SchemaEntry::string()
                    .with_allowed_values(
                        crate::ui::section_chrome::SectionFrameStyle::ALL
                            .iter()
                            .map(|value| value.as_str()),
                    )
                    .with_doc("Section chrome frame style"),
                default: Some(ConfigValue::from("top")),
            },
            BuiltinKey {
                key: "ui.chrome.rule_policy",
                schema: SchemaEntry::string()
                    .with_allowed_values([
                        "per-section",
                        "independent",
                        "separate",
                        "shared",
                        "stacked",
                        "list",
                    ])
                    .with_doc("How sibling section rules are shared"),
                default: Some(ConfigValue::from("shared")),
            },
            BuiltinKey {
                key: "ui.table.overflow",
                schema: SchemaEntry::string()
                    .with_allowed_values([
                        "clip", "hidden", "crop", "ellipsis", "truncate", "wrap", "none", "visible",
                    ])
                    .with_doc("Table overflow behavior"),
                default: Some(ConfigValue::from("ellipsis")),
            },
            BuiltinKey {
                key: "ui.table.border",
                schema: SchemaEntry::string()
                    .with_allowed_values(
                        crate::ui::TableBorderStyle::ALL
                            .iter()
                            .map(|value| value.as_str()),
                    )
                    .with_doc("Table border style"),
                default: Some(ConfigValue::from("square")),
            },
            BuiltinKey {
                key: "ui.table.nested_border",
                schema: SchemaEntry::string()
                    .with_allowed_values(
                        crate::ui::TableBorderOverride::ALL
                            .iter()
                            .map(|value| value.as_str()),
                    )
                    .with_doc("Border style for tables nested inside a record, or inherit"),
                default: Some(ConfigValue::from("none")),
            },
            BuiltinKey {
                key: "ui.verbosity",
                schema: SchemaEntry::string()
                    .with_allowed_values(
                        crate::core::MessageLevel::ALL
                            .iter()
                            .map(|value| value.as_str()),
                    )
                    .with_doc("Default message verbosity level"),
                default: Some(ConfigValue::from("success")),
            },
            BuiltinKey {
                key: "extensions.plugins.timeout_ms",
                schema: SchemaEntry::positive_integer()
                    .with_doc("Plugin describe timeout in milliseconds"),
                default: Some(ConfigValue::from(
                    crate::plugin::DEFAULT_PLUGIN_PROCESS_TIMEOUT_MS as i64,
                )),
            },
            BuiltinKey {
                key: "extensions.plugins.disabled",
                schema: SchemaEntry::string_list().with_doc("Disabled plugin names"),
                default: Some(ConfigValue::from(Vec::<String>::new())),
            },
            BuiltinKey {
                key: "repl.prompt",
                schema: SchemaEntry::string().with_doc("Prompt template used by the REPL"),
                default: None,
            },
            BuiltinKey {
                key: "repl.exit_message",
                schema: SchemaEntry::string()
                    .with_doc("Message printed when leaving the root REPL; empty disables it"),
                default: Some(ConfigValue::from("")),
            },
            BuiltinKey {
                key: "repl.notifications.sound",
                schema: SchemaEntry::string()
                    .with_allowed_values(["off", "bell"])
                    .with_doc(
                        "Master terminal sound policy; best-effort BEL in rich human REPL only",
                    ),
                default: Some(ConfigValue::from("off")),
            },
            BuiltinKey {
                key: "repl.input_mode",
                schema: SchemaEntry::string()
                    .with_allowed_values(
                        crate::repl::ReplInputMode::ALL
                            .iter()
                            .map(|value| value.as_str()),
                    )
                    .with_doc("REPL input mode"),
                default: Some(ConfigValue::from("auto")),
            },
            BuiltinKey {
                key: "command.prefixes",
                schema: SchemaEntry::string()
                    .with_allowed_values(["off", "interactive", "always"])
                    .with_doc("Accept unique command prefixes: off, interactive, or always"),
                default: Some(ConfigValue::from("interactive")),
            },
            BuiltinKey {
                key: "repl.hint.command_limit",
                schema: SchemaEntry::integer()
                    .with_doc("Maximum command names in typing hints; zero or less hides lists"),
                default: Some(ConfigValue::from(3)),
            },
            BuiltinKey {
                key: "repl.complete_on",
                schema: SchemaEntry::string_list().with_doc(
                    "Typing triggers for the completion menu: flag, space, after_<n>_letters",
                ),
                default: Some(ConfigValue::from(vec!["flag".to_string()])),
            },
            BuiltinKey {
                key: "repl.simple_prompt",
                schema: SchemaEntry::boolean()
                    .with_doc("Whether the REPL should use the simple prompt"),
                default: Some(ConfigValue::from(false)),
            },
            BuiltinKey {
                key: "repl.shell_indicator",
                schema: SchemaEntry::string().with_doc("Template for the current shell indicator"),
                default: Some(ConfigValue::from("[{shell}]")),
            },
            BuiltinKey {
                key: "repl.shellable_commands",
                schema: SchemaEntry::string_list()
                    .with_doc("Product-owned commands that support REPL subshell entry"),
                default: None,
            },
            BuiltinKey {
                key: "repl.intro",
                schema: SchemaEntry::string()
                    .with_allowed_values(
                        crate::repl::presentation::ReplIntroStyle::ALL
                            .iter()
                            .map(|value| value.as_str()),
                    )
                    .with_doc("REPL intro detail level"),
                default: Some(ConfigValue::from("full")),
            },
            BuiltinKey {
                key: "repl.intro_template.minimal",
                schema: SchemaEntry::string().with_doc("Template for the minimal REPL intro"),
                default: None,
            },
            BuiltinKey {
                key: "repl.intro_template.compact",
                schema: SchemaEntry::string().with_doc("Template for the compact REPL intro"),
                default: None,
            },
            BuiltinKey {
                key: "repl.intro_template.full",
                schema: SchemaEntry::string().with_doc("Template for the full REPL intro"),
                default: None,
            },
            BuiltinKey {
                key: "repl.intro_tips",
                schema: SchemaEntry::string_list()
                    .with_doc("Tips for the intro's {{tip}} placeholder; one is shown per day"),
                default: None,
            },
            BuiltinKey {
                key: "repl.history.path",
                schema: SchemaEntry::string().with_doc("Path to the persistent REPL history file"),
                default: None,
            },
            BuiltinKey {
                key: "repl.history.max_entries",
                schema: SchemaEntry::positive_integer()
                    .with_doc("Maximum number of persisted REPL history entries"),
                default: Some(ConfigValue::from(DEFAULT_REPL_HISTORY_MAX_ENTRIES)),
            },
            BuiltinKey {
                key: "repl.history.enabled",
                schema: SchemaEntry::boolean()
                    .with_doc("Whether persistent REPL history is enabled"),
                default: Some(ConfigValue::from(true)),
            },
            BuiltinKey {
                key: "repl.history.record",
                schema: SchemaEntry::string()
                    .with_allowed_values(["all", "commands", "off"])
                    .with_doc("Record all eligible lines, recognized command attempts, or nothing"),
                default: Some(ConfigValue::from("commands")),
            },
            BuiltinKey {
                key: "repl.history.dedupe",
                schema: SchemaEntry::boolean()
                    .with_doc("Whether duplicate history entries are collapsed"),
                default: Some(ConfigValue::from(true)),
            },
            BuiltinKey {
                key: "repl.history.menu_rows",
                schema: SchemaEntry::positive_integer()
                    .with_doc("Maximum rows shown in the history menu"),
                default: Some(ConfigValue::from(DEFAULT_REPL_HISTORY_MENU_ROWS)),
            },
            BuiltinKey {
                key: "repl.history.exclude",
                schema: SchemaEntry::string_list()
                    .with_doc("Commands excluded from persisted history"),
                default: None,
            },
            BuiltinKey {
                key: "debug.level",
                schema: SchemaEntry::integer().with_doc("Developer debug verbosity"),
                default: Some(ConfigValue::from(0)),
            },
            BuiltinKey {
                key: "log.file.enabled",
                schema: SchemaEntry::boolean().with_doc("Whether file logging is enabled"),
                default: Some(ConfigValue::from(false)),
            },
            BuiltinKey {
                key: "log.file.path",
                schema: SchemaEntry::string().with_doc("Path to the runtime log file"),
                default: None,
            },
            BuiltinKey {
                key: "log.file.level",
                schema: SchemaEntry::string()
                    .with_allowed_values(LOG_LEVEL_NAMES)
                    .with_doc("Minimum log level written to the log file"),
                default: Some(ConfigValue::from("info")),
            },
        ];
        keys.extend(crate::core::style::ColorRole::ALL.map(|role| BuiltinKey {
            key: role.key(),
            schema: SchemaEntry::string().with_doc(
                "Override this palette role for every theme; unset uses the theme's value",
            ),
            default: None,
        }));
        keys
    })
}
