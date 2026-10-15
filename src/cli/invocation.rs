//! Per-invocation flag scanning.
//!
//! Some flags affect only the current command invocation and may appear before
//! the real command path. This module peels those flags off argv/token input
//! so the rest of the CLI grammar can parse the actual command while the host
//! still receives the invocation-scoped render/debug/provider options.

use std::collections::BTreeSet;
use std::ffi::OsString;

use crate::core::document::{Document, HelpLevel};
use crate::core::output::{ColorMode, OutputFormat, RenderMode, UnicodeMode};
use miette::{Result, miette};

pub(crate) const INVOCATION_HELP_SECTION: &str = r#"Common Invocation Options:
  --format <auto|json|table|mreg|value|md>
                                            Format this invocation only
  --json | --table | --mreg | --value | --md
                                            Convenience aliases for --format
  --render-mode <auto|plain|rich>           Render mode for this invocation
  --plain | --rich                          Convenience aliases for --render-mode
  --color <auto|always|never>               Color policy for this invocation
  --no-color                                Alias for --color never
  --unicode <auto|always|never>             Unicode policy for this invocation
  --ascii                                   Alias for --unicode never
  -v, --verbose                             Increase message verbosity
  -q, --quiet                               Decrease message verbosity
  -d, --debug                               Increase developer log verbosity
  --plugin-provider <PLUGIN_ID>             Select provider for this invocation

These flags may appear anywhere before `--` and affect only the current command.
"#;

