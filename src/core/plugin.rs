//! Wire-format DTOs for the plugin protocol.
//!
//! This module exists to define the stable boundary between `osp-cli` and
//! external plugins. The app and plugin manager can evolve internally, but the
//! JSON shapes in this module are the contract that both sides need to agree
//! on.
//!
//! In broad terms:
//!
//! - `Describe*` types advertise commands, arguments, and policy metadata
//! - `Response*` types carry execution results, messages, and render hints
//! - validation helpers reject protocol-shape errors before higher-level code
//!   tries to trust the payload
//!
//! Wire flow:
//!
//! ```text
//! plugin executable
//!      │
//!      ├── `describe` -> PluginDescribe / DescribeCommandV1
//!      │                host builds command catalog, completion, and policy
//!      │
//!      └── `run`      -> ResponseV1
//!                       host validates payload before adapting/rendering it
//! ```
//!
//! Useful mental split:
//!
//! - plugin authors care about these types as the stable JSON contract
//! - host-side code cares about them as validated input before converting into
//!   command catalogs, policy registries, and rendered output
//!
//! Clap-backed convenience constructors such as
//! [`crate::core::plugin::DescribeCommandV1::from_clap`] translate command
//! trees into the wire DTOs.
//!
//! Contract:
//!
//! - these types may depend on shared `core` metadata, but they should stay
//!   free of host runtime concerns
//! - any parsing/validation here should enforce protocol rules, not business
//!   policy
//! - caller-facing docs should describe stable wire behavior rather than
//!   internal plugin manager details

use crate::core::MessageLevel;
use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use crate::core::clap_metadata as clap_meta;
use crate::core::command_policy::CommandPath;

/// Current plugin wire protocol version understood by this crate.
pub const PLUGIN_PROTOCOL_V1: u32 = 1;

/// Optional reply to `--osp-describe`; the executable filename supplies the name.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct PluginDescribe {
    /// Entity production and consumption declarations.
    #[serde(flatten)]
    pub context: crate::core::context::ContextContract,
    /// Short help text.
    #[serde(default)]
    pub about: String,
    /// Set to `json` to render stdout through the host by default.
    #[serde(default)]
    pub output: Option<String>,
    /// Authorization metadata for this command.
    #[serde(default)]
    pub auth: Option<DescribeCommandAuthV1>,
    /// Positional arguments.
    #[serde(default)]
    pub args: Vec<DescribeArgV1>,
    /// Flags keyed by their spelling.
    #[serde(default)]
    pub flags: BTreeMap<String, DescribeFlagV1>,
    /// Nested commands.
    #[serde(default)]
    pub subcommands: Vec<DescribeCommandV1>,
}
impl PluginDescribe {
    pub(crate) fn command(self, name: &str) -> DescribeCommandV1 {
        let mut command = DescribeCommandV1 {
            context: self.context,
            name: name.into(),
            about: self.about,
            auth: self.auth,
            args: self.args,
            flags: self.flags,
            subcommands: self.subcommands,
        };
        command.inherit_auth_from_parents();
        command
    }
}

/// Recursive command description used in plugin metadata.
///
/// Each node describes one command segment plus its direct flags, positionals,
/// auth metadata, and nested subcommands. Together these nodes form the
/// semantic command tree the host uses for help, completion, and policy lookups.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DescribeCommandV1 {
    /// Entity production and consumption declarations.
    #[serde(flatten)]
    pub context: crate::core::context::ContextContract,
    /// Command name exposed by the plugin.
    pub name: String,
    /// Short help text for the command.
    #[serde(default)]
    pub about: String,
    /// Optional authorization metadata for the command.
    #[serde(default)]
    pub auth: Option<DescribeCommandAuthV1>,
    /// Positional argument descriptions in declaration order.
    #[serde(default)]
    pub args: Vec<DescribeArgV1>,
    /// Flag descriptions keyed by protocol flag spelling.
    #[serde(default)]
    pub flags: BTreeMap<String, DescribeFlagV1>,
    /// Nested subcommands under this command.
    #[serde(default)]
    pub subcommands: Vec<DescribeCommandV1>,
}

/// Authorization metadata attached to a described command.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct DescribeCommandAuthV1 {
    /// Visibility level for the command.
    #[serde(default)]
    pub visibility: Option<DescribeVisibilityModeV1>,
    /// Legacy capabilities, accepted for wire compatibility and ignored.
    #[serde(default)]
    pub required_capabilities: Vec<String>,
    /// Legacy feature flags, accepted for wire compatibility and ignored.
    #[serde(default)]
    pub feature_flags: Vec<String>,
    /// Legacy visibility requirements, accepted and ignored.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub visible_session: Option<DescribeSessionRequirementsV1>,
    /// Session-state requirements that gate command execution.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub run_session: Option<DescribeSessionRequirementsV1>,
}

/// Wire-format auth strength used by plugin metadata.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DescribeAuthStrengthV1 {
    /// Standard authenticated session strength.
    Basic,
    /// Stronger authentication such as MFA-backed login.
    Strong,
}

