//! Entity declarations, argument resolution and scoped REPL persistence.

use crate::core::plugin::{ResponseMessageV1, ResponseV1};
use crate::native::{NativeSessionContext, NativeSessionContextEntry};
use anyhow::{Result, bail};
use clap::Command;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

/// Shared vocabulary for selections across native commands and plugins.
pub const KINDS: &[&str] = &[
    "account", "group", "person", "host", "netgroup", "contact", "task", "provider", "vm",
];
pub(crate) fn shared(kind: &str) -> bool {
    matches!(kind, "account" | "group" | "host")
}

pub(crate) fn valid_value(value: &str) -> bool {
    !value.trim().is_empty() && value.len() <= 256 && !value.chars().any(char::is_control)
}

/// How an omitted entity argument may use context.
#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum Fill {
    /// Only an explicit value or `.` is accepted.
    #[default]
    Explicit,
    /// An omitted read subject may be filled.
    Read,
    /// A remembered scope may narrow an explicitly named object's operation.
    Narrow,
}

/// One declared entity argument; argument names are clap IDs or flag spellings.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Consumes {
    /// Shared entity kind.
    pub kind: String,
    /// Positional ID or long flag; empty for plugin environment-only consumption.
    #[serde(default)]
    pub argument: String,
    /// Omission policy.
    #[serde(default)]
    pub fill: Fill,
    /// Other arguments that make this selection inapplicable.
    #[serde(default)]
    pub conflicts: Vec<String>,
    /// Require a value even when the grammar leaves the argument optional.
    #[serde(default)]
    pub required: bool,
}

/// Entity metadata attached with clap `add`, including from derive attributes.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ContextContract {
    /// Arguments which consume remembered entities.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub consumes: Vec<Consumes>,
    /// Kind to canonical result pointers (or `$ARG` for an acknowledged name).
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub produces: BTreeMap<String, Vec<String>>,
}
impl clap::builder::CommandExt for ContextContract {}
impl ContextContract {
    /// Declares an entity argument.
    pub fn consumes(mut self, kind: &str, argument: &str, fill: Fill) -> Self {
        self.consumes.push(Consumes {
            kind: kind.into(),
            argument: argument.into(),
            fill,
            conflicts: Vec::new(),
            required: false,
        });
        self
    }
    /// Declares a canonical entity in a singleton successful result.
    pub fn produces(mut self, kind: &str, pointer: &str) -> Self {
        self.produces
            .entry(kind.into())
            .or_default()
            .push(pointer.into());
        self
    }
}

