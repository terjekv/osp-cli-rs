//! Runtime hint model shared across host, native commands, and plugin
//! boundaries.
//!
//! This module exists so runtime-facing behavior like verbosity, output mode,
//! and terminal identity can travel through the system as normalized data
//! instead of ad hoc environment parsing in each caller.
//!
//! High-level flow:
//!
//! - parse runtime-oriented environment hints into
//!   [`crate::core::RuntimeHints`]
//! - normalize those values into enums and bounded scalar settings
//! - export the same normalized hints back into environment pairs when needed
//!
//! Contract:
//!
//! - environment parsing rules for runtime hints live here
//! - callers should consume [`crate::core::RuntimeHints`] instead of
//!   reparsing raw env vars
//!
//! Public API shape:
//!
//! - [`crate::core::RuntimeHints::new`] is the exact constructor for
//!   already-resolved runtime settings
//! - [`crate::core::RuntimeHints::from_env`] and
//!   [`crate::core::RuntimeHints::from_env_iter`] are the probing and
//!   adaptation factories
//! - optional metadata such as profile and terminal identity uses `with_*`
//!   refinements instead of raw ad hoc assembly

use std::collections::HashMap;

use crate::core::output::{ColorMode, OutputFormat, UnicodeMode};

/// Environment variable carrying the UI verbosity hint.
pub(crate) const ENV_OSP_UI_VERBOSITY: &str = "OSP_UI_VERBOSITY";
/// Environment variable carrying the debug level hint.
pub(crate) const ENV_OSP_DEBUG_LEVEL: &str = "OSP_DEBUG_LEVEL";
/// Environment variable carrying the preferred output format.
pub(crate) const ENV_OSP_FORMAT: &str = "OSP_FORMAT";
/// Environment variable carrying the color-mode hint.
pub(crate) const ENV_OSP_COLOR: &str = "OSP_COLOR";
/// Environment variable carrying the Unicode-mode hint.
pub(crate) const ENV_OSP_UNICODE: &str = "OSP_UNICODE";
/// Environment variable carrying the active profile name.
pub(crate) const ENV_OSP_PROFILE: &str = "OSP_PROFILE";
/// Environment variable carrying the active terminal identifier.
pub(crate) const ENV_OSP_TERMINAL: &str = "OSP_TERMINAL";
/// Environment variable carrying the terminal kind hint.
pub(crate) const ENV_OSP_TERMINAL_KIND: &str = "OSP_TERMINAL_KIND";

/// UI message verbosity derived from runtime hints and environment.
#[derive(
    Debug,
    Clone,
    Copy,
    PartialEq,
    Eq,
    PartialOrd,
    Ord,
    Default,
    serde::Serialize,
    serde::Deserialize,
)]
#[serde(rename_all = "lowercase")]
pub enum MessageLevel {
    /// Show only errors.
    Error,
    /// Show errors and warnings.
    Warning,
    /// Show success messages in addition to warnings and errors.
    #[default]
    Success,
    /// Show normal informational output.
    Info,
    /// Show trace-level output.
    Trace,
    /// Show evidence behind requests and cache decisions.
    Evidence,
}

impl MessageLevel {
    /// All variants in canonical configuration order.
    pub const ALL: [Self; 6] = [
        Self::Error,
        Self::Warning,
        Self::Success,
        Self::Info,
        Self::Trace,
        Self::Evidence,
    ];

    /// Numeric command detail exported to executable plugins (0 through 3).
    pub fn command_verbosity(self) -> u8 {
        match self {
            Self::Error | Self::Warning | Self::Success => 0,
            Self::Info => 1,
            Self::Trace => 2,
            Self::Evidence => 3,
        }
    }