/// Wire-format credential requirement used by plugin metadata.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", tag = "state")]
pub enum DescribeCredentialRequirementV1 {
    /// Require that the named credential exists.
    Present {
        /// Credential or token namespace, such as `osp` or `mreg`.
        service: String,
    },
    /// Require that the named credential exists and is currently valid.
    Valid {
        /// Credential or token namespace, such as `osp` or `mreg`.
        service: String,
    },
    /// Legacy freshness requirement, accepted and ignored.
    Fresh {
        /// Credential or token namespace, such as `osp` or `mreg`.
        service: String,
        /// Minimum remaining lifetime required for the command.
        min_ttl_seconds: u64,
    },
}

impl DescribeCredentialRequirementV1 {
    fn hint(&self) -> String {
        match self {
            Self::Present { service } => format!("token: {service}"),
            Self::Valid { service } => format!("token: {service} valid"),
            Self::Fresh {
                service,
                min_ttl_seconds,
            } => format!("token: {service} fresh({min_ttl_seconds}s)"),
        }
    }

    fn service(&self) -> &str {
        match self {
            Self::Present { service } | Self::Valid { service } | Self::Fresh { service, .. } => {
                service
            }
        }
    }
}

/// Wire-format session requirements attached to a command.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct DescribeSessionRequirementsV1 {
    /// Legacy auth strength, accepted and ignored.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub auth_strength: Option<DescribeAuthStrengthV1>,
    /// Credential or token requirements that must be satisfied.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub credentials: Vec<DescribeCredentialRequirementV1>,
}

impl DescribeSessionRequirementsV1 {
    fn hint(&self) -> Option<String> {
        let mut parts = Vec::new();
        parts.extend(
            self.credentials
                .iter()
                .filter(|credential| {
                    !matches!(credential, DescribeCredentialRequirementV1::Fresh { .. })
                })
                .map(DescribeCredentialRequirementV1::hint),
        );
        (!parts.is_empty()).then(|| parts.join(", "))
    }
}

/// Wire-format visibility mode used by plugin metadata.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DescribeVisibilityModeV1 {
    /// Command is visible and runnable without authentication.
    Public,
    /// Command requires an authenticated user.
    Authenticated,
    /// Legacy capability mode, accepted and treated as public.
    CapabilityGated,
    /// Command should be hidden from normal help surfaces.
    Hidden,
}

impl DescribeVisibilityModeV1 {
    /// Returns the canonical protocol label for this visibility mode.
    ///
    /// # Examples
    ///
    /// ```
    /// use osp_cli::core::plugin::DescribeVisibilityModeV1;
    ///
    /// assert_eq!(DescribeVisibilityModeV1::Hidden.as_label(), "hidden");
    /// ```
    pub fn as_label(self) -> &'static str {
        match self {
            DescribeVisibilityModeV1::Public => "public",
            DescribeVisibilityModeV1::Authenticated => "authenticated",
            DescribeVisibilityModeV1::CapabilityGated => "capability_gated",
            DescribeVisibilityModeV1::Hidden => "hidden",
        }
    }
}

impl DescribeCommandAuthV1 {
    /// Returns a compact help hint for non-default auth requirements.
    ///
    /// This is meant for help and completion surfaces where the full policy
    /// object would be too noisy.
    ///
    /// # Examples
    ///
    /// ```
    /// use osp_cli::core::plugin::{
    ///     DescribeAuthStrengthV1, DescribeCommandAuthV1,
    ///     DescribeCredentialRequirementV1, DescribeSessionRequirementsV1,
    ///     DescribeVisibilityModeV1,
    /// };
    ///
    /// let auth = DescribeCommandAuthV1 {
    ///     visibility: Some(DescribeVisibilityModeV1::CapabilityGated),
    ///     required_capabilities: vec!["ldap.write".to_string()],
    ///     feature_flags: vec!["write-mode".to_string()],
    ///     run_session: Some(DescribeSessionRequirementsV1 {
    ///         auth_strength: Some(DescribeAuthStrengthV1::Strong),
    ///         credentials: vec![DescribeCredentialRequirementV1::valid("osp")],
    ///     }),
    ///     ..DescribeCommandAuthV1::default()
    /// };
    ///
    /// assert_eq!(
    ///     auth.hint().as_deref(),
    ///     Some("run: token: osp valid")
    /// );
    /// ```
    pub fn hint(&self) -> Option<String> {
        let mut parts = Vec::new();

        match self.visibility {
            Some(DescribeVisibilityModeV1::Public) | None => {}
            Some(DescribeVisibilityModeV1::Authenticated) => parts.push("auth".to_string()),
            Some(DescribeVisibilityModeV1::CapabilityGated) => {}
            Some(DescribeVisibilityModeV1::Hidden) => parts.push("hidden".to_string()),
        }

        if let Some(session) = self.run_session.as_ref().and_then(|session| session.hint()) {
            parts.push(format!("run: {session}"));
        }

        (!parts.is_empty()).then(|| parts.join("; "))
    }
}

impl DescribeCredentialRequirementV1 {
    /// Creates a wire requirement that the named credential exists.
    pub fn present(service: impl Into<String>) -> Self {
        Self::Present {
            service: service.into(),
        }
    }

    /// Creates a wire requirement that the named credential exists and is valid.
    pub fn valid(service: impl Into<String>) -> Self {
        Self::Valid {
            service: service.into(),
        }
    }

    /// Creates a wire requirement that the named credential exists, is valid,
    /// and is fresh.
    pub fn fresh(service: impl Into<String>, min_ttl_seconds: u64) -> Self {
        Self::Fresh {
            service: service.into(),
            min_ttl_seconds,
        }
    }
}

