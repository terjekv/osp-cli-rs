use std::ops::Range;

use crate::config::SchemaValueType;
use miette::{LabeledSpan, NamedSource, SourceSpan};

/// Structured TOML parse details preserved for richer CLI diagnostics.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TomlParseDiagnostic {
    message: String,
    source_code: Option<NamedSource<String>>,
    span: Option<SourceSpan>,
}

impl TomlParseDiagnostic {
    /// Creates a TOML parse diagnostic with only the parser message.
    #[must_use]
    pub fn new(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
            source_code: None,
            span: None,
        }
    }

    /// Attaches source text and an optional parser span for rich rendering.
    #[must_use]
    pub fn with_source(
        mut self,
        source_name: impl Into<String>,
        source_text: impl Into<String>,
        span: Option<Range<usize>>,
    ) -> Self {
        let source_text = source_text.into();
        self = self.with_location_from_source(&source_text, span.clone());
        self.source_code = Some(NamedSource::new(source_name.into(), source_text));
        self.span = span.map(|range| (range.start, range.len()).into());
        self
    }

    /// Records a safe line and column without retaining the source document.
    pub(crate) fn with_location_from_source(
        mut self,
        source_text: &str,
        span: Option<Range<usize>>,
    ) -> Self {
        if let Some(start) = span.map(|span| span.start.min(source_text.len()))
            && let Some(prefix) = source_text.get(..start)
        {
            let line = prefix.bytes().filter(|byte| *byte == b'\n').count() + 1;
            let column = prefix
                .rsplit_once('\n')
                .map_or(prefix, |(_, tail)| tail)
                .chars()
                .count()
                + 1;
            self.message = format!("{} at line {line}, column {column}", self.message);
        }
        self
    }

    /// Returns the parser message without rendering additional context.
    #[must_use]
    pub fn message(&self) -> &str {
        &self.message
    }

    /// Returns the attached source text when the parser failure came from a known document.
    #[must_use]
    pub fn source_code(&self) -> Option<&NamedSource<String>> {
        self.source_code.as_ref()
    }

    /// Returns a label for the offending TOML span when location data is available.
    #[must_use]
    pub fn labels(&self) -> Option<Vec<LabeledSpan>> {
        Some(vec![LabeledSpan::at(
            self.span?,
            "invalid TOML starts here",
        )])
    }
}