    /// Returns the canonical string representation for this verbosity level.
    pub fn as_str(self) -> &'static str {
        match self {
            MessageLevel::Error => "error",
            MessageLevel::Warning => "warning",
            MessageLevel::Success => "success",
            MessageLevel::Info => "info",
            MessageLevel::Trace => "trace",
            MessageLevel::Evidence => "evidence",
        }
    }

    /// Parses a case-insensitive verbosity level or supported alias.
    ///
    /// # Examples
    ///
    /// ```
    /// use osp_cli::core::MessageLevel;
    ///
    /// assert_eq!(MessageLevel::parse("warn"), Some(MessageLevel::Warning));
    /// assert_eq!(MessageLevel::parse("TRACE"), Some(MessageLevel::Trace));
    /// assert_eq!(MessageLevel::parse("loud"), None);
    /// ```
    pub fn parse(value: &str) -> Option<Self> {
        match value.trim().to_ascii_lowercase().as_str() {
            "error" => Some(MessageLevel::Error),
            "warning" | "warn" => Some(MessageLevel::Warning),
            "success" => Some(MessageLevel::Success),
            "info" => Some(MessageLevel::Info),
            "trace" => Some(MessageLevel::Trace),
            "evidence" => Some(MessageLevel::Evidence),
            _ => None,
        }
    }
}

/// Runtime terminal mode exposed through environment hints.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum RuntimeTerminalKind {
    /// Invocation is running as a one-shot CLI command.
    Cli,
    /// Invocation is running inside the interactive REPL.
    Repl,
    /// Terminal kind is unknown or unspecified.
    #[default]
    Unknown,
}

impl RuntimeTerminalKind {
    /// Returns the canonical string representation for this terminal kind.
    pub fn as_str(self) -> &'static str {
        match self {
            RuntimeTerminalKind::Cli => "cli",
            RuntimeTerminalKind::Repl => "repl",
            RuntimeTerminalKind::Unknown => "unknown",
        }
    }

    /// Parses a case-insensitive terminal kind name.
    ///
    /// # Examples
    ///
    /// ```
    /// use osp_cli::core::RuntimeTerminalKind;
    ///
    /// assert_eq!(RuntimeTerminalKind::parse("cli"), Some(RuntimeTerminalKind::Cli));
    /// assert_eq!(RuntimeTerminalKind::parse("REPL"), Some(RuntimeTerminalKind::Repl));
    /// assert_eq!(RuntimeTerminalKind::parse("tty"), None);
    /// ```
    pub fn parse(value: &str) -> Option<Self> {
        match value.trim().to_ascii_lowercase().as_str() {
            "cli" => Some(RuntimeTerminalKind::Cli),
            "repl" => Some(RuntimeTerminalKind::Repl),
            "unknown" => Some(RuntimeTerminalKind::Unknown),
            _ => None,
        }
    }
}

/// Normalized runtime settings loaded from environment variables.
#[derive(Debug, Clone, PartialEq, Eq)]
#[must_use]
pub struct RuntimeHints {
    /// Effective UI message verbosity.
    pub ui_verbosity: MessageLevel,
    /// Effective debug level capped to the supported range.
    pub debug_level: u8,
    /// Effective output format preference.
    pub format: OutputFormat,
    /// Effective color-mode preference.
    pub color: ColorMode,
    /// Effective Unicode-mode preference.
    pub unicode: UnicodeMode,
    /// Active profile identifier, when set.
    pub profile: Option<String>,
    /// Active terminal identifier, when set.
    pub terminal: Option<String>,
    /// Effective terminal kind hint.
    pub terminal_kind: RuntimeTerminalKind,
}

impl Default for RuntimeHints {
    fn default() -> Self {
        Self {
            ui_verbosity: MessageLevel::Success,
            debug_level: 0,
            format: OutputFormat::Auto,
            color: ColorMode::Auto,
            unicode: UnicodeMode::Auto,
            profile: None,
            terminal: None,
            terminal_kind: RuntimeTerminalKind::Unknown,
        }
    }
}