/// Wire-format type hint for plugin argument values.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum DescribeValueTypeV1 {
    /// Value represents a filesystem path.
    Path,
}

/// Suggested value emitted in plugin metadata.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct DescribeSuggestionV1 {
    /// Raw suggestion value inserted into the command line.
    pub value: String,
    /// Optional short metadata string.
    #[serde(default)]
    pub meta: Option<String>,
    /// Optional display label for menu rendering.
    #[serde(default)]
    pub display: Option<String>,
    /// Optional sort key used for ordering suggestions.
    #[serde(default)]
    pub sort: Option<String>,
}

/// Positional argument description emitted by a plugin.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct DescribeArgV1 {
    /// Positional name or value label.
    #[serde(default)]
    pub name: Option<String>,
    /// Short help text for the argument.
    #[serde(default)]
    pub about: Option<String>,
    /// Whether the argument must be supplied.
    #[serde(default)]
    pub required: bool,
    /// Whether the argument may be repeated.
    #[serde(default)]
    pub multi: bool,
    /// Optional wire-format value type hint.
    #[serde(default)]
    pub value_type: Option<DescribeValueTypeV1>,
    /// Suggested values for the argument.
    #[serde(default)]
    pub suggestions: Vec<DescribeSuggestionV1>,
}

/// Flag description emitted by a plugin.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct DescribeFlagV1 {
    /// Canonical spelling for this entry when it is an alias.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub alias_of: Option<String>,
    /// Short help text for the flag.
    #[serde(default)]
    pub about: Option<String>,
    /// Whether the flag must be supplied.
    #[serde(default)]
    pub required: bool,
    /// Whether the flag is boolean-only and takes no value.
    #[serde(default)]
    pub flag_only: bool,
    /// Whether the flag may be repeated.
    #[serde(default)]
    pub multi: bool,
    /// Optional wire-format value type hint.
    #[serde(default)]
    pub value_type: Option<DescribeValueTypeV1>,
    /// Suggested values for the flag.
    #[serde(default)]
    pub suggestions: Vec<DescribeSuggestionV1>,
}

/// Protocol v1 command response envelope.
///
/// This is the execute-time contract for normal plugin runs. The host validates
/// this envelope first and only then adapts `data`, `messages`, and `meta`
/// into its own output pipeline.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ResponseV1 {
    /// Protocol version declared by the response.
    pub protocol_version: u32,
    /// Whether the command completed successfully.
    pub ok: bool,
    /// Response payload produced by the plugin.
    pub data: serde_json::Value,
    /// Decoded record before display shaping, when it differs from `data`.
    /// The host selects it with `--source-record` and ignores `data`'s display metadata.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source: Option<serde_json::Value>,
    /// Structured error payload present when `ok` is `false`.
    pub error: Option<ResponseErrorV1>,
    /// User-facing messages emitted alongside the payload.
    #[serde(default)]
    pub messages: Vec<ResponseMessageV1>,
    /// Rendering and presentation metadata for the payload.
    #[serde(default)]
    pub meta: ResponseMetaV1,
    /// Optional values the host should remember in its current session,
    /// keyed by subject kind (for example `host` or `user`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub context: Option<BTreeMap<String, String>>,
}

impl ResponseV1 {
    /// Creates a successful protocol v1 response with default metadata.
    pub fn ok(data: impl Into<serde_json::Value>) -> Self {
        Self {
            protocol_version: PLUGIN_PROTOCOL_V1,
            ok: true,
            data: data.into(),
            source: None,
            error: None,
            messages: Vec::new(),
            meta: ResponseMetaV1::default(),
            context: None,
        }
    }
}

/// Structured error payload returned when `ok` is `false`.
///
/// Keep stable, caller-meaningful failure classification in `code`; process
/// exit codes are reserved for transport/setup failure rather than
/// application-level branching.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ResponseErrorV1 {
    /// Stable machine-readable error code.
    pub code: String,
    /// Human-readable error message.
    pub message: String,
    /// Arbitrary structured error details.
    #[serde(default)]
    pub details: serde_json::Value,
}