const FORMAT_ALIASES: &[(&str, OutputFormat)] = &[
    ("--json", OutputFormat::Json),
    ("--table", OutputFormat::Table),
    ("--mreg", OutputFormat::Mreg),
    ("--value", OutputFormat::Value),
    ("--md", OutputFormat::Markdown),
];
const MODE_ALIASES: &[(&str, RenderMode)] =
    &[("--plain", RenderMode::Plain), ("--rich", RenderMode::Rich)];

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct InvocationOptions {
    pub(crate) format: Option<OutputFormat>,
    pub(crate) mode: Option<RenderMode>,
    pub(crate) color: Option<ColorMode>,
    pub(crate) unicode: Option<UnicodeMode>,
    pub(crate) verbose: u8,
    pub(crate) quiet: u8,
    pub(crate) debug: u8,
    pub(crate) plugin_provider: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ScannedCliArgs {
    pub(crate) argv: Vec<OsString>,
    pub(crate) invocation: InvocationOptions,
}

#[derive(Clone, PartialEq, Eq)]
pub(crate) struct ScannedCommandTokens {
    pub(crate) tokens: Vec<String>,
    pub(crate) invocation: InvocationOptions,
}

#[derive(Clone, PartialEq, Eq)]
pub(crate) struct ScannedCommandTokenTrace {
    pub(crate) tokens: Vec<String>,
    pub(crate) invocation: InvocationOptions,
    pub(crate) kept_indices: Vec<usize>,
}

impl InvocationOptions {
    /// Adds flags found in a second scan, such as an expanded alias, using
    /// the same conflict rules as a single scan.
    pub(crate) fn merge(&mut self, other: InvocationOptions) -> Result<()> {
        if let Some(format) = other.format {
            set_format(self, format, &format!("--format {}", format.as_str()))?;
        }
        if let Some(mode) = other.mode {
            set_mode(self, mode, &format!("--render-mode {}", mode.as_str()))?;
        }
        if let Some(color) = other.color {
            set_color(self, color, &format!("--color {}", color.as_str()))?;
        }
        if let Some(unicode) = other.unicode {
            set_unicode(self, unicode, &format!("--unicode {}", unicode.as_str()))?;
        }
        if let Some(provider) = other.plugin_provider {
            set_plugin_provider(self, &provider, "--plugin-provider")?;
        }
        self.verbose = self.verbose.saturating_add(other.verbose);
        self.quiet = self.quiet.saturating_add(other.quiet);
        self.debug = self.debug.saturating_add(other.debug);
        Ok(())
    }
}

pub(crate) fn scan_cli_argv(argv: &[OsString]) -> Result<ScannedCliArgs> {
    if argv.is_empty() {
        return Ok(ScannedCliArgs {
            argv: Vec::new(),
            invocation: InvocationOptions::default(),
        });
    }

    let tail = &argv[1..];
    let lossy = tail
        .iter()
        .map(|value| value.to_string_lossy().into_owned())
        .collect::<Vec<_>>();
    let scanned = scan_command_tokens_with_trace(&lossy)?;
    // Kept tokens are verbatim, so forward the native values (e.g. paths).
    let mut cleaned = Vec::with_capacity(scanned.kept_indices.len() + 1);
    cleaned.push(argv[0].clone());
    cleaned.extend(
        scanned
            .kept_indices
            .iter()
            .map(|&index| tail[index].clone()),
    );

    Ok(ScannedCliArgs {
        argv: cleaned,
        invocation: scanned.invocation,
    })
}

pub(crate) fn scan_command_tokens(tokens: &[String]) -> Result<ScannedCommandTokens> {
    let traced = scan_command_tokens_with_trace(tokens)?;
    Ok(ScannedCommandTokens {
        tokens: traced.tokens,
        invocation: traced.invocation,
    })
}

pub(crate) fn scan_command_tokens_with_trace(
    tokens: &[String],
) -> Result<ScannedCommandTokenTrace> {
    let mut cleaned = Vec::with_capacity(tokens.len());
    let mut kept_indices = Vec::with_capacity(tokens.len());
    let mut invocation = InvocationOptions::default();
    let mut index = 0usize;
    let mut passthrough = false;

    while index < tokens.len() {
        let token = &tokens[index];
        if passthrough {
            cleaned.push(token.clone());
            kept_indices.push(index);
            index += 1;
            continue;
        }

        if token == "--" {
            passthrough = true;
            cleaned.push(token.clone());
            kept_indices.push(index);
            index += 1;
            continue;
        }

        if let Some((_, format)) = FORMAT_ALIASES.iter().find(|(flag, _)| token == flag) {
            set_format(&mut invocation, *format, token)?;
            index += 1;
            continue;
        }

        if let Some((_, mode)) = MODE_ALIASES.iter().find(|(flag, _)| token == flag) {
            set_mode(&mut invocation, *mode, token)?;
            index += 1;
            continue;
        }

        if token == "--ascii" {
            set_unicode(&mut invocation, UnicodeMode::Never, token)?;
            index += 1;
            continue;
        }

        if token == "--no-color" {
            set_color(&mut invocation, ColorMode::Never, token)?;
            index += 1;
            continue;
        }

        if token == "--verbose" {
            invocation.verbose = invocation.verbose.saturating_add(1);
            index += 1;
            continue;
        }

        if token == "--quiet" {
            invocation.quiet = invocation.quiet.saturating_add(1);
            index += 1;
            continue;
        }

        if token == "--debug" {
            invocation.debug = invocation.debug.saturating_add(1);
            index += 1;
            continue;
        }

        if is_short_count_cluster(token) {
            for ch in token.chars().skip(1) {
                match ch {
                    'v' => invocation.verbose = invocation.verbose.saturating_add(1),
                    'q' => invocation.quiet = invocation.quiet.saturating_add(1),
                    'd' => invocation.debug = invocation.debug.saturating_add(1),
                    _ => {}
                }
            }
            index += 1;
            continue;
        }

        let (flag, inline_value) = token
            .split_once('=')
            .map_or((token.as_str(), None), |(flag, value)| (flag, Some(value)));
        if matches!(
            flag,
            "--format" | "--render-mode" | "--color" | "--unicode" | "--plugin-provider"
        ) {
            let value = inline_value
                .or_else(|| tokens.get(index + 1).map(String::as_str))
                .ok_or_else(|| miette!("`{flag}` expects a value"))?;
            match flag {
                "--format" => {
                    let format = OutputFormat::parse(value)
                        .ok_or_else(|| miette!("unknown output format: {value}"))?;
                    set_format(&mut invocation, format, flag)?;
                }
                "--render-mode" => {
                    let mode = RenderMode::parse(value)
                        .ok_or_else(|| miette!("unknown render mode: {value}"))?;
                    set_mode(&mut invocation, mode, flag)?;
                }
                "--color" => {
                    let color = ColorMode::parse(value)
                        .ok_or_else(|| miette!("unknown color mode: {value}"))?;
                    set_color(&mut invocation, color, flag)?;
                }
                "--unicode" => {
                    let unicode = UnicodeMode::parse(value)
                        .ok_or_else(|| miette!("unknown unicode mode: {value}"))?;
                    set_unicode(&mut invocation, unicode, flag)?;
                }
                _ => set_plugin_provider(&mut invocation, value, flag)?,
            }
            index += if inline_value.is_some() { 1 } else { 2 };
            continue;
        }

        cleaned.push(token.clone());
        kept_indices.push(index);
        index += 1;
    }

    Ok(ScannedCommandTokenTrace {
        tokens: cleaned,
        invocation,
        kept_indices,
    })
}

pub(crate) fn invocation_help_view() -> Document {
    Document::from_help_text(INVOCATION_HELP_SECTION)
}

pub(crate) fn extend_with_invocation_help(view: &mut Document, level: HelpLevel) {
    if level >= HelpLevel::Verbose {
        view.blocks.extend(invocation_help_view().blocks);
    }
}

pub(crate) fn should_show_invocation_help(invocation: &InvocationOptions) -> bool {
    invocation.verbose > 0
}

pub(crate) fn hidden_invocation_completion_flags(
    invocation: &InvocationOptions,
) -> BTreeSet<String> {
    let hide_all = !should_show_invocation_help(invocation);
    let mut hidden = BTreeSet::new();
    if hide_all || invocation.format.is_some() {
        hidden.insert("--format".to_string());
        hidden.extend(FORMAT_ALIASES.iter().map(|(flag, _)| (*flag).to_string()));
    }
    if hide_all || invocation.mode.is_some() {
        hidden.insert("--render-mode".to_string());
        hidden.extend(MODE_ALIASES.iter().map(|(flag, _)| (*flag).to_string()));
    }
    if hide_all || invocation.color.is_some() {
        hidden.insert("--color".to_string());
        hidden.insert("--no-color".to_string());
    }
    if hide_all || invocation.unicode.is_some() {
        hidden.extend(["--unicode", "--ascii"].map(str::to_string));
    }
    if hide_all || invocation.plugin_provider.is_some() {
        hidden.insert("--plugin-provider".to_string());
    }
    if hide_all {
        hidden.extend(["--verbose", "--quiet", "--debug"].map(str::to_string));
    }
    hidden
}

fn is_short_count_cluster(token: &str) -> bool {
    token.starts_with('-')
        && !token.starts_with("--")
        && token.len() > 1
        && token
            .chars()
            .skip(1)
            .all(|ch| matches!(ch, 'v' | 'q' | 'd'))
}

fn set_format(options: &mut InvocationOptions, format: OutputFormat, source: &str) -> Result<()> {
    if let Some(existing) = options.format {
        return Err(miette!(
            "conflicting output format flags: existing `{}` and `{source}`",
            existing.as_str()
        ));
    }
    options.format = Some(format);
    Ok(())
}

fn set_mode(options: &mut InvocationOptions, mode: RenderMode, source: &str) -> Result<()> {
    if let Some(existing) = options.mode {
        return Err(miette!(
            "conflicting render mode flags: existing `{}` and `{source}`",
            existing.as_str()
        ));
    }
    options.mode = Some(mode);
    Ok(())
}

fn set_color(options: &mut InvocationOptions, color: ColorMode, source: &str) -> Result<()> {
    if let Some(existing) = options.color {
        return Err(miette!(
            "conflicting color mode flags: existing `{}` and `{source}`",
            existing.as_str()
        ));
    }
    options.color = Some(color);
    Ok(())
}

fn set_unicode(options: &mut InvocationOptions, unicode: UnicodeMode, source: &str) -> Result<()> {
    if let Some(existing) = options.unicode {
        return Err(miette!(
            "conflicting unicode mode flags: existing `{}` and `{source}`",
            existing.as_str()
        ));
    }
    options.unicode = Some(unicode);
    Ok(())
}

fn set_plugin_provider(options: &mut InvocationOptions, value: &str, source: &str) -> Result<()> {
    let normalized = value.trim();
    if normalized.is_empty() {
        return Err(miette!("{source} expects a non-empty value"));
    }
    if options.plugin_provider.is_some() {
        return Err(miette!("`{source}` specified more than once"));
    }
    options.plugin_provider = Some(normalized.to_string());
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{
        INVOCATION_HELP_SECTION, InvocationOptions, extend_with_invocation_help,
        hidden_invocation_completion_flags, invocation_help_view, scan_cli_argv,
        scan_command_tokens, scan_command_tokens_with_trace, should_show_invocation_help,
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
        let scanned = scan(&[
            "ldap",
            "user",
            "oistes",
            "--json",
            "-vv",
            "--plugin-provider",
            "uio-ldap",
        ]);
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
                plugin_provider: Some("uio-ldap".to_string()),
            }
        );

        let passthrough = scan(&["ldap", "--", "--json", "-vv"]);
        assert_eq!(passthrough.tokens, vec!["ldap", "--", "--json", "-vv"]);
        assert_eq!(passthrough.invocation, InvocationOptions::default());

        let non_host_short = scan(&["ldap", "-x", "--json", "alice"]);
        assert_eq!(non_host_short.tokens, vec!["ldap", "-x", "alice"]);
        assert_eq!(non_host_short.invocation.format, Some(OutputFormat::Json));

        let traced = scan_command_tokens_with_trace(
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
            (
                vec!["ldap", "--plugin-provider", "one", "--plugin-provider=two"],
                "specified more than once",
            ),
            (vec!["ldap", "--render-mode", "wat"], "unknown render mode"),
            (
                vec!["ldap", "--plugin-provider", "   "],
                "expects a non-empty value",
            ),
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
        let rendered = invocation_help_view().rows();
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
        assert!(!INVOCATION_HELP_SECTION.contains("--guide"));
        assert!(INVOCATION_HELP_SECTION.contains("auto|json|table"));

        assert!(!should_show_invocation_help(&InvocationOptions::default()));
        assert!(should_show_invocation_help(&InvocationOptions {
            verbose: 1,
            ..InvocationOptions::default()
        }));

        let mut hidden = Document::from_help_text("Usage: osp [COMMAND]\n");
        extend_with_invocation_help(&mut hidden, HelpLevel::Normal);
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
        assert!(hidden.contains("--plugin-provider"));
        assert!(hidden.contains("--debug"));

        let verbose = hidden_invocation_completion_flags(&InvocationOptions {
            verbose: 1,
            ..InvocationOptions::default()
        });
        assert!(!verbose.contains("--json"));
        assert!(!verbose.contains("--plugin-provider"));
        assert!(!verbose.contains("--debug"));

        let used_one_shots = hidden_invocation_completion_flags(&InvocationOptions {
            verbose: 1,
            format: Some(OutputFormat::Json),
            plugin_provider: Some("ldap".to_string()),
            ..InvocationOptions::default()
        });
        assert!(used_one_shots.contains("--format"));
        assert!(!used_one_shots.contains("--guide"));
        assert!(used_one_shots.contains("--json"));
        assert!(used_one_shots.contains("--table"));
        assert!(used_one_shots.contains("--plugin-provider"));
        assert!(!used_one_shots.contains("--debug"));

        let scanned = scan_cli_argv(&[
            OsString::from("osp"),
            OsString::from("--json"),
            OsString::from("config"),
            OsString::from("show"),
        ])
        .expect("cli argv scan should succeed");
        assert_eq!(
            scanned.argv,
            vec![
                OsString::from("osp"),
                OsString::from("config"),
                OsString::from("show")
            ]
        );
        assert_eq!(scanned.invocation.format, Some(OutputFormat::Json));

        let empty = scan_cli_argv(&[]).expect("empty argv should scan");
        assert!(empty.argv.is_empty());
        assert_eq!(empty.invocation, InvocationOptions::default());

        let only_flags = scan_cli_argv(&[
            OsString::from("osp"),
            OsString::from("--json"),
            OsString::from("-vv"),
        ])
        .expect("cli argv scan should succeed");
        assert_eq!(only_flags.argv, vec![OsString::from("osp")]);
        assert_eq!(only_flags.invocation.format, Some(OutputFormat::Json));
        assert_eq!(only_flags.invocation.verbose, 2);
    }
}