/// Error type returned by config parsing, validation, and resolution code.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum ConfigError {
    /// Reading a config file from disk failed.
    #[error("failed to read config file {path}: {reason}")]
    FileRead {
        /// Path that could not be read.
        path: String,
        /// Lower-level failure description.
        reason: String,
    },
    /// Writing a config file to disk failed.
    #[error("failed to write config file {path}: {reason}")]
    FileWrite {
        /// Path that could not be written.
        path: String,
        /// Lower-level failure description.
        reason: String,
    },
    /// Adds path context while propagating another config error.
    #[error("{inner} (path: {path})")]
    LayerLoad {
        /// Path of the layer being loaded.
        path: String,
        /// Underlying config error.
        inner: Box<ConfigError>,
    },
    /// Secrets file permissions are broader than allowed.
    #[error("insecure permissions on secrets file {path}: mode {mode:o}, expected 600")]
    InsecureSecretsPermissions {
        /// Path of the secrets file.
        path: String,
        /// Observed Unix file mode.
        mode: u32,
    },
    /// A selected persistent secrets backend failed.
    #[error("{backend} secrets backend failed: {reason}")]
    SecretBackend {
        /// Stable backend name.
        backend: String,
        /// Backend-specific failure without secret contents.
        reason: String,
    },
    /// TOML parsing failed before semantic validation.
    #[error("failed to parse TOML: {}", .0.message())]
    TomlParse(TomlParseDiagnostic),
    /// The parsed TOML document root was not a table.
    #[error("config root must be a TOML table")]
    TomlRootMustBeTable,
    /// Encountered an unsupported top-level section name.
    #[error("unknown top-level config section: {0}")]
    UnknownTopLevelSection(String),
    /// A named section had the wrong TOML type.
    #[error("invalid section {section}: expected {expected}")]
    InvalidSection {
        /// Fully qualified section name.
        section: String,
        /// Expected TOML kind for the section.
        expected: String,
    },
    /// Encountered a TOML value kind that the config model does not accept.
    #[error("unsupported TOML value at {path}: {kind}")]
    UnsupportedTomlValue {
        /// Dotted config path at which the value was found.
        path: String,
        /// TOML value kind that was rejected.
        kind: String,
    },
    /// An `OSP__...` environment override used invalid syntax or scope.
    #[error("invalid env override {key}: {reason}")]
    InvalidEnvOverride {
        /// Environment variable name or derived config key.
        key: String,
        /// Validation failure description.
        reason: String,
    },
    /// A config key failed syntactic or semantic validation.
    #[error("invalid config key {key}: {reason}")]
    InvalidConfigKey {
        /// Key that failed validation.
        key: String,
        /// Validation failure description.
        reason: String,
    },
    /// A caller attempted to write a read-only config key.
    #[error("config key {key} is read-only: {reason}")]
    ReadOnlyConfigKey {
        /// Key that is read-only.
        key: String,
        /// Explanation of why the key is immutable.
        reason: String,
    },
    /// A bootstrap-only key was used in a disallowed scope.
    #[error("bootstrap-only key {key} is not allowed in scope {}; allowed scopes: global or terminal-only", bootstrap_scope(.profile, .terminal))]
    InvalidBootstrapScope {
        /// Bootstrap-only key being validated.
        key: String,
        /// Profile selector present on the rejected scope, if any.
        profile: Option<String>,
        /// Terminal selector present on the rejected scope, if any.
        terminal: Option<String>,
    },
    /// No usable bootstrap profile name could be determined during bootstrap.
    ///
    /// This includes both "no default profile exists" and "the selected
    /// bootstrap profile normalized to empty".
    #[error("missing profile.default and no fallback profile")]
    MissingDefaultProfile,
    /// A bootstrap-only key failed its value validation rule.
    #[error("invalid bootstrap value for {key}: {reason}")]
    InvalidBootstrapValue {
        /// Bootstrap-only key being validated.
        key: String,
        /// Validation failure description.
        reason: String,
    },
    /// Resolution requested a profile that is not known.
    #[error("Unknown profile '{profile}'. Available: {}.", .known.join(", "))]
    UnknownProfile {
        /// Requested profile name.
        profile: String,
        /// Known profile names at the time of resolution.
        known: Vec<String>,
    },
    /// Placeholder syntax inside a template string is malformed.
    #[error("invalid placeholder syntax in key {key}: {template}")]
    InvalidPlaceholderSyntax {
        /// Key containing the invalid template.
        key: String,
        /// Original template string.
        template: String,
    },
    /// Placeholder expansion referenced an unknown key.
    #[error("unresolved placeholder in key {key}: {placeholder}")]
    UnresolvedPlaceholder {
        /// Key whose template is being resolved.
        key: String,
        /// Placeholder that could not be resolved.
        placeholder: String,
    },
    /// Placeholder expansion formed a dependency cycle.
    #[error("placeholder cycle detected: {}", .cycle.join(" -> "))]
    PlaceholderCycle {
        /// Ordered key path describing the detected cycle.
        cycle: Vec<String>,
    },
    /// Placeholder expansion referenced a non-scalar value.
    #[error("placeholder {placeholder} in key {key} points to a non-scalar value")]
    NonScalarPlaceholder {
        /// Key whose template is being resolved.
        key: String,
        /// Placeholder that resolved to a non-scalar value.
        placeholder: String,
    },
    /// One or more config keys are unknown to the schema.
    #[error("unknown config keys: {}", .keys.join(", "))]
    UnknownConfigKeys {
        /// Unknown canonical keys.
        keys: Vec<String>,
    },
    /// A required runtime-visible key was missing after resolution.
    #[error("missing required config key: {key}")]
    MissingRequiredKey {
        /// Missing canonical key.
        key: String,
    },
    /// A value could not be adapted to the schema's declared type.
    #[error("invalid type for key {key}: expected {expected}, got {actual}")]
    InvalidValueType {
        /// Key whose value had the wrong type.
        key: String,
        /// Schema type expected for the key.
        expected: SchemaValueType,
        /// Actual type name observed during adaptation.
        actual: String,
    },
    /// A typed config value violated a schema-owned value constraint.
    #[error("invalid value for key {key}: {reason}")]
    InvalidConfigValue {
        /// Key whose value was rejected.
        key: String,
        /// Validation failure description.
        reason: String,
    },
    /// A string value was outside the schema allow-list.
    #[error("invalid value for key {key}: {value}. allowed: {}", .allowed.join(", "))]
    InvalidEnumValue {
        /// Key whose value was rejected.
        key: String,
        /// Rejected value.
        value: String,
        /// Allowed normalized values.
        allowed: Vec<String>,
    },
}