/// Rendering hints attached to a plugin response.
///
/// These hints are advisory rather than authoritative. They let a plugin keep
/// semantic ownership of preferred column order and alignment without taking
/// over the host's renderer.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ResponseMetaV1 {
    /// The entity targeted by this response was not found; clear filled read context.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub context_not_found: bool,
    /// Remembered values filled for this execution.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub context: BTreeMap<String, String>,
    /// Preferred output format for rendering the payload.
    pub format_hint: Option<String>,
    /// Preferred field paths for row or curated record projections.
    /// Paths accept dotted keys or JSON Pointers for literal field segments.
    pub columns: Option<Vec<String>>,
    /// Preferred alignment hints for displayed columns.
    #[serde(default)]
    pub column_align: Vec<ColumnAlignmentV1>,
    /// Numeric field paths representing Unix seconds; formatted only for humans.
    #[serde(default)]
    pub unix_timestamp_columns: Vec<String>,
    /// Human-only field decorations; canonical JSON and DSL values stay unchanged.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub display_rules: Vec<crate::core::output_model::DisplayRule>,
    /// Optional human headings aligned with `columns`. JSON and DSL keys remain
    /// the canonical column paths.
    #[serde(default)]
    pub column_labels: Vec<String>,
    /// Top-level `data` field whose array supplies canonical rows for display and DSL.
    ///
    /// The full `data` document is retained only for unstaged JSON output.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub row_path: Option<String>,
    /// Preserve raw `data` for unstaged JSON rendering.
    ///
    /// The default row projection remains appropriate for list-oriented
    /// commands. Document-oriented commands opt in when an object or scalar is
    /// itself their stable machine contract.
    #[serde(default)]
    pub preserve_json_document: bool,
    /// Human-only notes after the result; absent from JSON and DSL data.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub display_notes: Vec<String>,
    /// Native-only human supplements, rendered on stderr without changing payload data.
    #[serde(skip)]
    pub detail_columns: Vec<String>,
    /// Replace the previous progress block when stderr is a terminal.
    #[serde(default, skip_serializing_if = "is_false")]
    pub progress_replace: bool,
    /// Human-only cap on rows shown when no DSL stage runs. JSON output and
    /// pipeline stages always see every row the producer returned.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub display_limit: Option<usize>,
    /// Set when the rows are one page of a larger result: how to fetch the
    /// rest. Pipeline stages then warn that counts and groups cover this page.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub partial_rows: Option<String>,
}

fn is_false(value: &bool) -> bool {
    !*value
}

/// Column alignment hint used in plugin response metadata.
///
/// Alignment follows the corresponding `ResponseMetaV1::columns` position. When
/// omitted, the host falls back to its renderer defaults for that column.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum ColumnAlignmentV1 {
    /// Use the renderer's default alignment.
    #[default]
    Default,
    /// Left-align the column.
    Left,
    /// Center-align the column.
    Center,
    /// Right-align the column.
    Right,
}

/// User-facing message emitted alongside a plugin response.
///
/// These messages are rendered on the host's diagnostic/message path, not
/// folded into `data`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ResponseMessageV1 {
    /// Severity level for the message.
    #[serde(with = "message_level_v1")]
    pub level: MessageLevel,
    /// Human-readable message text.
    pub text: String,
}

pub(crate) fn canonical_plugin_command_name(command: &str) -> Result<String, String> {
    let command = command.trim();
    if command.is_empty() {
        return Err("command name must not be empty".to_string());
    }
    if !command
        .chars()
        .all(|ch| ch.is_ascii_lowercase() || ch.is_ascii_digit() || matches!(ch, '-' | '_'))
    {
        return Err(format!(
            "command name `{command}` must use lowercase ASCII letters, digits, `-`, or `_`"
        ));
    }
    Ok(command.to_string())
}

mod message_level_v1 {
    use super::MessageLevel;
    use serde::{Deserialize, Deserializer, Serializer};

    pub fn serialize<S: Serializer>(
        level: &MessageLevel,
        serializer: S,
    ) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(if *level >= MessageLevel::Trace {
            "trace"
        } else {
            level.as_str()
        })
    }

    pub fn deserialize<'de, D: Deserializer<'de>>(
        deserializer: D,
    ) -> Result<MessageLevel, D::Error> {
        let value = String::deserialize(deserializer)?;
        MessageLevel::parse(&value)
            .filter(|level| *level <= MessageLevel::Trace)
            .ok_or_else(|| serde::de::Error::custom("unsupported plugin message level"))
    }
}

impl DescribeCommandV1 {
    /// Resolves described options and subcommands for authorization checks.
    pub(crate) fn resolve_invocation(&self, args: &[String]) -> DescribeInvocation {
        let mut segments = vec![self.name.clone()];
        let mut current = self;
        let mut inherited_flags = Vec::new();
        let mut positional_index = 0;
        let mut open_multi_positional = false;
        let mut index = 0;

        while let Some(token) = args.get(index) {
            if token == "--" {
                break;
            }

            if matches!(token.as_str(), "--help" | "-h") {
                break;
            }

            if let Some(subcommand) = current
                .subcommands
                .iter()
                .find(|subcommand| subcommand.name.eq_ignore_ascii_case(token))
            {
                // A token that can be either a positional value or a nested
                // command is not safe to resolve from describe metadata. Do
                // not guess the child path and accidentally apply its (or
                // the parent's) wrong policy.
                if open_multi_positional || current.args.get(positional_index).is_some() {
                    return DescribeInvocation {
                        path: CommandPath::new(segments),
                        ambiguous: true,
                    };
                }
                segments.push(subcommand.name.clone());
                inherited_flags.push(&current.flags);
                current = subcommand;
                positional_index = 0;
                open_multi_positional = false;
                index += 1;
                continue;
            }

            let flag = current
                .flags
                .iter()
                .chain(inherited_flags.iter().rev().flat_map(|flags| flags.iter()))
                .find_map(|(name, flag)| flag_token_matches(name, token).then_some(flag));
            let has_attached_value = token.contains('=')
                || (token.starts_with('-')
                    && !token.starts_with("--")
                    && token.chars().count() > 2);
            if let Some(flag) = flag {
                if !flag.flag_only && !has_attached_value {
                    // Missing option values are ambiguous; do not try to
                    // interpret a later token as a subcommand or help flag.
                    if args.get(index + 1).is_none() {
                        break;
                    }
                    index += 2;
                } else {
                    index += 1;
                }
                continue;
            }

            // The description cannot tell whether an unknown option is a
            // plugin-owned value, an option with a value, or a typo. Stop at
            // that boundary rather than guessing a nested policy path.
            if token.starts_with('-') {
                break;
            }

            if let Some(argument) = current.args.get(positional_index) {
                positional_index += 1;
                index += 1;
                if argument.multi {
                    open_multi_positional = true;
                    while let Some(value) = args.get(index) {
                        if value == "--"
                            || value.starts_with('-')
                            || current
                                .subcommands
                                .iter()
                                .any(|subcommand| subcommand.name.eq_ignore_ascii_case(value))
                        {
                            break;
                        }
                        index += 1;
                    }
                } else {
                    open_multi_positional = false;
                }
            } else {
                // An unrecognised positional may be a plugin-specific
                // grammar construct. It is not safe to keep scanning past it.
                break;
            }
        }

        DescribeInvocation {
            path: CommandPath::new(segments),
            ambiguous: match args.get(index) {
                None => false,
                Some(token) if token == "--" => args.get(index + 1).is_some(),
                Some(_) => true,
            } && !current.subcommands.is_empty(),
        }
    }

