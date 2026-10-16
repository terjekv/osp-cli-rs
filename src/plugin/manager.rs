//! Executable inventory, first-found command lookup, and host metadata.

use crate::completion::CompletionNode;
use crate::core::command_policy::CommandPath;
use crate::core::plugin::{DescribeCommandAuthV1, DescribeCommandV1, PluginDescribe};
use crate::core::runtime::RuntimeHints;
use std::collections::{BTreeMap, HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::Duration;

/// Default limit for optional describe subprocesses, in milliseconds.
pub const DEFAULT_PLUGIN_PROCESS_TIMEOUT_MS: usize = 10_000;

/// Search root from which an executable was discovered.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PluginSource {
    /// A directory supplied through `--plugin-dir`.
    Explicit,
    /// A directory supplied through `OSP_PLUGIN_PATH`.
    Environment,
    /// The user's configuration directory.
    User,
    /// An installation directory beside the executable.
    Bundled,
    /// The inherited executable search path.
    Path,
}
impl std::fmt::Display for PluginSource {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Self::Explicit => "explicit",
            Self::Environment => "env",
            Self::User => "user",
            Self::Bundled => "bundled",
            Self::Path => "path",
        })
    }
}

/// One executable and its optional metadata. Describe failures do not prevent runs.
#[derive(Debug, Clone)]
pub struct Plugin {
    /// Command name from the executable's `osp-` suffix.
    pub name: String,
    /// Executable path.
    pub path: PathBuf,
    /// Discovery root kind.
    pub source: PluginSource,
    /// Opt-in command metadata.
    pub describe: Option<PluginDescribe>,
    /// Optional describe failure.
    pub issue: Option<String>,
    /// Inventory status: ok, disabled, describe error, or shadowed.
    pub status: String,
}
impl Plugin {
    pub(crate) fn command(&self) -> DescribeCommandV1 {
        self.describe
            .clone()
            .unwrap_or_default()
            .command(&self.name)
    }
    fn available(&self) -> bool {
        self.status == "ok" || self.issue.is_some()
    }
}

/// Execution origin of a command catalog entry.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CommandSource {
    /// In-process command.
    Native,
    /// External executable, with its discovery root.
    Plugin(PluginSource),
}

/// Shared command metadata used by native and executable commands.
#[derive(Debug, Clone)]
pub struct CommandCatalogEntry {
    /// Top-level command name.
    pub name: String,
    /// Short help text.
    pub about: String,
    /// Authorization metadata.
    pub auth: Option<DescribeCommandAuthV1>,
    /// Immediate subcommand names.
    pub subcommands: Vec<String>,
    /// Completion tree.
    pub completion: CompletionNode,
    /// Execution origin.
    pub source: CommandSource,
}
impl CommandCatalogEntry {
    /// Short authorization hint for help listings.
    pub fn auth_hint(&self) -> Option<String> {
        self.auth.as_ref().and_then(|auth| auth.hint())
    }
}

/// Environment supplied by the host in addition to the inherited environment.
#[derive(Clone, Default)]
pub(crate) struct PluginDispatchContext {
    /// Rendering and session hints.
    pub(crate) runtime_hints: RuntimeHints,
    /// Effective user identity.
    pub(crate) user: Option<String>,
    /// Shared config environment values.
    pub(crate) shared_env: Vec<(String, String)>,
    /// Per-command config values, applied after shared values.
    pub(crate) plugin_env: HashMap<String, Vec<(String, String)>>,
    /// Known secret values supplied through the executable environment.
    pub(crate) secrets: Vec<String>,
}

