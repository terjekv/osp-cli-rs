//! Invocation flags shared by argv scanning, help and REPL completion.

use std::collections::BTreeSet;
use std::ffi::{OsStr, OsString};

use crate::core::document::{DocBlock, Document, DocumentEntry, HelpLevel};
use crate::core::output::{ColorMode, OutputFormat, RenderMode, UnicodeMode};
use miette::{Result, miette};

const INVOCATION_HELP_NOTE: &str =
    "These flags may appear anywhere before `--` and affect only the current command.";

pub(crate) struct InvocationFlag {
    pub(crate) long: &'static str,
    short: Option<char>,
    pub(crate) value_name: Option<&'static str>,
    pub(crate) choices: fn() -> Vec<&'static str>,
    pub(crate) aliases: &'static [InvocationAlias],
    pub(crate) help: &'static str,
    pub(crate) completion_help: Option<&'static str>,
    set: fn(&mut InvocationOptions, &str, &str) -> Result<()>,
    is_set: fn(&InvocationOptions) -> bool,
}

impl InvocationFlag {}

pub(crate) struct InvocationAlias {
    pub(crate) long: &'static str,
    value: &'static str,
}

impl InvocationAlias {
    pub(crate) fn tooltip(&self, flag: &InvocationFlag) -> String {
        format!("Alias for {} {}", flag.long, self.value)
    }
}

pub(crate) const INVOCATION_FLAGS: &[InvocationFlag] = &[
    InvocationFlag {
        long: "--format",
        short: None,
        value_name: Some("FORMAT"),
        choices: || {
            OutputFormat::ALL
                .into_iter()
                .map(OutputFormat::as_str)
                .collect()
        },
        aliases: &[
            InvocationAlias {
                long: "--json",
                value: "json",
            },
            InvocationAlias {
                long: "--table",
                value: "table",
            },
            InvocationAlias {
                long: "--mreg",
                value: "mreg",
            },
            InvocationAlias {
                long: "--value",
                value: "value",
            },
            InvocationAlias {
                long: "--md",
                value: "md",
            },
        ],
        help: "Format this invocation only",
        completion_help: None,
        set: |o, v, source| {
            parse_option(
                &mut o.format,
                v,
                source,
                "output format",
                OutputFormat::parse,
                OutputFormat::as_str,
            )
        },
        is_set: |o| o.format.is_some(),
    },
    InvocationFlag {
        long: "--render-mode",
        short: None,
        value_name: Some("MODE"),
        choices: || {
            RenderMode::ALL
                .into_iter()
                .map(RenderMode::as_str)
                .collect()
        },
        aliases: &[
            InvocationAlias {
                long: "--plain",
                value: "plain",
            },
            InvocationAlias {
                long: "--rich",
                value: "rich",
            },
        ],
        help: "Render mode for this invocation",
        completion_help: None,
        set: |o, v, source| {
            parse_option(
                &mut o.mode,
                v,
                source,
                "render mode",
                RenderMode::parse,
                RenderMode::as_str,
            )
        },
        is_set: |o| o.mode.is_some(),
    },
    InvocationFlag {
        long: "--color",
        short: None,
        value_name: Some("COLOR"),
        choices: || ColorMode::ALL.into_iter().map(ColorMode::as_str).collect(),
        aliases: &[InvocationAlias {
            long: "--no-color",
            value: "never",
        }],
        help: "Color policy for this invocation",
        completion_help: None,
        set: |o, v, source| {
            parse_option(
                &mut o.color,
                v,
                source,
                "color mode",
                ColorMode::parse,
                ColorMode::as_str,
            )
        },
        is_set: |o| o.color.is_some(),
    },
    InvocationFlag {
        long: "--unicode",
        short: None,
        value_name: Some("UNICODE"),
        choices: || {
            UnicodeMode::ALL
                .into_iter()
                .map(UnicodeMode::as_str)
                .collect()
        },
        aliases: &[InvocationAlias {
            long: "--ascii",
            value: "never",
        }],
        help: "Unicode policy for this invocation",
        completion_help: None,
        set: |o, v, source| {
            parse_option(
                &mut o.unicode,
                v,
                source,
                "unicode mode",
                UnicodeMode::parse,
                UnicodeMode::as_str,
            )
        },
        is_set: |o| o.unicode.is_some(),
    },
    InvocationFlag {
        long: "--verbose",
        short: Some('v'),
        value_name: None,
        choices: Vec::new,
        aliases: &[],
        help: "Increase message verbosity",
        completion_help: None,
        set: |o, _, _| {
            o.verbose = o.verbose.saturating_add(1);
            Ok(())
        },
        is_set: |_| false,
    },
    InvocationFlag {
        long: "--quiet",
        short: Some('q'),
        value_name: None,
        choices: Vec::new,
        aliases: &[],
        help: "Decrease message verbosity",
        completion_help: None,
        set: |o, _, _| {
            o.quiet = o.quiet.saturating_add(1);
            Ok(())
        },
        is_set: |_| false,
    },
    InvocationFlag {
        long: "--debug",
        short: Some('d'),
        value_name: None,
        choices: Vec::new,
        aliases: &[],
        help: "Increase developer log verbosity",
        completion_help: None,
        set: |o, _, _| {
            o.debug = o.debug.saturating_add(1);
            Ok(())
        },
        is_set: |_| false,
    },
];

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct InvocationOptions {
    pub(crate) format: Option<OutputFormat>,
    pub(crate) mode: Option<RenderMode>,
    pub(crate) color: Option<ColorMode>,
    pub(crate) unicode: Option<UnicodeMode>,
    pub(crate) verbose: u8,
    pub(crate) quiet: u8,
    pub(crate) debug: u8,
}