    /// Fills unannotated descendants with their nearest parent's auth policy.
    pub(crate) fn inherit_auth_from_parents(&mut self) {
        self.inherit_auth(None);
    }

    fn inherit_auth(&mut self, parent: Option<&DescribeCommandAuthV1>) {
        if self.auth.is_none() {
            self.auth = parent.cloned();
        }
        let inherited = self.auth.as_ref();
        for subcommand in &mut self.subcommands {
            subcommand.inherit_auth(inherited);
        }
    }
}

/// Result of the one conservative pass over a plugin invocation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct DescribeInvocation {
    /// Best-known canonical command path for policy evaluation.
    pub(crate) path: CommandPath,
    /// Whether an unknown token could hide a nested command path.
    pub(crate) ambiguous: bool,
}

fn flag_token_matches(flag_name: &str, token: &str) -> bool {
    token == flag_name
        || flag_name.starts_with("--")
            && token
                .strip_prefix(flag_name)
                .is_some_and(|suffix| suffix.starts_with('='))
        || flag_name.starts_with('-')
            && !flag_name.starts_with("--")
            && token.starts_with(flag_name)
}

impl ResponseV1 {
    /// Validates the response envelope before the app trusts its payload.
    ///
    /// Hosts should run this before adapting plugin JSON into rows, semantic
    /// output, or user-facing messages.
    ///
    /// # Examples
    ///
    /// ```
    /// use osp_cli::core::plugin::{ResponseMetaV1, ResponseV1};
    /// use serde_json::json;
    ///
    /// let response = ResponseV1 {
    ///     source: None,
    ///     protocol_version: 1,
    ///     ok: true,
    ///     data: json!({"uid": "alice"}),
    ///     error: None,
    ///     messages: Vec::new(),
    ///     meta: ResponseMetaV1::default(),
    ///     context: None,
    /// };
    ///
    /// assert!(response.validate_v1().is_ok());
    ///
    /// let invalid = ResponseV1 {
    ///     ok: false,
    ///     error: None,
    ///     ..response.clone()
    /// };
    ///
    /// assert_eq!(
    ///     invalid.validate_v1().unwrap_err(),
    ///     "ok=false requires error payload"
    /// );
    /// ```
    pub fn validate_v1(&self) -> Result<(), String> {
        if self.protocol_version != PLUGIN_PROTOCOL_V1 {
            return Err(format!(
                "unsupported response protocol version: {}",
                self.protocol_version
            ));
        }
        if self.ok && self.error.is_some() {
            return Err("ok=true requires error=null".to_string());
        }
        if !self.ok && self.error.is_none() {
            return Err("ok=false requires error payload".to_string());
        }
        if self
            .context
            .as_ref()
            .is_some_and(|entries| entries.keys().any(|kind| kind.trim().is_empty()))
        {
            return Err("response context kinds must not be empty".to_string());
        }
        if self
            .messages
            .iter()
            .any(|message| message.text.trim().is_empty())
        {
            return Err("response messages must not contain empty text".to_string());
        }
        if let Some(row_path) = self.meta.row_path.as_deref() {
            if row_path.trim().is_empty() {
                return Err("meta.row_path must not be empty".to_string());
            }
            let Some(rows) = self
                .data
                .as_object()
                .and_then(|document| document.get(row_path))
            else {
                return Err("meta.row_path must name a top-level data field".to_string());
            };
            if !rows.is_array() {
                return Err("meta.row_path must reference an array".to_string());
            }
        }
        if !self.meta.column_labels.is_empty() {
            let Some(columns) = self
                .meta
                .columns
                .as_ref()
                .filter(|columns| !columns.is_empty())
            else {
                return Err("meta.column_labels requires meta.columns".to_string());
            };
            if self.meta.column_labels.len() != columns.len() {
                return Err("meta.column_labels must align with meta.columns".to_string());
            }
            if self
                .meta
                .column_labels
                .iter()
                .any(|label| label.trim().is_empty())
            {
                return Err("meta.column_labels must not contain empty labels".to_string());
            }
        }
        Ok(())
    }
}