fn bootstrap_scope(profile: &Option<String>, terminal: &Option<String>) -> String {
    match (profile.as_deref(), terminal.as_deref()) {
        (Some(profile), Some(terminal)) => format!("profile={profile}, terminal={terminal}"),
        (Some(profile), None) => format!("profile={profile}"),
        (None, Some(terminal)) => format!("terminal={terminal}"),
        (None, None) => "global".to_string(),
    }
}

impl miette::Diagnostic for ConfigError {
    fn source_code(&self) -> Option<&dyn miette::SourceCode> {
        match self {
            ConfigError::TomlParse(diagnostic) => diagnostic
                .source_code()
                .map(|source| source as &dyn miette::SourceCode),
            ConfigError::LayerLoad { inner, .. } => inner.source_code(),
            _ => None,
        }
    }

    fn labels(&self) -> Option<Box<dyn Iterator<Item = LabeledSpan> + '_>> {
        match self {
            ConfigError::TomlParse(diagnostic) => diagnostic.labels().map(|labels| {
                Box::new(labels.into_iter()) as Box<dyn Iterator<Item = LabeledSpan>>
            }),
            ConfigError::LayerLoad { inner, .. } => inner.labels(),
            _ => None,
        }
    }

    fn diagnostic_source(&self) -> Option<&dyn miette::Diagnostic> {
        match self {
            ConfigError::LayerLoad { inner, .. } => Some(inner.as_ref()),
            _ => None,
        }
    }
}

pub(crate) fn with_path_context(path: String, error: ConfigError) -> ConfigError {
    ConfigError::LayerLoad {
        path,
        inner: Box::new(error),
    }
}

#[cfg(test)]
mod tests {
    use super::{ConfigError, TomlParseDiagnostic, with_path_context};
    use crate::config::SchemaValueType;