/// Discovers executable commands and caches optional metadata.
#[derive(Debug)]
pub struct PluginManager {
    pub(super) explicit_dirs: Vec<PathBuf>,
    pub(super) config_root: Option<PathBuf>,
    pub(super) cache_root: Option<PathBuf>,
    pub(super) process_timeout: Duration,
    pub(super) allow_path_discovery: bool,
    pub(super) allow_default_roots: bool,
    pub(super) allow_bundled_roots: bool,
    pub(super) disabled: Mutex<HashSet<String>>,
    pub(super) snapshot: Mutex<Option<Vec<Plugin>>>,
}
impl PluginManager {
    /// Searches explicit roots, environment roots, user and bundled roots, then PATH.
    pub fn new(explicit_dirs: Vec<PathBuf>) -> Self {
        Self {
            explicit_dirs,
            config_root: None,
            cache_root: None,
            process_timeout: Duration::from_millis(DEFAULT_PLUGIN_PROCESS_TIMEOUT_MS as u64),
            allow_path_discovery: true,
            allow_default_roots: true,
            allow_bundled_roots: true,
            disabled: Mutex::default(),
            snapshot: Mutex::default(),
        }
    }
    /// Overrides config and cache roots for an embedded host.
    pub fn with_roots(mut self, config_root: Option<PathBuf>, cache_root: Option<PathBuf>) -> Self {
        self.config_root = config_root;
        self.cache_root = cache_root;
        self
    }
    /// Controls user and environment roots for isolated hosts.
    pub fn with_default_roots(mut self, enabled: bool) -> Self {
        self.allow_default_roots = enabled;
        self
    }
    /// Controls installation roots for isolated hosts.
    pub fn with_bundled_roots(mut self, enabled: bool) -> Self {
        self.allow_bundled_roots = enabled;
        self
    }
    /// Controls inherited PATH scanning for isolated hosts.
    pub fn with_path_discovery(mut self, enabled: bool) -> Self {
        self.allow_path_discovery = enabled;
        self
    }
    /// Sets the describe-only time limit; command runs have no timeout.
    pub fn with_process_timeout(mut self, timeout: Duration) -> Self {
        self.process_timeout = timeout;
        self
    }
    /// Returns the describe time limit.
    pub fn process_timeout(&self) -> Duration {
        self.process_timeout
    }
    /// Returns explicit launch roots.
    pub fn explicit_dirs(&self) -> &[PathBuf] {
        &self.explicit_dirs
    }
    /// Returns the configured user root override.
    pub fn config_root(&self) -> Option<&Path> {
        self.config_root.as_deref()
    }
    /// Returns the configured cache root override.
    pub fn cache_root(&self) -> Option<&Path> {
        self.cache_root.as_deref()
    }
    /// Replaces disabled command names and invalidates the in-memory inventory.
    pub fn set_disabled(&self, names: impl IntoIterator<Item = String>) {
        // Match discovery's lock order.
        let mut snapshot = self.snapshot.lock().unwrap_or_else(|e| e.into_inner());
        *self.disabled.lock().unwrap_or_else(|e| e.into_inner()) = names.into_iter().collect();
        *snapshot = None;
    }
    /// Lists every discovered executable, including disabled and shadowed entries.
    pub fn list(&self) -> Vec<Plugin> {
        self.discover()
    }
    /// Finds the first enabled executable for a command.
    ///
    /// ```
    /// use osp_cli::plugin::PluginManager;
    /// let manager = PluginManager::new(Vec::new())
    ///     .with_default_roots(false).with_bundled_roots(false)
    ///     .with_path_discovery(false);
    /// assert!(manager.find("hello").is_none());
    /// ```
    pub fn find(&self, name: &str) -> Option<Plugin> {
        self.list()
            .into_iter()
            .find(|p| p.name.eq_ignore_ascii_case(name) && p.available())
    }
    /// Drops both the in-memory inventory and the persisted describe cache.
    ///
    /// ```
    /// use osp_cli::plugin::PluginManager;
    /// let manager = PluginManager::new(Vec::new())
    ///     .with_default_roots(false).with_bundled_roots(false)
    ///     .with_path_discovery(false);
    /// manager.refresh();
    /// assert!(manager.list().is_empty());
    /// ```
    pub fn refresh(&self) {
        let mut snapshot = self.snapshot.lock().unwrap_or_else(|e| e.into_inner());
        if let Some(path) = self.describe_cache_path() {
            let _ = std::fs::remove_file(path);
        }
        *snapshot = None;
    }
    /// Builds metadata for the first enabled executable for each command name.
    pub fn command_catalog(&self) -> Vec<CommandCatalogEntry> {
        self.list()
            .into_iter()
            .filter(Plugin::available)
            .map(|plugin| {
                let command = plugin.command();
                CommandCatalogEntry {
                    name: command.name.clone(),
                    about: command.about.clone(),
                    auth: command.auth.clone(),
                    subcommands: command.subcommands.iter().map(|s| s.name.clone()).collect(),
                    completion: CompletionNode::from_describe(&command),
                    source: CommandSource::Plugin(plugin.source),
                }
            })
            .collect()
    }
    pub(crate) fn command_auth(&self) -> BTreeMap<CommandPath, DescribeCommandAuthV1> {
        let mut auth = BTreeMap::new();
        for plugin in self.list().into_iter().filter(Plugin::available) {
            super::catalog::collect_command_auth(&mut auth, &plugin.command(), &[]);
        }
        auth
    }
}