impl DescribeCommandV1 {
    /// Converts a `clap` command into a protocol v1 command description.
    ///
    /// Use this when the surrounding plugin metadata is assembled elsewhere but
    /// one command tree should still come from `clap`.
    ///
    /// # Examples
    ///
    /// ```
    /// use clap::Command;
    /// use osp_cli::core::plugin::DescribeCommandV1;
    ///
    /// let command = DescribeCommandV1::from_clap(
    ///     Command::new("ldap")
    ///         .about("Directory lookups")
    ///         .subcommand(Command::new("user").about("Lookup one user")),
    /// );
    ///
    /// assert_eq!(command.name, "ldap");
    /// assert_eq!(command.subcommands[0].name, "user");
    /// ```
    pub fn from_clap(command: clap::Command) -> Self {
        let mut context = command
            .get::<crate::core::context::ContextContract>()
            .cloned()
            .unwrap_or_default();
        for consume in &mut context.consumes {
            if let Some(argument) = command
                .get_positionals()
                .find(|arg| arg.get_id().as_str() == consume.argument)
            {
                consume.argument = clap_meta::arg_name(argument);
            }
            for conflict in &mut consume.conflicts {
                if let Some(argument) = command
                    .get_arguments()
                    .find(|arg| arg.get_id().as_str() == conflict)
                {
                    *conflict = argument
                        .get_long()
                        .map(str::to_owned)
                        .unwrap_or_else(|| clap_meta::arg_name(argument));
                }
            }
        }
        Self {
            context,
            name: command.get_name().to_string(),
            about: clap_meta::plain(command.get_about()).unwrap_or_default(),
            auth: None,
            args: command
                .get_positionals()
                .filter(|arg| !arg.is_hide_set())
                .map(|arg| DescribeArgV1 {
                    name: Some(clap_meta::arg_name(arg)),
                    about: clap_meta::arg_help(arg),
                    required: arg.is_required_set(),
                    multi: clap_meta::is_multi(arg),
                    value_type: clap_meta::is_path(arg).then_some(DescribeValueTypeV1::Path),
                    suggestions: clap_suggestions(arg),
                })
                .collect(),
            flags: command
                .get_arguments()
                .filter(|arg| !arg.is_positional() && !arg.is_hide_set())
                .flat_map(|arg| {
                    let spellings = clap_meta::flag_spellings(arg);
                    let preferred = spellings.first().cloned();
                    spellings.into_iter().enumerate().map(move |(index, name)| {
                        (
                            name,
                            DescribeFlagV1 {
                                alias_of: (index > 0).then(|| preferred.clone()).flatten(),
                                about: clap_meta::arg_help(arg),
                                required: arg.is_required_set(),
                                flag_only: !arg.get_action().takes_values(),
                                multi: clap_meta::is_multi(arg),
                                value_type: clap_meta::is_path(arg)
                                    .then_some(DescribeValueTypeV1::Path),
                                suggestions: clap_suggestions(arg),
                            },
                        )
                    })
                })
                .collect(),
            subcommands: command
                .get_subcommands()
                .filter(|child| !child.is_hide_set())
                .map(|child| Self::from_clap(child.clone()))
                .collect(),
        }
    }
}

fn clap_suggestions(arg: &clap::Arg) -> Vec<DescribeSuggestionV1> {
    arg.get_possible_values()
        .into_iter()
        .filter(|value| !value.is_hide_set())
        .map(|value| DescribeSuggestionV1 {
            value: value.get_name().to_string(),
            meta: value.get_help().map(ToString::to_string),
            display: None,
            sort: None,
        })
        .collect()
}

pub(crate) fn validate_command(command: &DescribeCommandV1) -> Result<(), String> {
    for consume in &command.context.consumes {
        if !crate::core::context::KINDS.contains(&consume.kind.as_str()) {
            return Err(format!("unknown context kind: {}", consume.kind));
        }
        if matches!(
            consume.argument.trim_start_matches('-'),
            "password" | "token" | "otp" | "auth" | "user" | "profile" | "url"
        ) {
            return Err("context cannot supply credentials, identity, profile or URLs".into());
        }
    }
    for kind in command.context.produces.keys() {
        if !crate::core::context::KINDS.contains(&kind.as_str()) {
            return Err(format!("unknown context kind: {kind}"));
        }
    }
    if canonical_plugin_command_name(&command.name)? != command.name {
        return Err("command name must not have leading or trailing whitespace".to_string());
    }
    if let Some(auth) = &command.auth {
        validate_command_auth(auth)?;
    }

    let mut ids = std::collections::BTreeSet::from(["help".to_string()]);
    let mut optional = false;
    let mut multi = false;
    for (index, arg) in command.args.iter().enumerate() {
        let id = arg.name.clone().unwrap_or_else(|| format!("arg{index}"));
        if id.is_empty() || !ids.insert(id.clone()) {
            return Err(format!("duplicate or empty argument id `{id}`"));
        }
        if optional && arg.required {
            return Err("required positional follows an optional positional".into());
        }
        optional |= !arg.required;
        if arg.multi {
            if multi
                || index + 2 < command.args.len()
                || (index + 1 < command.args.len() && !command.args[index + 1].required)
            {
                return Err(
                    "variadic positional must be last or precede one required positional".into(),
                );
            }
            multi = true;
        }
        validate_suggestions(&arg.suggestions, "argument")?;
    }
    for (name, flag) in &command.flags {
        let spelling = name.trim_start_matches('-');
        let valid = if let Some(long) = name.strip_prefix("--") {
            !long.is_empty() && !long.starts_with('-') && !long.contains(['=', ' '])
        } else {
            name.starts_with('-') && spelling.chars().count() == 1
        };
        if !valid || matches!(name.as_str(), "--help" | "-h") {
            return Err(format!("invalid flag spelling `{name}`"));
        }
        if let Some(target) = &flag.alias_of {
            if target == name
                || command
                    .flags
                    .get(target)
                    .is_none_or(|f| f.alias_of.is_some())
            {
                return Err(format!("invalid flag alias `{name}`"));
            }
        } else if !ids.insert(spelling.to_string()) {
            return Err(format!("duplicate argument id `{spelling}`"));
        }
        validate_suggestions(&flag.suggestions, &format!("flag `{name}`"))?;
    }
    let mut names = std::collections::BTreeSet::from(["help"]);
    for subcommand in &command.subcommands {
        if !names.insert(subcommand.name.as_str()) {
            return Err(format!("duplicate subcommand `{}`", subcommand.name));
        }
        validate_command(subcommand)?;
    }

    Ok(())
}

