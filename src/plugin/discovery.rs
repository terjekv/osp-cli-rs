//! Ordered executable discovery and optional, fingerprinted metadata.

use super::{Plugin, PluginManager, PluginSource};
use crate::core::plugin::PluginDescribe;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, HashSet};
use std::path::{Path, PathBuf};
use std::time::UNIX_EPOCH;

#[derive(Clone, Default, Serialize, Deserialize)]
struct CachedDescribe {
    size: u64,
    modified: u128,
    describe: Option<PluginDescribe>,
    issue: Option<String>,
}

impl PluginManager {
    pub(super) fn discover(&self) -> Vec<Plugin> {
        let mut snapshot = self.snapshot.lock().unwrap_or_else(|e| e.into_inner());
        if let Some(plugins) = &*snapshot {
            return plugins.clone();
        }
        let cache_path = self.describe_cache_path();
        let old: BTreeMap<PathBuf, CachedDescribe> = cache_path
            .as_ref()
            .and_then(|path| std::fs::read(path).ok())
            .and_then(|bytes| serde_json::from_slice(&bytes).ok())
            .unwrap_or_default();
        let disabled = self.disabled.lock().unwrap_or_else(|e| e.into_inner());
        let mut seen_paths = HashSet::new();
        let mut seen_names = BTreeMap::new();
        let mut plugins = Vec::new();
        let mut pending = Vec::new();
        for (root, source) in self.search_roots() {
            let Ok(entries) = std::fs::read_dir(root) else {
                continue;
            };
            let mut paths = entries
                .flatten()
                .map(|entry| entry.path())
                .collect::<Vec<_>>();
            paths.sort();
            for path in paths {
                let Some(name) = executable_name(&path) else {
                    continue;
                };
                let canonical = path.canonicalize().unwrap_or_else(|_| path.clone());
                // Symlinks with different names are separate commands.
                if !seen_paths.insert((canonical.clone(), name.clone())) {
                    continue;
                }
                let status = if disabled.contains(&name) {
                    "disabled".to_string()
                } else if let Some(winner) = seen_names.get(&name) {
                    format!("shadowed by {winner}")
                } else {
                    seen_names.insert(name.clone(), path.display().to_string());
                    "ok".to_string()
                };
                let index = plugins.len();
                plugins.push(Plugin {
                    name,
                    path: path.clone(),
                    source,
                    describe: None,
                    issue: None,
                    status,
                });
                if plugins[index].status != "ok" {
                    continue;
                }
                let Ok(metadata) = std::fs::metadata(&canonical) else {
                    continue;
                };
                let modified = metadata
                    .modified()
                    .ok()
                    .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
                    .map(|t| t.as_nanos())
                    .unwrap_or_default();
                let cache_key = if path.is_absolute() {
                    path.clone()
                } else {
                    std::env::current_dir().unwrap_or_default().join(&path)
                };
                let entry = old
                    .get(&cache_key)
                    .filter(|entry| entry.size == metadata.len() && entry.modified == modified)
                    .cloned();
                pending.push((
                    index,
                    cache_key,
                    entry.clone().unwrap_or(CachedDescribe {
                        size: metadata.len(),
                        modified,
                        ..Default::default()
                    }),
                    entry.is_none(),
                ));
            }
        }
        drop(disabled);
        let results = std::thread::scope(|scope| {
            let jobs = pending
                .into_iter()
                .map(|(index, path, mut entry, uncached)| {
                    let plugin = &plugins[index];
                    scope.spawn(move || {
                        if uncached {
                            let marker = std::fs::read(&path)
                                .map(|bytes| {
                                    bytes
                                        .windows(b"--osp-describe".len())
                                        .any(|s| s == b"--osp-describe")
                                })
                                .unwrap_or(false);
                            if marker {
                                match super::run::describe(plugin, self.process_timeout) {
                                    Ok(describe) => entry.describe = Some(describe),
                                    Err(error) => entry.issue = Some(error.to_string()),
                                }
                            }
                        }
                        (index, path, entry)
                    })
                })
                .collect::<Vec<_>>();
            jobs.into_iter()
                .map(|job| {
                    job.join()
                        .unwrap_or_else(|panic| std::panic::resume_unwind(panic))
                })
                .collect::<Vec<_>>()
        });
        let mut cache = BTreeMap::new();
        for (index, path, entry) in results {
            plugins[index].describe = entry.describe.clone();
            plugins[index].issue = entry.issue.clone();
            if let Some(issue) = &entry.issue {
                plugins[index].status = format!("describe error: {issue}");
            }
            cache.insert(path, entry);
        }
        if let Some(path) = cache_path
            && let Ok(payload) = serde_json::to_vec(&cache)
        {
            let result = path
                .parent()
                .map(std::fs::create_dir_all)
                .transpose()
                .and_then(|_| {
                    crate::config::write_text_atomic(&path, &payload, false)
                        .map_err(std::io::Error::other)
                });
            if let Err(error) = result {
                tracing::debug!(%error, "could not save plugin describe cache");
            }
        }
        *snapshot = Some(plugins.clone());
        plugins
    }

    pub(super) fn describe_cache_path(&self) -> Option<PathBuf> {
        self.cache_root
            .clone()
            .or_else(|| {
                self.allow_default_roots
                    .then(crate::config::default_cache_root_dir)
                    .flatten()
            })
            .map(|path| path.join("plugins-describe.json"))
    }

    fn search_roots(&self) -> Vec<(PathBuf, PluginSource)> {
        let mut roots = self
            .explicit_dirs
            .iter()
            .cloned()
            .map(|p| (p, PluginSource::Explicit))
            .collect::<Vec<_>>();
        if self.allow_default_roots
            && let Some(paths) = std::env::var_os("OSP_PLUGIN_PATH")
        {
            roots.extend(std::env::split_paths(&paths).map(|p| (p, PluginSource::Environment)));
        }
        if let Some(root) = self.config_root.clone().or_else(|| {
            self.allow_default_roots
                .then(crate::config::default_config_root_dir)
                .flatten()
        }) {
            roots.push((root.join("plugins"), PluginSource::User));
        }
        if self.allow_bundled_roots
            && let Some(path) = std::env::var_os("OSP_BUNDLED_PLUGIN_DIR")
        {
            roots.push((PathBuf::from(path), PluginSource::Bundled));
        }
        if self.allow_bundled_roots
            && let Ok(exe) = std::env::current_exe()
            && let Some(bin) = exe.parent()
        {
            roots.push((bin.join("plugins"), PluginSource::Bundled));
            roots.push((bin.join("../lib/osp/plugins"), PluginSource::Bundled));
        }
        if self.allow_path_discovery
            && let Some(paths) = std::env::var_os("PATH")
        {
            roots.extend(std::env::split_paths(&paths).map(|p| (p, PluginSource::Path)));
        }
        roots
    }
}

fn executable_name(path: &Path) -> Option<String> {
    if !path.is_file() {
        return None;
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        if std::fs::metadata(path).ok()?.permissions().mode() & 0o111 == 0 {
            return None;
        }
    }
    let file = path.file_name()?.to_str()?;
    #[cfg(windows)]
    let file = file
        .strip_suffix(".exe")
        .or_else(|| file.strip_suffix(".cmd"))
        .or_else(|| file.strip_suffix(".bat"))
        .unwrap_or(file);
    let name = file.strip_prefix("osp-")?;
    (!name.is_empty()).then(|| name.to_string())
}
