//! Configuration exists so the app can answer three questions consistently:
//! what keys are legal, which source wins, and what file edits are allowed.
//!
//! The mental model is:
//!
//! - [`crate::config::LoaderPipeline`] materializes source layers from files,
//!   environment, and in-memory overrides.
//! - [`crate::config::ConfigResolver`] applies source precedence, scope
//!   precedence, interpolation, and schema adaptation.
//! - [`crate::config::set_scoped_value_in_toml`] edits TOML-backed config files
//!   while preserving the same schema and scope rules used at runtime.
//!
//! Quick starts:
//!
//! In-memory or test-only config resolution:
//!
//! ```
//! use osp_cli::config::{ConfigLayer, LoaderPipeline, ResolveOptions, StaticLayerLoader};
//!
//! let mut defaults = ConfigLayer::default();
//! defaults.set("profile.default", "default");
//! defaults.set("theme.path", Vec::<String>::new());
//! defaults.set("theme.name", "dracula");
//!
//! let resolved = LoaderPipeline::new(StaticLayerLoader::new(defaults))
//!     .resolve(ResolveOptions::new().with_terminal("cli"))
//!     .unwrap();
//!
//! assert_eq!(resolved.terminal(), Some("cli"));
//! assert_eq!(resolved.get_string("theme.name"), Some("dracula"));
//! ```
//!
//! Host-style bootstrap uses [`crate::config::RuntimeDefaults`], [`crate::config::RuntimeConfigPaths`] and
//! [`crate::config::build_runtime_pipeline`] to produce a [`crate::config::ResolvedConfig`]. Inspect its
//! active profile and typed values directly; derived host state stays internal.
//!
//! On-disk config mutation:
//!
//! - use [`crate::config::set_scoped_value_in_toml`] and
//!   [`crate::config::unset_scoped_value_in_toml`] instead of editing TOML by
//!   hand
//! - use `config explain` when the main question
//!   is "why did this value win?"
//!
//! Broad-strokes flow:
//!
//! ```text
//! files / env / session overrides
//!        │
//!        ▼ [ LoaderPipeline ]
//!    ConfigLayer values + scope metadata
//!        │
//!        ▼ [ ConfigResolver ]
//!    precedence + interpolation + type adaptation
//!        │
//!        ├── ResolvedConfig  (full provenance-aware map)
//!        └── config explain  (why this winner won)
//! ```
//!
//! Read this module when you need to answer "where did this config value come
//! from?", "why did this value win?", or "what writes are legal for this key?".
//!
//! Most callers should not be assembling bespoke config logic. The normal path
//! is:
//!
//! - load layers through [`crate::config::LoaderPipeline`]
//! - resolve once through [`crate::config::ConfigResolver`]
//! - consume the result as [`crate::config::ResolvedConfig`]
//! - for host-style startup, prefer [`crate::config::RuntimeDefaults`],
//!   [`crate::config::RuntimeConfigPaths`], and
//!   [`crate::config::build_runtime_pipeline`] over hand-rolled file/env
//!   discovery
//!
//! The same discipline should hold for writes. File edits belong through the
//! store helpers here so `config set`, runtime resolution, and `config explain`
//! stay aligned. A hand-rolled file edit path almost always creates drift.
//!
//! Contract:
//!
//! - source precedence and scope precedence are defined here, not in callers
//! - schema validation and config-store editing should stay aligned
//! - other modules should not hand-roll config merging or scope filtering

mod bootstrap;
mod builtin;
mod core;
mod defaults;
mod error;
mod explain;
mod interpolate;
mod keys;
mod loader;
mod resolver;
mod runtime;
mod secrets;
mod selector;
mod store;

pub(crate) use core::{
    ActiveProfileSource, BootstrapConfigExplain, BootstrapPhase, BootstrapScopeRule,
    BootstrapValueRule, ConfigExplain, ConfigSchema, ExplainCandidate, ExplainInterpolation,
    ExplainInterpolationStep, ExplainLayer, SchemaEntry, bootstrap_key_spec, is_alias_key,
    is_bootstrap_only_key, validate_bootstrap_value, validate_key_scope,
};
pub use core::{
    ConfigLayer, ConfigSource, ConfigValue, LayerEntry, ResolveOptions, ResolvedConfig,
    ResolvedValue, SchemaValueType, Scope, SecretValue, TomlEditResult, is_sensitive_key,
};
pub(crate) use defaults::{
    DEFAULT_REPL_HISTORY_MAX_ENTRIES, DEFAULT_REPL_HISTORY_MENU_ROWS, DEFAULT_UI_WIDTH_MAX,
};
pub use error::{ConfigError, TomlParseDiagnostic};
pub(crate) use keys::ConfigKeyMap;
pub use loader::{
    ChainedLoader, ConfigLoader, EnvSecretsLoader, EnvVarLoader, LoadedLayers, LoaderPipeline,
    SecretsTomlLoader, StaticLayerLoader, TomlFileLoader,
};
pub use resolver::ConfigResolver;
#[cfg(test)]
pub(crate) use runtime::RuntimeConfig;
pub use runtime::{
    RuntimeBootstrapMode, RuntimeConfigPaths, RuntimeDefaults, RuntimeLoadOptions,
    build_runtime_pipeline, default_cache_root_dir, default_config_root_dir, default_home_dir,
    default_state_root_dir,
};
pub(crate) use secrets::{RuntimeSecretStore, SecretBackendKind, SecretStoreEditResult};
pub(crate) use store::secret_file_mode;
pub use store::{
    TomlSecretPermissions, TomlStoreEditMode, TomlStoreEditOptions, set_scoped_value_in_toml,
    unset_scoped_value_in_toml,
};

pub(crate) use core::{normalize_identifier, normalize_scope};
use defaults::build_builtin_defaults;
pub(crate) use error::with_path_context;
pub(crate) use secrets::RuntimeSelectedSecretsLoader;
pub(crate) use store::{AtomicWriteDurability, lock_file_transaction, write_text_atomic};

#[cfg(test)]
use defaults::DEFAULT_PROFILE_NAME;

#[cfg(test)]
#[path = "tests/loading_and_mutation.rs"]
mod loading_and_mutation;