fn validate_suggestions(suggestions: &[DescribeSuggestionV1], owner: &str) -> Result<(), String> {
    if suggestions
        .iter()
        .any(|entry| entry.value.trim().is_empty())
    {
        return Err(format!("{owner} suggestions must not contain empty values"));
    }
    Ok(())
}

fn validate_command_auth(auth: &DescribeCommandAuthV1) -> Result<(), String> {
    if let Some(requirements) = auth.run_session.as_ref() {
        validate_session_requirements("run_session", requirements)?;
    }
    Ok(())
}

fn validate_session_requirements(
    owner: &str,
    requirements: &DescribeSessionRequirementsV1,
) -> Result<(), String> {
    if requirements.credentials.iter().any(|requirement| {
        !matches!(requirement, DescribeCredentialRequirementV1::Fresh { .. })
            && requirement.service().trim().is_empty()
    }) {
        return Err(format!(
            "{owner} credentials must not contain empty services"
        ));
    }
    Ok(())
}

#[cfg(test)]
#[cfg_attr(coverage_nightly, coverage(off))]
mod tests {
    use std::collections::BTreeMap;

    use super::{
        DescribeAuthStrengthV1, DescribeCommandAuthV1, DescribeCredentialRequirementV1,
        DescribeSessionRequirementsV1, DescribeVisibilityModeV1, PLUGIN_PROTOCOL_V1,
        ResponseMetaV1, ResponseV1, canonical_plugin_command_name,
    };
    use serde_json::json;
    #[test]
    fn protocol_validation_names_the_invalid_field_unit() {
        let response = json!({
            "protocol_version": PLUGIN_PROTOCOL_V1,
            "ok": true,
            "data": {"items": []},
            "error": null,
            "messages": [{"level": "info", "text": "listed"}],
            "meta": {"row_path": "items", "columns": ["name"], "column_labels": ["NAME"]}
        });
        let validate_response = |value: serde_json::Value| {
            serde_json::from_value::<ResponseV1>(value)
                .expect("response fixture should deserialize")
                .validate_v1()
        };
        assert_eq!(validate_response(response.clone()), Ok(()));
        for (pointer, value, field) in [
            ("/protocol_version", json!(0), "protocol version"),
            ("/error", json!({"code": "x", "message": "y"}), "error=null"),
            ("/ok", json!(false), "error payload"),
            ("/messages/0/text", json!(" "), "messages"),
            ("/meta/row_path", json!(" "), "meta.row_path"),
            ("/meta/columns", json!(null), "requires meta.columns"),
            ("/meta/column_labels/0", json!(" "), "empty labels"),
        ] {
            let mut invalid = response.clone();
            *invalid.pointer_mut(pointer).expect("fixture path") = value;
            let error = validate_response(invalid).expect_err(pointer);
            assert!(error.contains(field), "{pointer}: {error}");
        }
    }

    #[test]
    fn response_row_path_requires_a_top_level_array_unit() {
        let response = ResponseV1 {
            source: None,
            protocol_version: PLUGIN_PROTOCOL_V1,
            ok: true,
            data: json!({
                "items": [{"name": "db01.uio.no"}],
                "page": {"next_cursor": null}
            }),
            error: None,
            messages: Vec::new(),
            context: None,
            meta: ResponseMetaV1 {
                context: Default::default(),
                row_path: Some("items".to_string()),
                ..ResponseMetaV1::default()
            },
        };

        assert!(response.validate_v1().is_ok());
        assert_eq!(
            serde_json::to_value(&response).expect("response should serialize")["meta"]["row_path"],
            "items"
        );

        let missing = ResponseV1 {
            data: json!({"page": {"next_cursor": null}}),
            ..response.clone()
        };
        assert_eq!(
            missing.validate_v1().unwrap_err(),
            "meta.row_path must name a top-level data field"
        );

        let non_array = ResponseV1 {
            data: json!({"items": {"name": "db01.uio.no"}}),
            ..response
        };
        assert_eq!(
            non_array.validate_v1().unwrap_err(),
            "meta.row_path must reference an array"
        );
    }