pub(crate) struct ScannedCliArgs {
    pub(crate) argv: Vec<OsString>,
    pub(crate) invocation: InvocationOptions,
    pub(crate) error: Option<miette::Report>,
}

#[derive(Clone, PartialEq, Eq)]
pub(crate) struct ScannedCommandTokens {
    pub(crate) tokens: Vec<String>,
    pub(crate) invocation: InvocationOptions,
    pub(crate) kept_indices: Vec<usize>,
}

impl InvocationOptions {
    /// Merges alias-expanded options using the scanner's conflict rules.
    pub(crate) fn merge(&mut self, other: InvocationOptions) -> Result<()> {
        if let Some(value) = other.format {
            set_once(&mut self.format, value, |existing| {
                format!(
                    "conflicting output format flags: existing `{}` and `--format {}`",
                    existing.as_str(),
                    value.as_str()
                )
            })?;
        }
        if let Some(value) = other.mode {
            set_once(&mut self.mode, value, |existing| {
                format!(
                    "conflicting render mode flags: existing `{}` and `--render-mode {}`",
                    existing.as_str(),
                    value.as_str()
                )
            })?;
        }
        if let Some(value) = other.color {
            set_once(&mut self.color, value, |existing| {
                format!(
                    "conflicting color mode flags: existing `{}` and `--color {}`",
                    existing.as_str(),
                    value.as_str()
                )
            })?;
        }
        if let Some(value) = other.unicode {
            set_once(&mut self.unicode, value, |existing| {
                format!(
                    "conflicting unicode mode flags: existing `{}` and `--unicode {}`",
                    existing.as_str(),
                    value.as_str()
                )
            })?;
        }
        self.verbose = self.verbose.saturating_add(other.verbose);
        self.quiet = self.quiet.saturating_add(other.quiet);
        self.debug = self.debug.saturating_add(other.debug);
        Ok(())
    }
}

pub(crate) fn scan_cli_argv(argv: &[OsString]) -> ScannedCliArgs {
    let mut invocation = InvocationOptions::default();
    let tail = argv.get(1..).unwrap_or_default();
    let (kept_indices, error) = scan_tokens(tail, &mut invocation);
    ScannedCliArgs {
        argv: argv
            .first()
            .cloned()
            .into_iter()
            .chain(kept_indices.iter().map(|&index| tail[index].clone()))
            .collect(),
        invocation,
        error,
    }
}