impl RuntimeHints {
    /// Creates a fully specified runtime-hint payload with no profile or
    /// terminal metadata attached yet.
    ///
    /// `debug_level` is clamped to the supported `0..=3` range.
    ///
    /// # Examples
    ///
    /// ```
    /// use osp_cli::core::{ColorMode, OutputFormat, UnicodeMode};
    /// use osp_cli::core::MessageLevel;
    /// use osp_cli::core::RuntimeHints;
    ///
    /// let hints = RuntimeHints::new(
    ///     MessageLevel::Info,
    ///     7,
    ///     OutputFormat::Json,
    ///     ColorMode::Always,
    ///     UnicodeMode::Never,
    /// );
    ///
    /// assert_eq!(hints.ui_verbosity, MessageLevel::Info);
    /// assert_eq!(hints.debug_level, 3);
    /// assert_eq!(hints.profile, None);
    /// ```
    pub fn new(
        ui_verbosity: MessageLevel,
        debug_level: u8,
        format: OutputFormat,
        color: ColorMode,
        unicode: UnicodeMode,
    ) -> Self {
        Self {
            ui_verbosity,
            debug_level: debug_level.min(3),
            format,
            color,
            unicode,
            profile: None,
            terminal: None,
            terminal_kind: RuntimeTerminalKind::Unknown,
        }
    }

    /// Reads runtime hints from the current process environment.
    pub fn from_env() -> Self {
        Self::from_env_iter(
            [
                ENV_OSP_UI_VERBOSITY,
                "OSP_COMMAND_VERBOSITY",
                ENV_OSP_DEBUG_LEVEL,
                ENV_OSP_FORMAT,
                ENV_OSP_COLOR,
                ENV_OSP_UNICODE,
                ENV_OSP_PROFILE,
                ENV_OSP_TERMINAL,
                ENV_OSP_TERMINAL_KIND,
            ]
            .into_iter()
            .filter_map(|name| std::env::var(name).ok().map(|value| (name, value))),
        )
    }

    /// Replaces the optional active-profile label.
    pub fn with_profile(mut self, profile: Option<String>) -> Self {
        self.profile = profile
            .map(|value| value.trim().to_string())
            .filter(|value| !value.is_empty());
        self
    }

    /// Replaces the optional terminal identifier.
    pub fn with_terminal(mut self, terminal: Option<String>) -> Self {
        self.terminal = terminal
            .map(|value| value.trim().to_string())
            .filter(|value| !value.is_empty());
        self
    }

    /// Replaces the terminal-kind hint.
    pub fn with_terminal_kind(mut self, terminal_kind: RuntimeTerminalKind) -> Self {
        self.terminal_kind = terminal_kind;
        self
    }