#[derive(Default)]
pub(crate) struct ContextBinding {
    pub(crate) profile: String,
    pub(crate) identity: String,
    pub(crate) origins: BTreeMap<String, String>,
    persisted: BTreeMap<String, Vec<String>>,
    pub(crate) owners: BTreeMap<String, String>,
    loaded: bool,
    saved: Vec<NativeSessionContextEntry>,
}
impl NativeSessionContext {
    pub(crate) fn activate(&self, runtime: &crate::app::AppRuntime) {
        let config = runtime.config_state().resolved();
        let identity = runtime
            .access_policy()
            .map(|product| product.effective_identity(config))
            .unwrap_or_default();
        let mut binding = self.binding.write().unwrap_or_else(|e| e.into_inner());
        binding.identity = identity;
        binding.profile = config.active_profile().to_owned();
    }
    pub(crate) fn register_context(
        &self,
        service: &str,
        origin: String,
        persisted: &[&str],
        command: &Command,
        kinds: &[&str],
    ) {
        let mut binding = self.binding.write().unwrap_or_else(|e| e.into_inner());
        fn owners(command: &Command, service: &str, result: &mut BTreeMap<String, String>) {
            if let Some(contract) = command.get::<ContextContract>() {
                for kind in contract.produces.keys().filter(|kind| !shared(kind)) {
                    result.entry(kind.clone()).or_insert_with(|| service.into());
                }
                for consume in &contract.consumes {
                    if !shared(&consume.kind) {
                        result.insert(consume.kind.clone(), service.into());
                    }
                }
            }
            for child in command.get_subcommands() {
                owners(child, service, result);
            }
        }
        owners(command, service, &mut binding.owners);
        for kind in kinds {
            binding.owners.insert((*kind).into(), service.into());
        }
        binding.origins.insert(service.into(), origin);
        binding.persisted.insert(
            service.into(),
            persisted.iter().map(|s| (*s).into()).collect(),
        );
    }
    pub(crate) fn load_context(&self) -> Result<()> {
        let mut binding = self.binding.write().unwrap_or_else(|e| e.into_inner());
        if binding.loaded {
            return Ok(());
        }
        if let Some(path) = context_path().filter(|path| path.exists()) {
            let stored: Vec<NativeSessionContextEntry> =
                serde_json::from_slice(&std::fs::read(path)?)?;
            let mut entries = self
                .context_entries
                .write()
                .unwrap_or_else(|e| e.into_inner());
            let stored: Vec<_> = stored
                .into_iter()
                .filter(|e| {
                    binding
                        .persisted
                        .get(&e.service)
                        .is_some_and(|k| k.contains(&e.kind))
                        && valid_value(&e.value)
                })
                .collect();
            binding.saved = stored.clone();
            for entry in stored {
                entries.insert(entry.key(), entry);
            }
        }
        binding.loaded = true;
        Ok(())
    }
    /// Saves opt-in persisted kinds after a selection or migration.
    pub fn save_context(&self) -> Result<()> {
        let mut binding = self.binding.write().unwrap_or_else(|e| e.into_inner());
        if !binding.loaded {
            return Ok(());
        }
        let entries = self
            .context_entries
            .read()
            .unwrap_or_else(|e| e.into_inner());
        let stored: Vec<_> = entries
            .values()
            .filter(|e| {
                binding
                    .persisted
                    .get(&e.service)
                    .is_some_and(|k| k.contains(&e.kind))
            })
            .cloned()
            .collect();
        if let Some(path) = context_path() {
            let _transaction = crate::config::lock_file_transaction(&path)?;
            let current: Vec<NativeSessionContextEntry> = match std::fs::read(&path) {
                Ok(bytes) => serde_json::from_slice(&bytes)?,
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => Vec::new(),
                Err(error) => return Err(error.into()),
            };
            let mut merged: BTreeMap<_, _> = current.into_iter().map(|e| (e.key(), e)).collect();
            for entry in &binding.saved {
                if !stored.iter().any(|e| e.key() == entry.key()) {
                    merged.remove(&entry.key());
                }
            }
            for entry in &stored {
                if !binding.saved.contains(entry) {
                    merged.insert(entry.key(), entry.clone());
                }
            }
            crate::config::write_text_atomic(
                &path,
                &serde_json::to_vec(&merged.into_values().collect::<Vec<_>>())?,
                true,
            )?;
            binding.saved = stored;
        }
        Ok(())
    }
}
fn context_path() -> Option<std::path::PathBuf> {
    crate::config::default_state_root_dir().map(|p| p.join("context.json"))
}

/// Resolved command arguments and the context facts used by this execution.
pub struct Resolution {
    /// Arguments after explicit expansion and permitted read filling.
    pub args: Vec<String>,
    pub(crate) contract: ContextContract,
    matches: clap::ArgMatches,
    used: BTreeMap<String, String>,
    pub(crate) expanded: bool,
}
fn relaxed(command: Command) -> Command {
    command
        .mut_args(|arg| {
            let takes_value = arg.get_action().takes_values();
            let arg = arg.required(false);
            if takes_value {
                arg.value_parser(clap::builder::StringValueParser::new())
            } else {
                arg
            }
        })
        .mut_subcommands(relaxed)
}

fn parsed_leaf(command: &Command, args: &[String]) -> Result<clap::ArgMatches, clap::Error> {
    let mut matches = relaxed(command.clone()).try_get_matches_from(
        std::iter::once(command.get_name().to_string()).chain(args.iter().cloned()),
    )?;
    while let Some((_, child)) = matches.remove_subcommand() {
        matches = child;
    }
    Ok(matches)
}

