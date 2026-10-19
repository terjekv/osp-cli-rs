//! Translation between product input/display keys and canonical storage keys.

use crate::config::{ConfigError, ConfigLayer, normalize_identifier};

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(crate) struct ConfigKeyMap {
    namespace: Option<&'static str>,
}

impl ConfigKeyMap {
    pub(crate) const fn empty() -> Self {
        Self { namespace: None }
    }

    pub(crate) fn new(namespace: &'static str) -> Self {
        assert!(
            !namespace.is_empty()
                && namespace
                    .bytes()
                    .all(|c| c.is_ascii_alphanumeric() || c == b'_' || c == b'-')
        );
        Self {
            namespace: Some(namespace),
        }
    }

    pub(crate) fn canonical(self, key: &str) -> String {
        let key = normalize_identifier(key);
        let schema = super::core::builtin_config_schema();
        match self.namespace {
            Some(namespace)
                if !key.starts_with("extensions.")
                    && !schema.is_known_key(&key)
                    && !schema.is_removed_key(&key) =>
            {
                format!("extensions.{namespace}.{key}")
            }
            _ => key,
        }
    }

    pub(crate) fn display(self, key: &str) -> String {
        self.namespace
            .and_then(|namespace| key.strip_prefix(&format!("extensions.{namespace}.")))
            .unwrap_or(key)
            .to_owned()
    }

    pub(crate) fn normalize_layer(self, layer: &mut ConfigLayer) {
        for entry in &mut layer.entries {
            entry.key = self.canonical(&entry.key);
        }
    }

    pub(crate) fn error(self, mut error: ConfigError) -> ConfigError {
        use ConfigError::*;
        match &mut error {
            LayerLoad { inner, .. } => **inner = self.error((**inner).clone()),
            InvalidEnvOverride { key, .. }
            | InvalidConfigKey { key, .. }
            | ReadOnlyConfigKey { key, .. }
            | InvalidBootstrapScope { key, .. }
            | InvalidBootstrapValue { key, .. }
            | InvalidPlaceholderSyntax { key, .. }
            | MissingRequiredKey { key }
            | InvalidValueType { key, .. }
            | InvalidConfigValue { key, .. }
            | InvalidEnumValue { key, .. } => *key = self.display(key),
            UnresolvedPlaceholder { key, placeholder }
            | NonScalarPlaceholder { key, placeholder } => {
                *key = self.display(key);
                *placeholder = self.display(placeholder);
            }
            UnknownConfigKeys { keys } | PlaceholderCycle { cycle: keys } => {
                for key in keys {
                    *key = self.display(key);
                }
            }
            UnsupportedTomlValue { path, .. } => *path = self.display(path),
            _ => {}
        }
        error
    }
}