    /// Builds runtime hints from arbitrary key-value environment pairs.
    ///
    /// # Examples
    ///
    /// ```
    /// use osp_cli::core::{ColorMode, OutputFormat, UnicodeMode};
    /// use osp_cli::core::MessageLevel;
    /// use osp_cli::core::{RuntimeHints, RuntimeTerminalKind};
    ///
    /// let hints = RuntimeHints::from_env_iter([
    ///     ("OSP_UI_VERBOSITY", "trace"),
    ///     ("OSP_DEBUG_LEVEL", "7"),
    ///     ("OSP_FORMAT", "json"),
    ///     ("OSP_COLOR", "never"),
    ///     ("OSP_PROFILE", "uio"),
    ///     ("OSP_TERMINAL_KIND", "repl"),
    /// ]);
    ///
    /// assert_eq!(hints.ui_verbosity, MessageLevel::Trace);
    /// assert_eq!(hints.debug_level, 3);
    /// assert_eq!(hints.format, OutputFormat::Json);
    /// assert_eq!(hints.color, ColorMode::Never);
    /// assert_eq!(hints.unicode, UnicodeMode::Auto);
    /// assert_eq!(hints.profile.as_deref(), Some("uio"));
    /// assert_eq!(hints.terminal_kind, RuntimeTerminalKind::Repl);
    /// ```
    pub fn from_env_iter<I, K, V>(vars: I) -> Self
    where
        I: IntoIterator<Item = (K, V)>,
        K: AsRef<str>,
        V: AsRef<str>,
    {
        let values = vars
            .into_iter()
            .map(|(k, v)| (k.as_ref().to_string(), v.as_ref().to_string()))
            .collect::<HashMap<String, String>>();

        let mut ui_verbosity = values
            .get(ENV_OSP_UI_VERBOSITY)
            .and_then(|value| MessageLevel::parse(value))
            .unwrap_or(MessageLevel::Success);
        if values
            .get("OSP_COMMAND_VERBOSITY")
            .is_some_and(|v| v == "3")
        {
            ui_verbosity = MessageLevel::Evidence;
        }
        let debug_level = values
            .get(ENV_OSP_DEBUG_LEVEL)
            .and_then(|value| value.trim().parse::<u8>().ok())
            .unwrap_or(0)
            .min(3);
        let format = values
            .get(ENV_OSP_FORMAT)
            .and_then(|value| OutputFormat::parse(value))
            .unwrap_or(OutputFormat::Auto);
        let color = values
            .get(ENV_OSP_COLOR)
            .and_then(|value| ColorMode::parse(value))
            .unwrap_or(ColorMode::Auto);
        let unicode = values
            .get(ENV_OSP_UNICODE)
            .and_then(|value| UnicodeMode::parse(value))
            .unwrap_or(UnicodeMode::Auto);
        let profile = values
            .get(ENV_OSP_PROFILE)
            .map(String::as_str)
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(ToOwned::to_owned);
        let terminal = values
            .get(ENV_OSP_TERMINAL)
            .map(String::as_str)
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(ToOwned::to_owned);
        let terminal_kind = values
            .get(ENV_OSP_TERMINAL_KIND)
            .and_then(|value| RuntimeTerminalKind::parse(value))
            .or_else(|| {
                values
                    .get(ENV_OSP_TERMINAL)
                    .and_then(|value| RuntimeTerminalKind::parse(value))
            })
            .unwrap_or(RuntimeTerminalKind::Unknown);

        Self::new(ui_verbosity, debug_level, format, color, unicode)
            .with_profile(profile)
            .with_terminal(terminal)
            .with_terminal_kind(terminal_kind)
    }

    /// Returns this hint set as environment variable pairs suitable for export.
    ///
    /// # Examples
    ///
    /// ```
    /// use osp_cli::core::{ColorMode, OutputFormat, UnicodeMode};
    /// use osp_cli::core::MessageLevel;
    /// use osp_cli::core::{RuntimeHints, RuntimeTerminalKind};
    ///
    /// let hints = RuntimeHints::new(
    ///     MessageLevel::Info,
    ///     2,
    ///     OutputFormat::Json,
    ///     ColorMode::Always,
    ///     UnicodeMode::Never,
    /// )
    /// .with_profile(Some("uio".to_string()))
    /// .with_terminal(Some("xterm-256color".to_string()))
    /// .with_terminal_kind(RuntimeTerminalKind::Cli);
    /// let pairs = hints.env_pairs();
    ///
    /// assert!(pairs.iter().any(|(key, value)| *key == "OSP_FORMAT" && value == "json"));
    /// assert!(pairs.iter().any(|(key, value)| *key == "OSP_PROFILE" && value == "uio"));
    /// ```
    pub fn env_pairs(&self) -> Vec<(&'static str, String)> {
        let mut out = vec![
            (
                ENV_OSP_UI_VERBOSITY,
                if self.ui_verbosity >= MessageLevel::Trace {
                    "trace"
                } else {
                    self.ui_verbosity.as_str()
                }
                .to_string(),
            ),
            (
                "OSP_COMMAND_VERBOSITY",
                self.ui_verbosity.command_verbosity().to_string(),
            ),
            (ENV_OSP_DEBUG_LEVEL, self.debug_level.min(3).to_string()),
            (ENV_OSP_FORMAT, self.format.as_str().to_string()),
            (ENV_OSP_COLOR, self.color.as_str().to_string()),
            (ENV_OSP_UNICODE, self.unicode.as_str().to_string()),
            (
                ENV_OSP_TERMINAL_KIND,
                self.terminal_kind.as_str().to_string(),
            ),
        ];

        if let Some(profile) = &self.profile {
            out.push((ENV_OSP_PROFILE, profile.clone()));
        }
        if let Some(terminal) = &self.terminal {
            out.push((ENV_OSP_TERMINAL, terminal.clone()));
        }

        out
    }
}