/// Resolves entity arguments from a command declaration in a REPL session.
pub fn resolve(
    command: Command,
    args: &[String],
    session: &NativeSessionContext,
) -> Result<Resolution> {
    let mut result = Resolution {
        args: args.to_vec(),
        contract: ContextContract::default(),
        matches: clap::ArgMatches::default(),
        used: BTreeMap::new(),
        expanded: false,
    };
    if args
        .iter()
        .any(|arg| matches!(arg.as_str(), "--help" | "-h"))
    {
        return Ok(result);
    }
    let Ok(matches) = relaxed(command.clone()).try_get_matches_from(
        std::iter::once(command.get_name().to_string()).chain(args.iter().cloned()),
    ) else {
        return Ok(result);
    };
    let mut leaf = &command;
    let mut selected = &matches;
    loop {
        if let Some(contract) = leaf.get::<ContextContract>() {
            result.contract.consumes.extend(contract.consumes.clone());
            result.contract.produces.extend(contract.produces.clone());
        }
        let Some((name, child)) = selected.subcommand() else {
            break;
        };
        let Some(next) = leaf.find_subcommand(name) else {
            break;
        };
        leaf = next;
        selected = child;
    }
    for consume in &result.contract.consumes {
        if !KINDS.contains(&consume.kind.as_str()) {
            bail!("unknown context kind: {}", consume.kind);
        }
        if consume.argument.is_empty() {
            if let Some(value) = session.context_value(&consume.kind) {
                result.used.insert(consume.kind.clone(), value);
            }
            continue;
        }
        let id = consume.argument.trim_start_matches('-');
        if consume.conflicts.iter().any(|id| {
            selected.try_contains_id(id).is_ok()
                && selected.value_source(id) == Some(clap::parser::ValueSource::CommandLine)
        }) {
            continue;
        }
        let explicit = selected
            .try_get_many::<String>(id)
            .ok()
            .flatten()
            .map(|v| v.cloned().collect::<Vec<_>>())
            .unwrap_or_default();
        let current = session.context_value(&consume.kind);
        if explicit.iter().any(|v| v == ".") {
            let value = current
                .ok_or_else(|| anyhow::anyhow!("No current {}; specify a value", consume.kind))?;
            let mut marker = "__osp_context_value__".to_string();
            while result.args.iter().any(|arg| arg.contains(&marker)) {
                marker.push('_');
            }
            for index in 0..result.args.len() {
                let token = &result.args[index];
                if token != "." && !(token.starts_with('-') && token.ends_with('.')) {
                    continue;
                }
                let prefix = &token[..token.len() - 1];
                let mut probe = result.args.clone();
                probe[index] = format!("{prefix}{marker}");
                // Ask clap which argument owns this token; its indices also count
                // attached option values and individual short flags.
                if parsed_leaf(&command, &probe).ok().is_some_and(|matches| {
                    matches
                        .try_get_many::<String>(id)
                        .ok()
                        .flatten()
                        .is_some_and(|mut values| values.any(|v| v == &marker))
                }) {
                    result.args[index] = format!("{prefix}{value}");
                    result.expanded = true;
                }
            }
        } else if explicit.is_empty() {
            if consume.fill != Fill::Explicit
                && let Some(value) = current
            {
                if consume.argument.starts_with("--") {
                    result.args.push(consume.argument.clone());
                }
                result.args.push(value.clone());
                result.used.insert(consume.kind.clone(), value);
            } else if consume.required
                || leaf
                    .get_arguments()
                    .any(|a| a.get_id() == id && a.is_required_set())
            {
                bail!(
                    "{} required (current: {}; use . for it)",
                    id.to_uppercase(),
                    current.as_deref().unwrap_or("none")
                );
            }
        }
    }
    result.matches = parsed_leaf(&command, &result.args)?;
    Ok(result)
}
impl Resolution {
    pub(crate) fn record_history(
        &self,
        session: &NativeSessionContext,
        service: &str,
        before: &[String],
    ) -> Result<Option<String>> {
        if self.args == before {
            return Ok(None);
        }
        let expansion = HistoryExpansion {
            service: service.into(),
            before: before.to_vec(),
            after: self.args.clone(),
        };
        session.set_value("context.executed", serde_json::to_string(&expansion)?);
        Ok(self.expanded.then(|| {
            std::iter::once(service.to_owned())
                .chain(self.args.iter().cloned())
                .map(|t| crate::core::shell_words::escape_for_shell(&t))
                .collect::<Vec<_>>()
                .join(" ")
        }))
    }

    /// Publishes successful singleton results and records consumed context.
    pub fn finish(&self, response: &mut ResponseV1, session: &NativeSessionContext) {
        for (kind, value) in &self.used {
            response.messages.push(ResponseMessageV1 {
                level: crate::core::MessageLevel::Info,
                text: format!("{kind} {value} (from context)"),
            });
        }
        response.meta.context = self.used.clone();
        if let Some(entries) = &response.context {
            for (kind, value) in entries {
                if self.contract.produces.contains_key(kind) {
                    session.set_context_value(kind, value);
                }
            }
        }
        if response.ok {
            let data = match &response.data {
                serde_json::Value::Array(rows) if rows.len() == 1 => &rows[0],
                data => data,
            };
            for (kind, pointers) in &self.contract.produces {
                let mut values = pointers
                    .iter()
                    .filter_map(|pointer| {
                        if let Some(id) = pointer.strip_prefix('$') {
                            if data
                                .as_object()
                                .is_some_and(|o| o.is_empty() || o.keys().all(|k| k == "errors"))
                            {
                                return None;
                            }
                            self.matches
                                .try_get_one::<String>(id)
                                .ok()
                                .flatten()
                                .cloned()
                        } else {
                            match singleton_field(data, pointer)? {
                                serde_json::Value::String(value) => Some(value.clone()),
                                serde_json::Value::Number(value) => Some(value.to_string()),
                                _ => None,
                            }
                        }
                    })
                    .filter(|v| !v.is_empty())
                    .collect::<std::collections::BTreeSet<_>>();
                if values.len() == 1
                    && let Some(value) = values.pop_first()
                {
                    session.set_context_value(kind, value);
                }
            }
        }
        if response.meta.context_not_found {
            for (kind, value) in &self.used {
                if !self
                    .contract
                    .consumes
                    .iter()
                    .any(|c| c.kind == *kind && c.fill == Fill::Read)
                {
                    continue;
                }
                session.clear_context(Some(kind));
                response.messages.push(ResponseMessageV1 {
                    level: crate::core::MessageLevel::Info,
                    text: format!("{kind} {value} no longer found; cleared context"),
                });
            }
        }
        let empty = response.data.as_array().is_some_and(Vec::is_empty)
            || response
                .data
                .get("items")
                .and_then(serde_json::Value::as_array)
                .is_some_and(Vec::is_empty);
        if !response.ok || empty {
            for consume in self
                .contract
                .consumes
                .iter()
                .filter(|c| c.fill == Fill::Narrow && self.used.contains_key(&c.kind))
            {
                response.messages.push(ResponseMessageV1 {
                    level: crate::core::MessageLevel::Info,
                    text: format!(
                        "hint: Rerun with {}, or run context clear {}.",
                        consume.argument, consume.kind
                    ),
                });
            }
        }
        let visible = session.context_entries();
        if let Ok(mut entries) = session.context_entries.write() {
            for entry in entries.values_mut().filter(|e| {
                visible.contains(e)
                    && (shared(&e.kind) || e.service == session.service)
                    && self.used.contains_key(&e.kind)
            }) {
                entry.used_by = session.command_path.as_deref().unwrap_or_default().into();
            }
        }
    }
}

