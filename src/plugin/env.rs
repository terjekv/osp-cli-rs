//! Config values projected into the executable's inherited environment.

use crate::config::{ConfigValue, ResolvedConfig};
use std::collections::HashMap;

pub(crate) struct ConfigEnv {
    pub(crate) shared: Vec<(String, String)>,
    pub(crate) commands: HashMap<String, Vec<(String, String)>>,
    pub(crate) secrets: Vec<String>,
}

pub(crate) fn config_env(config: &ResolvedConfig) -> ConfigEnv {
    let mut shared = Vec::new();
    let mut secrets = std::env::vars_os()
        .filter(|(key, _)| crate::config::is_sensitive_key(&key.to_string_lossy()))
        .filter_map(|(_, value)| value.into_string().ok())
        .collect::<Vec<_>>();
    let mut commands: HashMap<String, Vec<(String, String)>> = HashMap::new();
    for (key, entry) in config.values() {
        let Some(rest) = key.strip_prefix("extensions.plugins.") else {
            continue;
        };
        let (target, name) = if let Some(name) = rest.strip_prefix("env.") {
            (&mut shared, name)
        } else if let Some((command, name)) = rest.split_once(".env.") {
            (commands.entry(command.to_string()).or_default(), name)
        } else {
            continue;
        };
        let normalized = name
            .split(|c: char| !c.is_ascii_alphanumeric())
            .filter(|s| !s.is_empty())
            .collect::<Vec<_>>()
            .join("_")
            .to_ascii_uppercase();
        if !normalized.is_empty() {
            let value = env_value(&entry.value);
            if entry.value.is_secret() || crate::config::is_sensitive_key(&normalized) {
                secrets.push(value.clone());
            }
            target.push((format!("OSP_PLUGIN_CFG_{normalized}"), value));
        }
    }
    secrets.retain(|value| !value.is_empty());
    secrets.sort_by_key(|value| std::cmp::Reverse(value.len()));
    secrets.dedup();
    ConfigEnv {
        shared,
        commands,
        secrets,
    }
}

fn env_value(value: &ConfigValue) -> String {
    match value.reveal() {
        ConfigValue::String(s) => s.clone(),
        ConfigValue::List(values) => {
            serde_json::Value::Array(values.iter().map(json_value).collect()).to_string()
        }
        other => json_value(other).to_string(),
    }
}
fn json_value(value: &ConfigValue) -> serde_json::Value {
    match value.reveal() {
        ConfigValue::String(s) => s.clone().into(),
        ConfigValue::Bool(b) => (*b).into(),
        ConfigValue::Integer(n) => (*n).into(),
        ConfigValue::Float(n) => serde_json::json!(n),
        ConfigValue::List(values) => values.iter().map(json_value).collect(),
        ConfigValue::Secret(_) => unreachable!("reveal unwraps secrets"),
    }
}