pub(crate) fn scan_command_tokens(tokens: &[String]) -> Result<ScannedCommandTokens> {
    let mut invocation = InvocationOptions::default();
    let (kept_indices, error) = scan_tokens(tokens, &mut invocation);
    if let Some(error) = error {
        return Err(error);
    }
    Ok(ScannedCommandTokens {
        tokens: kept_indices
            .iter()
            .map(|&index| tokens[index].clone())
            .collect(),
        invocation,
        kept_indices,
    })
}

// Finish the same scan after an error so startup diagnostics can use its
// verbosity and render options without a separate interpretation of argv.
fn scan_tokens<T: AsRef<OsStr>>(
    tokens: &[T],
    invocation: &mut InvocationOptions,
) -> (Vec<usize>, Option<miette::Report>) {
    let mut kept = Vec::new();
    let mut error = None;
    let mut index = 0;
    while index < tokens.len() {
        let token = tokens[index].as_ref().to_string_lossy();
        if token == "--" {
            kept.extend(index..tokens.len());
            break;
        }
        if token.starts_with('-')
            && !token.starts_with("--")
            && token.len() > 1
            && token.chars().skip(1).all(|ch| short_flag(ch).is_some())
        {
            for flag in token.chars().skip(1).filter_map(short_flag) {
                if let Err(err) = (flag.set)(invocation, "", &token) {
                    error.get_or_insert(err);
                }
            }
            index += 1;
            continue;
        }
        let (name, inline) = token
            .split_once('=')
            .map_or((token.as_ref(), None), |(name, v)| (name, Some(v)));
        let found = INVOCATION_FLAGS.iter().find_map(|flag| {
            if name == flag.long && (inline.is_none() || flag.value_name.is_some()) {
                Some((flag, None))
            } else {
                flag.aliases
                    .iter()
                    .find(|alias| token == alias.long)
                    .map(|alias| (flag, Some(alias.value)))
            }
        });
        if let Some((flag, alias)) = found {
            let takes_value = alias.is_none() && flag.value_name.is_some();
            let next = tokens.get(index + 1).map(|v| v.as_ref().to_string_lossy());
            let value = if takes_value {
                inline
                    .or(next.as_deref())
                    .ok_or_else(|| miette!("`{name}` expects a value"))
            } else {
                Ok(alias.unwrap_or(""))
            };
            if let Err(err) = value.and_then(|value| (flag.set)(invocation, value, name)) {
                error.get_or_insert(err);
            }
            index += if takes_value && inline.is_none() && next.is_some() {
                2
            } else {
                1
            };
        } else {
            kept.push(index);
            index += 1;
        }
    }
    (kept, error)
}

fn short_flag(ch: char) -> Option<&'static InvocationFlag> {
    INVOCATION_FLAGS.iter().find(|flag| flag.short == Some(ch))
}

fn set_once<T>(slot: &mut Option<T>, value: T, describe: impl Fn(&T) -> String) -> Result<()> {
    if let Some(existing) = slot.as_ref() {
        return Err(miette!(describe(existing)));
    }
    *slot = Some(value);
    Ok(())
}