fn singleton_field<'a>(
    mut data: &'a serde_json::Value,
    pointer: &str,
) -> Option<&'a serde_json::Value> {
    if pointer.is_empty() {
        return Some(data);
    }
    for segment in pointer.strip_prefix('/')?.split('/') {
        if data.as_array().is_some_and(|values| values.len() != 1) {
            return None;
        }
        data = data.pointer(&format!("/{segment}"))?;
    }
    Some(data)
}

pub(crate) fn plugin_grammar(describe: &crate::core::plugin::DescribeCommandV1) -> Command {
    let mut command = Command::new(describe.name.clone()).add(describe.context.clone());
    for (index, arg) in describe.args.iter().enumerate() {
        let mut argument =
            clap::Arg::new(arg.name.clone().unwrap_or_else(|| format!("arg{index}")))
                .required(arg.required);
        if arg.multi {
            argument = argument.num_args(0..);
        }
        command = command.arg(argument);
    }
    for (name, flag) in &describe.flags {
        if flag.alias_of.is_some() {
            continue;
        }
        let mut arg = clap::Arg::new(name.trim_start_matches('-').to_string());
        if let Some(long) = name.strip_prefix("--") {
            arg = arg.long(long.to_string());
        } else if let Some(short) = name.strip_prefix('-').and_then(|s| s.chars().next()) {
            arg = arg.short(short);
        }
        for (alias, _) in describe
            .flags
            .iter()
            .filter(|(_, spec)| spec.alias_of.as_deref() == Some(name))
        {
            if let Some(long) = alias.strip_prefix("--") {
                arg = arg.alias(long.to_owned());
            } else if let Some(short) = alias.strip_prefix('-').and_then(|s| s.chars().next()) {
                arg = arg.short_alias(short);
            }
        }
        if flag.flag_only {
            arg = arg.action(clap::ArgAction::SetTrue);
        } else if flag.multi {
            arg = arg.action(clap::ArgAction::Append);
        }
        command = command.arg(arg);
    }
    command.subcommands(describe.subcommands.iter().map(plugin_grammar))
}

#[derive(Serialize, Deserialize)]
pub(crate) struct HistoryExpansion {
    pub(crate) service: String,
    pub(crate) before: Vec<String>,
    pub(crate) after: Vec<String>,
}
impl HistoryExpansion {
    pub(crate) fn apply(
        &self,
        line: &str,
        config: &crate::config::ResolvedConfig,
    ) -> Option<String> {
        let parsed = crate::repl::input::ReplParsedLine::parse(line, config).ok()?;
        let mut tokens = parsed.command_tokens;
        let mut start = tokens
            .iter()
            .position(|t| t == &self.service)
            .map_or(0, |i| i + 1);
        for (before, after) in self.before.iter().zip(&self.after) {
            let index = start + tokens.get(start..)?.iter().position(|t| t == before)?;
            tokens[index] = after.clone();
            start = index + 1;
        }
        tokens.extend(self.after.iter().skip(self.before.len()).cloned());
        let mut expanded = tokens
            .iter()
            .map(|t| crate::core::shell_words::escape_for_shell(t))
            .collect::<Vec<_>>()
            .join(" ");
        for stage in parsed.stages {
            expanded.push_str(&format!(" | {stage}"));
        }
        Some(expanded)
    }
}