    #[test]
    fn config_error_display_and_path_context_cover_user_facing_variants_unit() {
        let cases = [
            (
                ConfigError::FileRead {
                    path: "/tmp/config.toml".to_string(),
                    reason: "permission denied".to_string(),
                },
                "failed to read config file /tmp/config.toml: permission denied",
            ),
            (
                ConfigError::FileWrite {
                    path: "/tmp/config.toml".to_string(),
                    reason: "disk full".to_string(),
                },
                "failed to write config file /tmp/config.toml: disk full",
            ),
            (
                ConfigError::InsecureSecretsPermissions {
                    path: "/tmp/secrets.toml".to_string(),
                    mode: 0o644,
                },
                "expected 600",
            ),
            (
                ConfigError::TomlParse(TomlParseDiagnostic::new("unexpected token")),
                "failed to parse TOML: unexpected token",
            ),
            (
                ConfigError::TomlRootMustBeTable,
                "config root must be a TOML table",
            ),
            (
                ConfigError::UnknownTopLevelSection("wat".to_string()),
                "unknown top-level config section: wat",
            ),
            (
                ConfigError::InvalidSection {
                    section: "profile.default".to_string(),
                    expected: "table".to_string(),
                },
                "invalid section profile.default: expected table",
            ),
            (
                ConfigError::UnsupportedTomlValue {
                    path: "ui.format".to_string(),
                    kind: "array".to_string(),
                },
                "unsupported TOML value at ui.format: array",
            ),
            (
                ConfigError::InvalidEnvOverride {
                    key: "OSP_UI_FORMAT".to_string(),
                    reason: "unknown enum".to_string(),
                },
                "invalid env override OSP_UI_FORMAT: unknown enum",
            ),
            (
                ConfigError::InvalidConfigKey {
                    key: "ui.wat".to_string(),
                    reason: "unknown key".to_string(),
                },
                "invalid config key ui.wat: unknown key",
            ),
            (
                ConfigError::ReadOnlyConfigKey {
                    key: "profile.active".to_string(),
                    reason: "derived at runtime".to_string(),
                },
                "config key profile.active is read-only: derived at runtime",
            ),
            (
                ConfigError::InvalidBootstrapScope {
                    key: "profile.default".to_string(),
                    profile: Some("prod".to_string()),
                    terminal: Some("repl".to_string()),
                },
                "profile=prod, terminal=repl",
            ),
            (
                ConfigError::InvalidBootstrapScope {
                    key: "profile.default".to_string(),
                    profile: Some("prod".to_string()),
                    terminal: None,
                },
                "scope profile=prod",
            ),
            (
                ConfigError::InvalidBootstrapScope {
                    key: "profile.default".to_string(),
                    profile: None,
                    terminal: Some("repl".to_string()),
                },
                "scope terminal=repl",
            ),
            (
                ConfigError::InvalidBootstrapScope {
                    key: "profile.default".to_string(),
                    profile: None,
                    terminal: None,
                },
                "scope global",
            ),
            (
                ConfigError::MissingDefaultProfile,
                "missing profile.default and no fallback profile",
            ),
            (
                ConfigError::InvalidBootstrapValue {
                    key: "profile.default".to_string(),
                    reason: "cannot be empty".to_string(),
                },
                "invalid bootstrap value for profile.default: cannot be empty",
            ),
            (
                ConfigError::UnknownProfile {
                    profile: "prod".to_string(),
                    known: vec!["default".to_string(), "dev".to_string()],
                },
                "Unknown profile 'prod'. Available: default, dev.",
            ),
            (
                ConfigError::InvalidPlaceholderSyntax {
                    key: "ui.format".to_string(),
                    template: "${oops".to_string(),
                },
                "invalid placeholder syntax in key ui.format: ${oops",
            ),
            (
                ConfigError::UnresolvedPlaceholder {
                    key: "ldap.uri".to_string(),
                    placeholder: "profile.current".to_string(),
                },
                "unresolved placeholder in key ldap.uri: profile.current",
            ),
            (
                ConfigError::PlaceholderCycle {
                    cycle: vec!["a".to_string(), "b".to_string(), "a".to_string()],
                },
                "placeholder cycle detected: a -> b -> a",
            ),
            (
                ConfigError::NonScalarPlaceholder {
                    key: "ldap.uri".to_string(),
                    placeholder: "profiles".to_string(),
                },
                "placeholder profiles in key ldap.uri points to a non-scalar value",
            ),
            (
                ConfigError::UnknownConfigKeys {
                    keys: vec!["ui.wat".to_string(), "ldap.nope".to_string()],
                },
                "unknown config keys: ui.wat, ldap.nope",
            ),
            (
                ConfigError::MissingRequiredKey {
                    key: "ldap.uri".to_string(),
                },
                "missing required config key: ldap.uri",
            ),
            (
                ConfigError::InvalidValueType {
                    key: "ui.debug".to_string(),
                    expected: SchemaValueType::Bool,
                    actual: "string".to_string(),
                },
                "invalid type for key ui.debug: expected bool, got string",
            ),
            (
                ConfigError::InvalidEnumValue {
                    key: "ui.format".to_string(),
                    value: "yaml".to_string(),
                    allowed: vec!["json".to_string(), "table".to_string()],
                },
                "invalid value for key ui.format: yaml. allowed: json, table",
            ),
        ];

        assert!(
            cases
                .into_iter()
                .all(|(error, expected)| error.to_string().contains(expected))
        );

        let wrapped = with_path_context(
            "/tmp/config.toml".to_string(),
            ConfigError::TomlParse(TomlParseDiagnostic::new("bad value")),
        );

        assert_eq!(
            wrapped.to_string(),
            "failed to parse TOML: bad value (path: /tmp/config.toml)"
        );

        if let ConfigError::LayerLoad { path, inner } = wrapped {
            assert_eq!(path, "/tmp/config.toml");
            assert!(matches!(*inner, ConfigError::TomlParse(_)));
        }
    }

    #[test]
    fn toml_parse_diagnostic_preserves_optional_source_and_span_unit() {
        let diagnostic = TomlParseDiagnostic::new("expected `]`").with_source(
            "config.toml",
            "not = [valid\n",
            Some(6..12),
        );
        let error = ConfigError::TomlParse(diagnostic.clone());

        assert!(error.to_string().contains("expected `]`"));
        assert!(diagnostic.source_code().is_some());
        let rendered = format!("{:?}", miette::Report::new(error));
        assert!(rendered.contains("config.toml"));
        assert!(rendered.contains("invalid TOML starts here"));
        assert!(rendered.contains("not = [valid"));
    }
}