fn parse_option<T: Copy>(
    slot: &mut Option<T>,
    raw: &str,
    source: &str,
    kind: &str,
    parse: fn(&str) -> Option<T>,
    label: fn(T) -> &'static str,
) -> Result<()> {
    let parsed = parse(raw);
    let value = parsed.ok_or_else(|| miette!("unknown {kind}: {raw}"));
    let result = value.and_then(|value| {
        set_once(slot, value, |existing| {
            format!(
                "conflicting {kind} flags: existing `{}` and `{source}`",
                label(*existing)
            )
        })
    });
    // After a conflict, the first error wins and the last render value is used
    // only to display that error, as in the former help fallback parser.
    *slot = parsed;
    result
}
fn invocation_help_entries(level: HelpLevel) -> Vec<DocumentEntry> {
    let mut entries = Vec::new();
    for flag in INVOCATION_FLAGS {
        if level < HelpLevel::Verbose && flag.short.is_none() {
            continue;
        }
        let choices = (flag.choices)();
        let name = match (flag.short, flag.value_name) {
            (Some(short), _) => format!("-{short}, {}", flag.long),
            (_, Some(value_name)) => format!(
                "{} <{}>",
                flag.long,
                if choices.is_empty() {
                    value_name.to_string()
                } else {
                    choices.join("|")
                }
            ),
            _ => flag.long.to_string(),
        };
        entries.push(DocumentEntry {
            name,
            description: flag.help.to_string(),
        });
        if !flag.aliases.is_empty() {
            entries.push(DocumentEntry {
                name: flag
                    .aliases
                    .iter()
                    .map(|alias| alias.long)
                    .collect::<Vec<_>>()
                    .join(" | "),
                description: if flag.aliases.len() == 1 {
                    flag.aliases[0].tooltip(flag)
                } else {
                    format!("Convenience aliases for {}", flag.long)
                },
            });
        }
    }
    entries
}

pub(crate) fn invocation_help_view(level: HelpLevel) -> Document {
    let mut document = Document {
        blocks: vec![
            DocBlock::section(
                "Common Invocation Options",
                vec![DocBlock::Entries {
                    entries: invocation_help_entries(level),
                }],
            ),
            DocBlock::Paragraph {
                text: INVOCATION_HELP_NOTE.to_string(),
            },
        ],
    };
    if level < HelpLevel::Verbose {
        document.blocks.truncate(1);
    }
    document
}

// `last --help` crosses a raw Clap boundary; structured help uses entries directly.
pub(crate) fn invocation_help_text() -> &'static str {
    static TEXT: std::sync::OnceLock<String> = std::sync::OnceLock::new();
    TEXT.get_or_init(|| {
        let mut text = "Common Invocation Options:\n".to_string();
        for entry in invocation_help_entries(HelpLevel::Verbose) {
            if entry.name.len() > 32 {
                text.push_str(&format!(
                    "  {}\n{:44}{}\n",
                    entry.name, "", entry.description
                ));
            } else {
                text.push_str(&format!("  {:42}{}\n", entry.name, entry.description));
            }
        }
        text.push_str(&format!("\n{INVOCATION_HELP_NOTE}\n"));
        text
    })
}

pub(crate) fn extend_with_invocation_help(view: &mut Document, level: HelpLevel) {
    if level >= HelpLevel::Normal {
        view.blocks.extend(invocation_help_view(level).blocks);
    }
}

pub(crate) fn should_show_invocation_help(invocation: &InvocationOptions) -> bool {
    invocation.verbose > 0
}

pub(crate) fn hidden_invocation_completion_flags(
    invocation: &InvocationOptions,
) -> BTreeSet<String> {
    INVOCATION_FLAGS
        .iter()
        .filter(|flag| !should_show_invocation_help(invocation) || (flag.is_set)(invocation))
        .flat_map(|flag| {
            std::iter::once(flag.long).chain(flag.aliases.iter().map(|alias| alias.long))
        })
        .map(str::to_string)
        .collect()
}

pub(crate) fn invocation_overview_summary() -> String {
    let groups = INVOCATION_FLAGS
        .iter()
        .map(|flag| {
            std::iter::once(flag.short.map_or_else(
                || flag.long.to_string(),
                |short| format!("-{short}/{}", flag.long),
            ))
            .chain(flag.aliases.iter().map(|alias| alias.long.to_string()))
            .collect::<Vec<_>>()
            .join("/")
        })
        .collect::<Vec<_>>();
    format!("per invocation: {}", groups.join(", "))
}

#[cfg(test)]
mod tests {
    use super::{
        InvocationOptions, extend_with_invocation_help, hidden_invocation_completion_flags,
        invocation_help_view, scan_cli_argv, scan_command_tokens, should_show_invocation_help,
    };
    use crate::core::document::Document;
    use crate::core::document::HelpLevel;
    use crate::core::output::{ColorMode, OutputFormat, RenderMode, UnicodeMode};
    use std::ffi::OsString;