    #[test]
    fn response_context_is_optional_and_round_trips_unit() {
        let mut legacy = serde_json::from_value::<ResponseV1>(json!({
            "protocol_version": 1,
            "ok": true,
            "data": [],
            "error": null,
            "meta": {}
        }))
        .expect("legacy response should deserialize");
        assert!(legacy.context.is_none());

        legacy.context = Some(BTreeMap::from([(
            "host".to_string(),
            "foo.uio.no".to_string(),
        )]));
        let wire = serde_json::to_value(legacy).expect("response should serialize");
        assert_eq!(wire["context"]["host"], "foo.uio.no");
    }

    #[test]
    fn response_column_labels_align_with_columns_unit() {
        let response = ResponseV1 {
            source: None,
            protocol_version: PLUGIN_PROTOCOL_V1,
            ok: true,
            data: json!({"items": [{"provider": {"name": "vmware"}}]}),
            error: None,
            messages: Vec::new(),
            context: None,
            meta: ResponseMetaV1 {
                context: Default::default(),
                columns: Some(vec!["provider.name".to_string()]),
                column_labels: vec!["PROVIDER".to_string()],
                row_path: Some("items".to_string()),
                ..ResponseMetaV1::default()
            },
        };
        assert!(response.validate_v1().is_ok());

        let mismatched = ResponseV1 {
            context: None,
            meta: ResponseMetaV1 {
                context: Default::default(),
                column_labels: vec!["PROVIDER".to_string(), "STATE".to_string()],
                ..response.meta.clone()
            },
            ..response
        };
        assert_eq!(
            mismatched.validate_v1().unwrap_err(),
            "meta.column_labels must align with meta.columns"
        );
    }

    #[test]
    fn command_auth_hint_stays_compact_and_stable_unit() {
        let auth = DescribeCommandAuthV1 {
            visibility: Some(DescribeVisibilityModeV1::CapabilityGated),
            required_capabilities: vec!["orch.approval.decide".to_string()],
            feature_flags: vec!["orch".to_string()],
            run_session: Some(DescribeSessionRequirementsV1 {
                auth_strength: Some(DescribeAuthStrengthV1::Strong),
                credentials: vec![DescribeCredentialRequirementV1::fresh("osp", 600)],
            }),
            ..DescribeCommandAuthV1::default()
        };
        assert_eq!(auth.hint(), None);
        assert_eq!(
            DescribeVisibilityModeV1::Authenticated.as_label(),
            "authenticated"
        );
    }

    #[test]
    fn canonical_plugin_command_name_rejects_mixed_case_unit() {
        assert_eq!(
            canonical_plugin_command_name("ldap-user").as_deref(),
            Ok("ldap-user")
        );
        assert!(
            canonical_plugin_command_name("Ldap")
                .expect_err("mixed-case command names should be rejected")
                .contains("must use lowercase ASCII letters")
        );
        assert!(
            canonical_plugin_command_name("ldap user")
                .expect_err("whitespace should be rejected")
                .contains("must use lowercase ASCII letters")
        );
    }
}

#[cfg(test)]
#[cfg_attr(coverage_nightly, coverage(off))]
mod clap_tests {
    use super::{DescribeCommandV1, DescribeValueTypeV1};
    use clap::{Arg, ArgAction, Command, ValueHint};

    #[test]
    fn clap_helper_captures_subcommands_flags_and_args() {
        let command = Command::new("ldap").about("LDAP plugin").subcommand(
            Command::new("user")
                .about("Lookup LDAP users")
                .arg(Arg::new("uid").help("User id"))
                .arg(
                    Arg::new("attributes")
                        .long("attributes")
                        .short('a')
                        .help("Attributes to fetch")
                        .action(ArgAction::Set)
                        .value_parser(["uid", "cn", "mail"]),
                )
                .arg(
                    Arg::new("input")
                        .long("input")
                        .help("Read from file")
                        .value_hint(ValueHint::FilePath),
                ),
        );

        let ldap = DescribeCommandV1::from_clap(command);
        assert_eq!(ldap.name, "ldap");
        assert_eq!(ldap.subcommands.len(), 1);

        let user = &ldap.subcommands[0];
        assert_eq!(user.name, "user");
        assert_eq!(user.args[0].name.as_deref(), Some("uid"));
        assert!(user.flags.contains_key("--attributes"));
        assert!(user.flags.contains_key("-a"));
        assert_eq!(
            user.flags["--attributes"]
                .suggestions
                .iter()
                .map(|entry| entry.value.as_str())
                .collect::<Vec<_>>(),
            vec!["uid", "cn", "mail"]
        );
        assert_eq!(
            user.flags["--input"].value_type,
            Some(DescribeValueTypeV1::Path)
        );
    }

    #[test]
    fn clap_command_conversion_skips_hidden_items() {
        let command = Command::new("ldap")
            .subcommand(Command::new("visible"))
            .subcommand(Command::new("hidden").hide(true))
            .arg(Arg::new("secret").long("secret").hide(true));

        let describe = DescribeCommandV1::from_clap(command);

        assert_eq!(
            describe
                .subcommands
                .iter()
                .map(|subcommand| subcommand.name.as_str())
                .collect::<Vec<_>>(),
            vec!["visible"]
        );
        assert!(!describe.flags.contains_key("--secret"));
    }
}