#[cfg(test)]
mod tests {
    use super::{
        ENV_OSP_COLOR, ENV_OSP_DEBUG_LEVEL, ENV_OSP_FORMAT, ENV_OSP_PROFILE, ENV_OSP_TERMINAL,
        ENV_OSP_UI_VERBOSITY, ENV_OSP_UNICODE, MessageLevel, RuntimeHints, RuntimeTerminalKind,
    };
    use crate::core::output::{ColorMode, OutputFormat, UnicodeMode};

    #[test]
    fn env_roundtrip_keeps_runtime_hints() {
        let hints = RuntimeHints::new(
            MessageLevel::Trace,
            7,
            OutputFormat::Json,
            ColorMode::Never,
            UnicodeMode::Always,
        )
        .with_profile(Some("uio".to_string()))
        .with_terminal(Some("xterm-256color".to_string()))
        .with_terminal_kind(RuntimeTerminalKind::Repl);

        let parsed = RuntimeHints::from_env_iter(hints.env_pairs());
        assert_eq!(parsed.ui_verbosity, MessageLevel::Trace);
        assert_eq!(parsed.debug_level, 3);
        assert_eq!(parsed.format, OutputFormat::Json);
        assert_eq!(parsed.color, ColorMode::Never);
        assert_eq!(parsed.unicode, UnicodeMode::Always);
        assert_eq!(parsed.profile.as_deref(), Some("uio"));
        assert_eq!(parsed.terminal.as_deref(), Some("xterm-256color"));
        assert_eq!(parsed.terminal_kind, RuntimeTerminalKind::Repl);
    }

    #[test]
    fn new_and_with_helpers_build_runtime_hints_unit() {
        let hints = RuntimeHints::new(
            MessageLevel::Info,
            9,
            OutputFormat::Table,
            ColorMode::Always,
            UnicodeMode::Never,
        )
        .with_profile(Some("  dev  ".to_string()))
        .with_terminal(Some(" xterm-256color ".to_string()))
        .with_terminal_kind(RuntimeTerminalKind::Cli);

        assert_eq!(hints.ui_verbosity, MessageLevel::Info);
        assert_eq!(hints.debug_level, 3);
        assert_eq!(hints.profile.as_deref(), Some("dev"));
        assert_eq!(hints.terminal.as_deref(), Some("xterm-256color"));
        assert_eq!(hints.terminal_kind, RuntimeTerminalKind::Cli);
    }

    #[test]
    fn from_env_defaults_when_vars_missing_or_invalid() {
        let parsed = RuntimeHints::from_env_iter(vec![
            (ENV_OSP_UI_VERBOSITY, "loud"),
            (ENV_OSP_DEBUG_LEVEL, "NaN"),
            (ENV_OSP_FORMAT, "???"),
            (ENV_OSP_COLOR, "blue"),
            (ENV_OSP_UNICODE, "emoji"),
        ]);

        assert_eq!(parsed.ui_verbosity, MessageLevel::Success);
        assert_eq!(parsed.debug_level, 0);
        assert_eq!(parsed.format, OutputFormat::Auto);
        assert_eq!(parsed.color, ColorMode::Auto);
        assert_eq!(parsed.unicode, UnicodeMode::Auto);
        assert_eq!(parsed.profile, None);
        assert_eq!(parsed.terminal, None);
        assert_eq!(parsed.terminal_kind, RuntimeTerminalKind::Unknown);
    }

    #[test]
    fn terminal_kind_falls_back_to_terminal_env() {
        let parsed =
            RuntimeHints::from_env_iter(vec![(ENV_OSP_TERMINAL, "repl"), (ENV_OSP_PROFILE, "tsd")]);

        assert_eq!(parsed.profile.as_deref(), Some("tsd"));
        assert_eq!(parsed.terminal.as_deref(), Some("repl"));
        assert_eq!(parsed.terminal_kind, RuntimeTerminalKind::Repl);
    }
}