    fn scan(tokens: &[&str]) -> super::ScannedCommandTokens {
        scan_command_tokens(
            &tokens
                .iter()
                .map(|value| (*value).to_string())
                .collect::<Vec<_>>(),
        )
        .expect("scan should succeed")
    }

    fn scan_error(tokens: &[&str]) -> String {
        scan_command_tokens(
            &tokens
                .iter()
                .map(|value| (*value).to_string())
                .collect::<Vec<_>>(),
        )
        .err()
        .expect("scan should fail")
        .to_string()
    }

    #[test]
    fn scan_command_tokens_covers_host_flags_aliases_clusters_and_trace_unit() {
        let scanned = scan(&["ldap", "user", "oistes", "--json", "-vv"]);
        assert_eq!(scanned.tokens, vec!["ldap", "user", "oistes"]);
        assert_eq!(
            scanned.invocation,
            InvocationOptions {
                format: Some(OutputFormat::Json),
                mode: None,
                color: None,
                unicode: None,
                verbose: 2,
                quiet: 0,
                debug: 0,
            }
        );

        let passthrough = scan(&["ldap", "--", "--json", "-vv"]);
        assert_eq!(passthrough.tokens, vec!["ldap", "--", "--json", "-vv"]);
        assert_eq!(passthrough.invocation, InvocationOptions::default());

        let non_host_short = scan(&["ldap", "-x", "--json", "alice"]);
        assert_eq!(non_host_short.tokens, vec!["ldap", "-x", "alice"]);
        assert_eq!(non_host_short.invocation.format, Some(OutputFormat::Json));

        let traced = scan_command_tokens(
            &["ldap", "--json", "user", "alice"]
                .iter()
                .map(|value| (*value).to_string())
                .collect::<Vec<_>>(),
        )
        .expect("trace scan should succeed");
        assert_eq!(traced.tokens, vec!["ldap", "user", "alice"]);
        assert_eq!(traced.kept_indices, vec![0, 2, 3]);

        let explicit_aliases = scan(&["ldap", "--plain", "--color", "never", "--ascii"]);
        assert_eq!(explicit_aliases.invocation.mode, Some(RenderMode::Plain));
        assert_eq!(explicit_aliases.invocation.color, Some(ColorMode::Never));
        assert_eq!(
            explicit_aliases.invocation.unicode,
            Some(UnicodeMode::Never)
        );

        let inline = scan(&[
            "ldap",
            "--format=value",
            "--render-mode=rich",
            "--color=always",
            "--unicode=always",
        ]);
        assert_eq!(inline.invocation.format, Some(OutputFormat::Value));
        assert_eq!(inline.invocation.mode, Some(RenderMode::Rich));
        assert_eq!(inline.invocation.color, Some(ColorMode::Always));
        assert_eq!(inline.invocation.unicode, Some(UnicodeMode::Always));

        let clustered = scan(&["ldap", "-vvqd", "user"]);
        assert_eq!(clustered.invocation.verbose, 2);
        assert_eq!(clustered.invocation.quiet, 1);
        assert_eq!(clustered.invocation.debug, 1);
        assert_eq!(clustered.tokens, vec!["ldap", "user"]);

        let guide = scan(&["ldap", "--format=auto"]);
        assert_eq!(guide.invocation.format, Some(OutputFormat::Auto));
    }

    #[test]
    fn scan_command_tokens_report_conflicts_and_missing_values_unit() {
        for (tokens, expected) in [
            (
                vec!["ldap", "--json", "--format", "table"],
                "conflicting output format flags",
            ),
            (
                vec!["ldap", "--json", "--format", "json"],
                "conflicting output format flags",
            ),
            (vec!["ldap", "--render-mode", "wat"], "unknown render mode"),
            (vec!["ldap", "--format"], "`--format` expects a value"),
            (vec!["ldap", "--color"], "`--color` expects a value"),
            (
                vec!["ldap", "--plain", "--render-mode", "rich"],
                "conflicting render mode flags",
            ),
            (
                vec!["ldap", "--color", "never", "--color=always"],
                "conflicting color mode flags",
            ),
            (
                vec!["ldap", "--ascii", "--unicode", "always"],
                "conflicting unicode mode flags",
            ),
        ] {
            assert!(
                scan_error(&tokens).contains(expected),
                "expected {expected:?} for tokens {tokens:?}"
            );
        }
    }

    #[test]
    fn invocation_help_hidden_completion_and_cli_argv_follow_visibility_and_flag_only_inputs_unit()
    {
        let rendered = invocation_help_view(crate::core::document::HelpLevel::Verbose).rows();
        assert!(!rendered.iter().any(|row| {
            row.get("name")
                .and_then(serde_json::Value::as_str)
                .is_some_and(|name| name.contains("--guide"))
        }));
        assert!(rendered.iter().any(|row| {
            row.get("name")
                .and_then(serde_json::Value::as_str)
                .is_some_and(|name| name.contains("--json"))
        }));
        assert!(rendered.iter().any(|row| {
            row.get("name")
                .and_then(serde_json::Value::as_str)
                .is_some_and(|name| name.contains("auto|json|table"))
        }));

        assert!(!should_show_invocation_help(&InvocationOptions::default()));
        assert!(should_show_invocation_help(&InvocationOptions {
            verbose: 1,
            ..InvocationOptions::default()
        }));

        let mut hidden = Document::from_help_text("Usage: osp [COMMAND]\n");
        extend_with_invocation_help(&mut hidden, HelpLevel::Tiny);
        assert!(
            !hidden
                .rows()
                .iter()
                .any(|row| row["section"] == "Common Invocation Options")
        );

        let mut visible = Document::from_help_text("Usage: osp [COMMAND]\n");
        extend_with_invocation_help(&mut visible, HelpLevel::Verbose);
        assert!(
            visible
                .rows()
                .iter()
                .any(|row| row["section"] == "Common Invocation Options")
        );

        let hidden = hidden_invocation_completion_flags(&InvocationOptions::default());
        assert!(!hidden.contains("--guide"));
        assert!(hidden.contains("--json"));
        assert!(hidden.contains("--debug"));

        let verbose = hidden_invocation_completion_flags(&InvocationOptions {
            verbose: 1,
            ..InvocationOptions::default()
        });
        assert!(!verbose.contains("--json"));
        assert!(!verbose.contains("--debug"));

        let used_one_shots = hidden_invocation_completion_flags(&InvocationOptions {
            verbose: 1,
            format: Some(OutputFormat::Json),
            ..InvocationOptions::default()
        });
        assert!(used_one_shots.contains("--format"));
        assert!(!used_one_shots.contains("--guide"));
        assert!(used_one_shots.contains("--json"));
        assert!(used_one_shots.contains("--table"));
        assert!(!used_one_shots.contains("--debug"));

        let scanned = scan_cli_argv(&[
            OsString::from("osp"),
            OsString::from("--json"),
            OsString::from("config"),
            OsString::from("show"),
        ]);
        assert!(scanned.error.is_none());
        assert_eq!(
            scanned.argv,
            vec![
                OsString::from("osp"),
                OsString::from("config"),
                OsString::from("show")
            ]
        );
        assert_eq!(scanned.invocation.format, Some(OutputFormat::Json));

        let empty = scan_cli_argv(&[]);
        assert!(empty.error.is_none());
        assert!(empty.argv.is_empty());
        assert_eq!(empty.invocation, InvocationOptions::default());

        let only_flags = scan_cli_argv(&[
            OsString::from("osp"),
            OsString::from("--json"),
            OsString::from("-vv"),
        ]);
        assert!(only_flags.error.is_none());
        assert_eq!(only_flags.argv, vec![OsString::from("osp")]);
        assert_eq!(only_flags.invocation.format, Some(OutputFormat::Json));
        assert_eq!(only_flags.invocation.verbose, 2);
    }
}
