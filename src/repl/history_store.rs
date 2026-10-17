//! Persistent REPL history scoped by file path and shell prefix.
//!
//! Records are stored oldest-to-newest. Public listing helpers preserve that
//! order while filtering by optional shell prefix,
//! and configured exclusion patterns. Shell-scoped views strip the prefix back
//! off so callers see the command as it was typed inside that shell.
//!
//! Pruning and clearing only remove records visible in the chosen scope.
//!
//! Public API shape:
//!
//! - [`HistoryConfig`] configures persistence and visibility
//! - [`SharedHistory`] is the public facade; the raw `reedline` store stays
//!   crate-private

use std::fs::File;
use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, RwLock};
use std::time::Duration;
#[cfg(not(miri))]
use std::time::{SystemTime, UNIX_EPOCH};

use anyhow::Result;
use miette::miette;
use reedline::{
    CommandLineSearch, History, HistoryItem, HistoryItemId, HistorySessionId, ReedlineError,
    ReedlineErrorVariants, Result as ReedlineResult, SearchDirection, SearchFilter, SearchQuery,
};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum BangCommand {
    Last,
    Relative(usize),
    Absolute { id: usize, suffix: Option<String> },
    Prefix(String),
    Contains(String),
}

pub(crate) fn parse_bang_command(raw: &str) -> miette::Result<Option<BangCommand>> {
    let Some((_, raw)) = split_bang_request(raw) else {
        return Ok(None);
    };
    if raw == "!" {
        return Ok(Some(BangCommand::Prefix(String::new())));
    }
    if raw == "!!" {
        return Ok(Some(BangCommand::Last));
    }
    if let Some(rest) = raw.strip_prefix("!?") {
        let term = rest.trim();
        if term.is_empty() {
            return Err(miette!("`!?` expects search text"));
        }
        return Ok(Some(BangCommand::Contains(term.to_string())));
    }
    if let Some(rest) = raw.strip_prefix("!-") {
        let offset = rest
            .trim()
            .parse::<usize>()
            .map_err(|_| miette!("`!-N` expects a positive integer"))?;
        if offset == 0 {
            return Err(miette!("`!-N` expects N >= 1"));
        }
        return Ok(Some(BangCommand::Relative(offset)));
    }
    let rest = raw.trim_start_matches('!').trim();
    if rest.is_empty() {
        return Ok(Some(BangCommand::Prefix(String::new())));
    }
    let digit_len = rest.bytes().take_while(u8::is_ascii_digit).count();
    let (id, suffix) = rest.split_at(digit_len);
    if !id.is_empty() && (suffix.is_empty() || suffix.starts_with(char::is_whitespace)) {
        let id = id
            .parse::<usize>()
            .map_err(|_| miette!("`!N` expects a positive integer"))?;
        if id == 0 {
            return Err(miette!("`!N` expects N >= 1"));
        }
        return Ok(Some(BangCommand::Absolute {
            id,
            suffix: (!suffix.trim_start().is_empty()).then(|| suffix.trim_start().to_string()),
        }));
    }
    Ok(Some(BangCommand::Prefix(rest.to_string())))
}

pub(crate) fn split_bang_request(raw: &str) -> Option<(bool, &str)> {
    let raw = raw.trim();
    if raw.starts_with('!') {
        return Some((false, raw));
    }

    let rest = raw.strip_prefix("sudo")?;
    if !rest.chars().next().is_some_and(char::is_whitespace) {
        return None;
    }
    let bang = rest.trim_start();
    bang.starts_with('!').then_some((true, bang))
}

/// Configuration for REPL history persistence, visibility, and shell scoping.
#[derive(Debug, Clone)]
#[non_exhaustive]
#[must_use]
pub struct HistoryConfig {
    /// Path to the history file, when persistence is enabled.
    pub path: Option<PathBuf>,
    /// Older file to read while the configured path does not exist.
    pub legacy_path: Option<PathBuf>,
    /// Maximum number of retained history entries.
    pub max_entries: usize,
    /// Whether history capture is enabled.
    pub enabled: bool,
    /// Whether duplicate commands should be collapsed.
    pub dedupe: bool,
    /// Prefix patterns excluded from persistence.
    pub exclude_patterns: Vec<String>,
    /// Shared shell-prefix scope used to filter and strip shell-prefixed views.
    pub shell_context: HistoryShellContext,
}

impl Default for HistoryConfig {
    fn default() -> Self {
        Self {
            path: None,
            legacy_path: None,
            max_entries: 1_000,
            enabled: true,
            dedupe: true,
            exclude_patterns: Vec::new(),
            shell_context: HistoryShellContext::default(),
        }
    }
}

impl HistoryConfig {
    /// Normalizes configured exclusion patterns.
    pub fn normalized(mut self) -> Self {
        self.exclude_patterns =
            normalize_exclude_patterns(std::mem::take(&mut self.exclude_patterns));
        self
    }

    fn load_records(&self, path: &Path) -> std::io::Result<Vec<HistoryRecord>> {
        let read_path = if !path.exists() {
            self.legacy_path.as_deref().unwrap_or(path)
        } else {
            path
        };
        load_records_result(read_path)
    }

    fn persist_enabled(&self) -> bool {
        self.enabled && self.path.is_some() && self.max_entries > 0
    }
}

/// Shared shell-prefix state used to scope history to the current integration shell.
#[derive(Clone, Default, Debug)]
pub struct HistoryShellContext {
    inner: Arc<RwLock<Option<String>>>,
}

impl HistoryShellContext {
    /// Creates a shell context with an initial normalized prefix.
    #[cfg(test)]
    pub fn new(prefix: impl Into<String>) -> Self {
        let context = Self::default();
        context.set_prefix(prefix);
        context
    }

    /// Sets or replaces the normalized shell prefix.
    pub fn set_prefix(&self, prefix: impl Into<String>) {
        if let Ok(mut guard) = self.inner.write() {
            *guard = normalize_shell_prefix(prefix.into());
        }
    }

    /// Clears the current shell prefix.
    #[cfg(test)]
    pub fn clear(&self) {
        if let Ok(mut guard) = self.inner.write() {
            *guard = None;
        }
    }

    /// Returns the current normalized shell prefix, if one is set.
    pub fn prefix(&self) -> Option<String> {
        self.inner.read().map(|value| value.clone()).unwrap_or(None)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
struct HistoryRecord {
    command_line: String,
    #[serde(default)]
    timestamp_ms: Option<i64>,
}

#[derive(Clone)]
struct PendingHistoryRecord {
    index: usize,
    record: HistoryRecord,
}

/// Visible history entry returned by listing operations after scope filtering.
#[derive(Debug, Clone)]
pub struct HistoryEntry {
    /// Stable identifier within the visible ordered entry list.
    pub id: i64,
    /// Recorded timestamp in milliseconds since the Unix epoch, when available.
    pub timestamp_ms: Option<i64>,
    /// Command line as presented in the selected scope.
    pub command: String,
}

/// Thread-safe facade over the REPL history store.
///
/// Clones share the same store and live shell context.
#[derive(Clone)]
pub struct SharedHistory {
    inner: Arc<Mutex<OspHistoryStore>>,
}

impl SharedHistory {
    /// Creates a shared history store from the provided configuration.
    ///
    /// Persisted history loading is best-effort: unreadable files and malformed
    /// lines are ignored during initialization.
    pub fn new(config: HistoryConfig) -> Self {
        Self {
            inner: Arc::new(Mutex::new(OspHistoryStore::new(config))),
        }
    }

    /// Runs one operation while holding the history lock, reporting poisoning.
    pub(crate) fn with<R>(
        &self,
        f: impl FnOnce(&mut OspHistoryStore) -> R,
    ) -> std::result::Result<R, &'static str> {
        self.inner
            .lock()
            .map(|mut store| f(&mut store))
            .map_err(|_| "history lock poisoned")
    }
}

/// History backend storing newline-delimited JSON records.
///
/// The store keeps insertion order, applies visibility rules when presenting
/// commands back to callers, and writes the full persisted record stream back
/// out atomically after mutations.
pub(crate) struct OspHistoryStore {
    config: HistoryConfig,
    records: Vec<HistoryRecord>,
    pending_record: Option<PendingHistoryRecord>,
    last_command: Option<String>,
}

impl OspHistoryStore {
    /// Creates a history store and eagerly loads persisted records when
    /// persistence is enabled.
    pub fn new(config: HistoryConfig) -> Self {
        let config = config.normalized();
        let mut records = Vec::new();
        if config.persist_enabled()
            && let Some(path) = &config.path
        {
            let _phase = tracing::info_span!(target: "osp_startup", "history load", name = %format_args!("history load {}", path.display())).entered();
            records = config.load_records(path).unwrap_or_default();
        }
        let mut store = Self {
            config,
            records,
            pending_record: None,
            last_command: None,
        };
        store.trim_to_capacity();
        store
    }

    /// Returns whether history operations are active for this store.
    ///
    /// This is false when history is disabled or when the configured capacity is
    /// zero.
    pub fn history_enabled(&self) -> bool {
        self.config.enabled && self.config.max_entries > 0
    }

    /// Returns visible commands in oldest-to-newest order for the provided
    /// shell prefix.
    ///
    /// Exclusion patterns still apply.
    pub fn recent_commands_for(&self, shell_prefix: Option<&str>) -> Vec<String> {
        self.visible_record_indices_for(shell_prefix)
            .into_iter()
            .map(|record_index| self.records[record_index].command_line.clone())
            .collect()
    }

    /// Returns visible history entries in oldest-to-newest order for the
    /// provided shell prefix.
    pub fn list_entries_for(&self, shell_prefix: Option<&str>) -> Vec<HistoryEntry> {
        if !self.history_enabled() {
            return Vec::new();
        }
        let shell_prefix = normalize_scope_prefix(shell_prefix);
        self.visible_record_indices_for(shell_prefix.as_deref())
            .into_iter()
            .enumerate()
            .map(|(idx, record_index)| HistoryEntry {
                id: idx as i64 + 1,
                timestamp_ms: self.records[record_index].timestamp_ms,
                command: self.view_command_line(
                    &self.records[record_index].command_line,
                    shell_prefix.as_deref(),
                ),
            })
            .collect()
    }

    /// Removes the oldest visible entries for a specific shell scope, keeping
    /// at most `keep`.
    ///
    /// Returns the number of removed entries.
    pub fn prune_for(&mut self, keep: usize, shell_prefix: Option<&str>) -> Result<usize> {
        if !self.history_enabled() {
            return Ok(0);
        }
        if self.config.persist_enabled() {
            return self.prune_persisted(keep, shell_prefix);
        }
        let eligible = self.visible_record_indices_for(shell_prefix);

        if keep == 0 {
            return self.remove_records(&eligible);
        }

        if eligible.len() <= keep {
            return Ok(0);
        }

        let remove_count = eligible.len() - keep;
        let to_remove = eligible.into_iter().take(remove_count).collect::<Vec<_>>();
        self.remove_records(&to_remove)
    }

    fn prune_persisted(&mut self, keep: usize, shell_prefix: Option<&str>) -> Result<usize> {
        let Some(path) = self.config.path.clone() else {
            return Ok(0);
        };
        let _transaction_lock = crate::config::lock_file_transaction(&path)?;
        let mut records = self.config.load_records(&path)?;
        trim_records_to_capacity(&mut records, self.config.max_entries);

        let mut pending_index = self.pending_record.as_ref().map(|pending| {
            let index = records.len();
            records.push(pending.record.clone());
            index
        });
        let eligible = self
            .visible_record_indices_in(&records, shell_prefix)
            .into_iter()
            .filter(|index| Some(*index) != pending_index)
            .collect::<Vec<_>>();
        let to_remove = if keep == 0 {
            eligible
        } else if eligible.len() <= keep {
            Vec::new()
        } else {
            let remove_count = eligible.len() - keep;
            eligible.into_iter().take(remove_count).collect()
        };

        if !to_remove.is_empty() {
            pending_index = pending_index.and_then(|index| shifted_index(index, &to_remove));
            remove_record_indices(&mut records, &to_remove);
            let mut persisted = records.clone();
            if let Some(index) = pending_index {
                persisted.remove(index);
            }
            trim_records_to_capacity(&mut persisted, self.config.max_entries);
            write_records(&path, &persisted)?;
        }

        let trimmed = trim_records_to_capacity(&mut records, self.config.max_entries);
        pending_index = pending_index.and_then(|index| index.checked_sub(trimmed));
        self.records = records;
        self.pending_record = pending_index.map(|index| PendingHistoryRecord {
            index,
            record: self.records[index].clone(),
        });
        Ok(to_remove.len())
    }

    /// Matches the displayed scope under the file transaction lock. Pending
    /// editor input stays buffered and is never included in the replacement.
    pub fn clear_matching_for(
        &mut self,
        pattern: &str,
        shell_prefix: Option<&str>,
        preview: bool,
        force: bool,
    ) -> Result<Vec<HistoryEntry>> {
        let path = self
            .config
            .path
            .clone()
            .filter(|_| self.config.persist_enabled());
        let _transaction_lock = path
            .as_ref()
            .map(|path| crate::config::lock_file_transaction(path))
            .transpose()?;
        if let Some(path) = &path {
            let mut records = self.config.load_records(path)?;
            trim_records_to_capacity(&mut records, self.config.max_entries);
            if let Some(pending) = &mut self.pending_record {
                pending.index = records.len();
                records.push(pending.record.clone());
            }
            self.records = records;
        }
        let pending_index = self.pending_record.as_ref().map(|pending| pending.index);
        let entries = self.list_entries_for(shell_prefix);
        let indices = self.visible_record_indices_for(shell_prefix);
        let matches = entries
            .into_iter()
            .zip(indices)
            .filter(|(entry, index)| {
                Some(*index) != pending_index && matches_clear_pattern(pattern, &entry.command)
            })
            .collect::<Vec<_>>();
        let (entries, indices): (Vec<_>, Vec<_>) = matches.into_iter().unzip();
        if !preview && (entries.len() <= 100 || force) && !indices.is_empty() {
            // Write before mutating memory so a failed replacement loses nothing.
            if let Some(path) = &path {
                let mut persisted = self.records.clone();
                let mut remove = indices.clone();
                if let Some(index) = pending_index {
                    remove.push(index);
                    remove.sort_unstable();
                }
                remove_record_indices(&mut persisted, &remove);
                write_records(path, &persisted)?;
            }
            self.remove_records(&indices)?;
            self.last_command = None;
        }
        Ok(entries)
    }

    pub(crate) fn expand(
        &self,
        command: &BangCommand,
        scope: Option<&str>,
        elevated: bool,
    ) -> Option<String> {
        let expanded = if matches!(command, BangCommand::Last) {
            self.last_command.clone().or_else(|| {
                self.recent_commands_for(self.shell_prefix().as_deref())
                    .last()
                    .cloned()
            })?
        } else {
            let recent = self.recent_commands_for(scope);
            let selected = match command {
                BangCommand::Last => unreachable!(),
                BangCommand::Relative(offset) => recent.get(recent.len().checked_sub(*offset)?),
                BangCommand::Absolute { id, .. } => recent.get(id.checked_sub(1)?),
                BangCommand::Prefix(prefix) => recent
                    .iter()
                    .rev()
                    .find(|full| strip_shell_prefix(full, scope).starts_with(prefix)),
                BangCommand::Contains(term) => recent
                    .iter()
                    .rev()
                    .find(|full| strip_shell_prefix(full, scope).contains(term)),
            }?;
            let visible = strip_shell_prefix(selected, scope);
            match command {
                BangCommand::Absolute {
                    suffix: Some(suffix),
                    ..
                } => {
                    format!("{visible} {suffix}")
                }
                _ => visible,
            }
        };
        Some(
            if elevated && expanded.split_whitespace().next() != Some("sudo") {
                format!("sudo {expanded}")
            } else {
                expanded
            },
        )
    }

    /// Remember the last submitted command independently of persisted history.
    pub(crate) fn remember_command(&mut self, command_line: &str) {
        if !command_line.trim().is_empty() && !is_sensitive_line(command_line) {
            self.last_command = Some(command_line.trim().to_string());
        }
    }

    /// The same expansion is used by Tab completion and Enter dispatch.
    pub(crate) fn expand_bang_request(&self, raw: &str) -> Option<String> {
        let (elevated, _) = split_bang_request(raw)?;
        let command = parse_bang_command(raw).ok().flatten()?;
        if matches!(&command, BangCommand::Prefix(prefix) if prefix.is_empty()) {
            return None;
        }
        self.expand(&command, self.shell_prefix().as_deref(), elevated)
    }

    /// Commit the editor's pending record, or save an accepted basic-mode replay.
    ///
    /// A sourced child must leave an unrelated pending parent record alone.
    pub(crate) fn finalize_execution(
        &mut self,
        input: &str,
        resolved: &str,
        accepted: bool,
    ) -> Result<()> {
        if input.starts_with(' ') {
            return Ok(());
        }
        let keep = accepted && !is_sensitive_line(input) && !self.is_command_excluded(resolved);
        if input != resolved {
            let original = apply_shell_prefix(input, self.shell_prefix().as_deref());
            let expanded = apply_shell_prefix(resolved, self.shell_prefix().as_deref());
            if let Some(pending) = self
                .pending_record
                .as_mut()
                .filter(|pending| pending.record.command_line == original)
            {
                pending.record.command_line = expanded;
                self.records[pending.index] = pending.record.clone();
            } else if self.pending_record.is_none() && keep {
                self.save(HistoryItem::from_command_line(resolved))?;
            }
        }
        self.finalize_pending(resolved, keep)
    }

    /// Saves and finalizes one accepted command line.
    ///
    /// # Errors
    ///
    /// Returns an error when saving or persisting the item fails.
    #[cfg(test)]
    pub(crate) fn save_command_line(&mut self, command_line: &str) -> Result<()> {
        let item = HistoryItem::from_command_line(command_line);
        self.save(item).map(|_| ())?;
        self.finalize_pending(command_line, true)?;
        Ok(())
    }

    pub(crate) fn stage_command_line(&mut self, command_line: &str) -> Result<()> {
        self.save(HistoryItem::from_command_line(command_line))
            .map(|_| ())?;
        Ok(())
    }

    pub(crate) fn shell_prefix(&self) -> Option<String> {
        self.config.shell_context.prefix()
    }

    fn shell_allows(&self, record: &HistoryRecord, shell_prefix: Option<&str>) -> bool {
        command_matches_shell_prefix(&record.command_line, shell_prefix)
    }

    fn view_command_line(&self, command: &str, shell_prefix: Option<&str>) -> String {
        strip_shell_prefix(command, shell_prefix)
    }

    fn record_view_if_allowed(
        &self,
        record: &HistoryRecord,
        shell_prefix: Option<&str>,
        require_shell: bool,
    ) -> Option<String> {
        if require_shell && !self.shell_allows(record, shell_prefix) {
            return None;
        }
        let view_command = self.view_command_line(&record.command_line, shell_prefix);
        if self.is_command_excluded(&view_command) {
            return None;
        }
        Some(view_command)
    }

    fn visible_record_indices_for(&self, shell_prefix: Option<&str>) -> Vec<usize> {
        self.visible_record_indices_in(&self.records, shell_prefix)
    }

    fn visible_record_indices_in(
        &self,
        records: &[HistoryRecord],
        shell_prefix: Option<&str>,
    ) -> Vec<usize> {
        let shell_prefix = normalize_scope_prefix(shell_prefix);
        let mut out = Vec::new();
        for (record_index, record) in records.iter().enumerate() {
            if self
                .record_view_if_allowed(record, shell_prefix.as_deref(), true)
                .is_none()
            {
                continue;
            }
            out.push(record_index);
        }
        out
    }

    fn is_command_excluded(&self, command: &str) -> bool {
        is_excluded_command(command, &self.config.exclude_patterns)
    }

    fn trim_to_capacity(&mut self) {
        trim_records_to_capacity(&mut self.records, self.config.max_entries);
    }

    fn append_record(&mut self, record: HistoryRecord) -> HistoryItemId {
        self.records.push(record);
        self.trim_to_capacity();
        HistoryItemId::new(self.records.len() as i64 - 1)
    }

    /// Removes records by index; callers pass sorted, distinct existing indices.
    fn remove_records(&mut self, indices: &[usize]) -> Result<usize> {
        if indices.is_empty() {
            return Ok(0);
        }
        let pending_index = self
            .pending_record
            .as_ref()
            .and_then(|pending| shifted_index(pending.index, indices));
        remove_record_indices(&mut self.records, indices);
        self.trim_to_capacity();
        self.pending_record = pending_index.map(|index| PendingHistoryRecord {
            index,
            record: self.records[index].clone(),
        });
        Ok(indices.len())
    }

    fn should_skip_command(&self, command: &str) -> bool {
        is_excluded_command(command, &self.config.exclude_patterns)
    }

    fn finalize_pending(&mut self, command_line: &str, keep: bool) -> Result<()> {
        let expected = apply_shell_prefix(command_line, self.shell_prefix().as_deref());
        let Some(pending) = self
            .pending_record
            .take_if(|pending| pending.record.command_line == expected)
        else {
            return Ok(());
        };

        if keep {
            let restore = pending.clone();
            if let Err(err) = self.commit_pending(pending) {
                self.pending_record = Some(restore);
                return Err(err);
            }
            return Ok(());
        }

        if self.records.get(pending.index) == Some(&pending.record) {
            self.records.remove(pending.index);
            self.trim_to_capacity();
        }
        Ok(())
    }

    fn commit_pending(&mut self, pending: PendingHistoryRecord) -> Result<()> {
        let Some(path) = self
            .config
            .path
            .as_ref()
            .filter(|_| self.config.persist_enabled())
            .cloned()
        else {
            return Ok(());
        };
        let _transaction_lock = crate::config::lock_file_transaction(&path)?;
        let mut records = self.config.load_records(&path)?;
        trim_records_to_capacity(&mut records, self.config.max_entries);
        let duplicate = self.config.dedupe
            && records
                .last()
                .is_some_and(|record| record.command_line == pending.record.command_line);
        if !duplicate {
            records.push(pending.record);
        }
        trim_records_to_capacity(&mut records, self.config.max_entries);
        write_records(&path, &records)?;
        self.records = records;
        Ok(())
    }

    fn record_matches_filter(
        &self,
        record: &HistoryRecord,
        filter: &SearchFilter,
        shell_prefix: Option<&str>,
    ) -> bool {
        let Some(view_command) = self.record_view_if_allowed(record, shell_prefix, true) else {
            return false;
        };
        if let Some(search) = &filter.command_line {
            let matches = match search {
                CommandLineSearch::Prefix(prefix) => view_command.starts_with(prefix),
                CommandLineSearch::Substring(substr) => view_command.contains(substr),
                CommandLineSearch::Exact(exact) => view_command == *exact,
            };
            if !matches {
                return false;
            }
        }
        true
    }

    fn record_from_item(&self, item: &HistoryItem, command_line: String) -> HistoryRecord {
        HistoryRecord {
            command_line,
            timestamp_ms: item.start_timestamp.map(|ts| ts.timestamp_millis()),
        }
    }

    fn history_item_from_record(
        &self,
        record: &HistoryRecord,
        shell_prefix: Option<&str>,
    ) -> HistoryItem {
        let command_line = self.view_command_line(&record.command_line, shell_prefix);
        HistoryItem::from_command_line(&command_line)
    }

    fn reedline_error(message: &'static str) -> ReedlineError {
        ReedlineError(ReedlineErrorVariants::OtherHistoryError(message))
    }

    fn record_matches_query(
        &self,
        record: &HistoryRecord,
        filter: &SearchFilter,
        start_time_ms: Option<i64>,
        end_time_ms: Option<i64>,
        shell_prefix: Option<&str>,
        skip_command_line: Option<&str>,
    ) -> bool {
        if !self.record_matches_filter(record, filter, shell_prefix) {
            return false;
        }
        if let Some(skip) = skip_command_line {
            let view_command = self.view_command_line(&record.command_line, shell_prefix);
            if view_command == skip {
                return false;
            }
        }
        if let Some(start) = start_time_ms {
            match record.timestamp_ms {
                Some(value) if value >= start => {}
                _ => return false,
            }
        }
        if let Some(end) = end_time_ms {
            match record.timestamp_ms {
                Some(value) if value <= end => {}
                _ => return false,
            }
        }
        true
    }
}

impl OspHistoryStore {
    pub(crate) fn save(&mut self, h: HistoryItem) -> ReedlineResult<HistoryItem> {
        self.pending_record = None;
        if !self.config.enabled || self.config.max_entries == 0 || h.command_line.starts_with(' ') {
            return Ok(h);
        }

        let raw = h.command_line.trim();
        if raw.is_empty() {
            return Ok(h);
        }

        // Recall belongs to dispatch: the editor saves before resolving it,
        // and recording that preview would change relative history indices.
        if self.should_skip_command(raw) {
            return Ok(h);
        }
        let shell_prefix = self.shell_prefix();
        let expanded_full = apply_shell_prefix(raw, shell_prefix.as_deref());

        if self.config.dedupe
            && self
                .records
                .last()
                .is_some_and(|record| record.command_line == expanded_full)
        {
            return Ok(h);
        }

        let mut record = self.record_from_item(&h, expanded_full);
        if record.timestamp_ms.is_none() {
            record.timestamp_ms = Some(now_ms());
        }
        let id = self.append_record(record);
        self.pending_record = Some(PendingHistoryRecord {
            index: id.0 as usize,
            record: self.records[id.0 as usize].clone(),
        });

        Ok(HistoryItem {
            id: Some(id),
            command_line: self.records[id.0 as usize].command_line.clone(),
            ..h
        })
    }

    pub(crate) fn load(&self, id: HistoryItemId) -> ReedlineResult<HistoryItem> {
        let idx = id.0 as usize;
        let shell_prefix = self.shell_prefix();
        let record = self
            .records
            .get(idx)
            .ok_or_else(|| Self::reedline_error("history item not found"))?;
        let mut item = self.history_item_from_record(record, shell_prefix.as_deref());
        item.id = Some(id);
        Ok(item)
    }

    pub(crate) fn count(&self, query: SearchQuery) -> ReedlineResult<i64> {
        Ok(self.search(query)?.len() as i64)
    }

    pub(crate) fn search(&self, query: SearchQuery) -> ReedlineResult<Vec<HistoryItem>> {
        let (min_id, max_id) = {
            let start = query.start_id.map(|value| value.0);
            let end = query.end_id.map(|value| value.0);
            if let SearchDirection::Backward = query.direction {
                (end, start)
            } else {
                (start, end)
            }
        };
        let min_id = min_id.map(|value| value + 1).unwrap_or(0);
        let max_id = max_id
            .map(|value| value - 1)
            .unwrap_or(self.records.len().saturating_sub(1) as i64);

        if self.records.is_empty() || max_id < 0 || min_id > max_id {
            return Ok(Vec::new());
        }

        let intrinsic_limit = max_id - min_id + 1;
        let limit = query
            .limit
            .map(|value| std::cmp::min(intrinsic_limit, value) as usize)
            .unwrap_or(intrinsic_limit as usize);

        let start_time_ms = query.start_time.map(|ts| ts.timestamp_millis());
        let end_time_ms = query.end_time.map(|ts| ts.timestamp_millis());
        let shell_prefix = self.shell_prefix();

        let mut results = Vec::new();
        let iter = self
            .records
            .iter()
            .enumerate()
            .skip(min_id as usize)
            .take(intrinsic_limit as usize);
        let skip_command_line = query
            .start_id
            .and_then(|id| self.records.get(id.0 as usize))
            .map(|record| self.view_command_line(&record.command_line, shell_prefix.as_deref()));

        if let SearchDirection::Backward = query.direction {
            for (idx, record) in iter.rev() {
                if results.len() >= limit {
                    break;
                }
                if !self.record_matches_query(
                    record,
                    &query.filter,
                    start_time_ms,
                    end_time_ms,
                    shell_prefix.as_deref(),
                    skip_command_line.as_deref(),
                ) {
                    continue;
                }
                let mut item = self.history_item_from_record(record, shell_prefix.as_deref());
                item.id = Some(HistoryItemId::new(idx as i64));
                results.push(item);
            }
        } else {
            for (idx, record) in iter {
                if results.len() >= limit {
                    break;
                }
                if !self.record_matches_query(
                    record,
                    &query.filter,
                    start_time_ms,
                    end_time_ms,
                    shell_prefix.as_deref(),
                    skip_command_line.as_deref(),
                ) {
                    continue;
                }
                let mut item = self.history_item_from_record(record, shell_prefix.as_deref());
                item.id = Some(HistoryItemId::new(idx as i64));
                results.push(item);
            }
        }

        Ok(results)
    }

    pub(crate) fn update(
        &mut self,
        _id: HistoryItemId,
        _updater: &dyn Fn(HistoryItem) -> HistoryItem,
    ) -> ReedlineResult<()> {
        Err(ReedlineError(
            ReedlineErrorVariants::HistoryFeatureUnsupported {
                history: "OspHistoryStore",
                feature: "updating entries",
            },
        ))
    }

    pub(crate) fn clear(&mut self) -> ReedlineResult<()> {
        if let Some(path) = self.config.path.as_ref() {
            let _transaction_lock = crate::config::lock_file_transaction(path)
                .map_err(|err| ReedlineError(ReedlineErrorVariants::IOError(err)))?;
            if self.config.legacy_path.is_some() {
                write_records(path, &[])
                    .map_err(|err| ReedlineError(ReedlineErrorVariants::IOError(err)))?;
            } else {
                match std::fs::remove_file(path) {
                    Ok(()) => {}
                    Err(err) if err.kind() == std::io::ErrorKind::NotFound => {}
                    Err(err) => return Err(ReedlineError(ReedlineErrorVariants::IOError(err))),
                }
            }
        }
        self.records.clear();
        self.pending_record = None;
        Ok(())
    }

    pub(crate) fn delete(&mut self, _h: HistoryItemId) -> ReedlineResult<()> {
        Err(ReedlineError(
            ReedlineErrorVariants::HistoryFeatureUnsupported {
                history: "OspHistoryStore",
                feature: "removing entries",
            },
        ))
    }

    pub(crate) fn sync(&mut self) -> std::io::Result<()> {
        if let Some(pending) = self.pending_record.take() {
            let restore = pending.clone();
            if let Err(err) = self.commit_pending(pending) {
                self.pending_record = Some(restore);
                return Err(std::io::Error::other(err));
            }
            return Ok(());
        }
        let Some(path) = self
            .config
            .path
            .as_ref()
            .filter(|_| self.config.persist_enabled())
        else {
            return Ok(());
        };
        let _transaction_lock = crate::config::lock_file_transaction(path)?;
        let mut records = self.config.load_records(path)?;
        trim_records_to_capacity(&mut records, self.config.max_entries);
        self.records = records;
        Ok(())
    }

    pub(crate) fn session(&self) -> Option<HistorySessionId> {
        None
    }
}

impl History for SharedHistory {
    fn save(&mut self, h: HistoryItem) -> ReedlineResult<HistoryItem> {
        self.with(|store| store.save(h))
            .map_err(OspHistoryStore::reedline_error)?
    }

    fn load(&self, id: HistoryItemId) -> ReedlineResult<HistoryItem> {
        self.with(|store| store.load(id))
            .map_err(OspHistoryStore::reedline_error)?
    }

    fn count(&self, query: SearchQuery) -> ReedlineResult<i64> {
        self.with(|store| store.count(query))
            .map_err(OspHistoryStore::reedline_error)?
    }

    fn search(&self, query: SearchQuery) -> ReedlineResult<Vec<HistoryItem>> {
        self.with(|store| store.search(query))
            .map_err(OspHistoryStore::reedline_error)?
    }

    fn update(
        &mut self,
        id: HistoryItemId,
        updater: &dyn Fn(HistoryItem) -> HistoryItem,
    ) -> ReedlineResult<()> {
        self.with(|store| store.update(id, updater))
            .map_err(OspHistoryStore::reedline_error)?
    }

    fn clear(&mut self) -> ReedlineResult<()> {
        self.with(OspHistoryStore::clear)
            .map_err(OspHistoryStore::reedline_error)?
    }

    fn delete(&mut self, h: HistoryItemId) -> ReedlineResult<()> {
        self.with(|store| store.delete(h))
            .map_err(OspHistoryStore::reedline_error)?
    }

    fn sync(&mut self) -> std::io::Result<()> {
        self.with(OspHistoryStore::sync)
            .map_err(std::io::Error::other)?
    }

    fn session(&self) -> Option<HistorySessionId> {
        self.with(|store| store.session()).ok().flatten()
    }
}

fn load_records_result(path: &Path) -> std::io::Result<Vec<HistoryRecord>> {
    let file = match File::open(path) {
        Ok(file) => file,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(err) => return Err(err),
    };
    let reader = BufReader::new(file);
    let mut records = Vec::new();
    for line in reader.lines() {
        let line = line?;
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }
        let record: HistoryRecord = match serde_json::from_str(trimmed) {
            Ok(record) => record,
            Err(_) => continue,
        };
        if record.command_line.trim().is_empty() {
            continue;
        }
        records.push(record);
    }
    Ok(records)
}

fn write_records(path: &Path, records: &[HistoryRecord]) -> std::io::Result<()> {
    let mut payload = Vec::new();
    for record in records {
        if is_sensitive_line(&record.command_line) {
            continue;
        }
        serde_json::to_writer(&mut payload, record).map_err(std::io::Error::other)?;
        payload.push(b'\n');
    }
    // History needs atomic visibility between sessions, without making each
    // prompt wait for crash-durable storage on a busy or remote filesystem.
    crate::config::write_text_atomic(
        path,
        &payload,
        true,
        crate::config::AtomicWriteDurability::Buffered,
    )
}

fn trim_records_to_capacity(records: &mut Vec<HistoryRecord>, capacity: usize) -> usize {
    let trimmed = records.len().saturating_sub(capacity);
    if capacity == 0 {
        records.clear();
    } else if trimmed > 0 {
        *records = records.split_off(trimmed);
    }
    trimmed
}

fn shifted_index(index: usize, removed: &[usize]) -> Option<usize> {
    if removed.binary_search(&index).is_ok() {
        None
    } else {
        Some(index - removed.partition_point(|removed| *removed < index))
    }
}

fn remove_record_indices(records: &mut Vec<HistoryRecord>, indices: &[usize]) {
    let mut removals = indices.iter().copied().peekable();
    let mut index = 0usize;
    records.retain(|_| {
        let keep = removals.next_if_eq(&index).is_none();
        index += 1;
        keep
    });
}

fn normalize_exclude_patterns(patterns: Vec<String>) -> Vec<String> {
    patterns
        .into_iter()
        .map(|pattern| pattern.trim().to_string())
        .filter(|pattern| !pattern.is_empty())
        .collect()
}

fn normalize_shell_prefix(value: String) -> Option<String> {
    let trimmed = value.trim();
    (!trimmed.is_empty()).then(|| format!("{trimmed} "))
}

fn normalize_scope_prefix(shell_prefix: Option<&str>) -> Option<String> {
    shell_prefix.and_then(|value| normalize_shell_prefix(value.to_string()))
}

fn command_matches_shell_prefix(command: &str, shell_prefix: Option<&str>) -> bool {
    match shell_prefix {
        Some(prefix) => command.starts_with(prefix),
        None => true,
    }
}

pub(crate) fn apply_shell_prefix(command: &str, shell_prefix: Option<&str>) -> String {
    let trimmed = command.trim();
    if trimmed.is_empty() {
        return String::new();
    }
    match shell_prefix {
        Some(prefix) => {
            let prefix_trimmed = prefix.trim_end();
            if trimmed == prefix_trimmed || trimmed.starts_with(prefix) {
                return trimmed.to_string();
            }
            let mut out = String::with_capacity(prefix.len() + trimmed.len());
            out.push_str(prefix);
            out.push_str(trimmed);
            out
        }
        _ => trimmed.to_string(),
    }
}

pub(crate) fn strip_shell_prefix(command: &str, shell_prefix: Option<&str>) -> String {
    let trimmed = command.trim();
    if trimmed.is_empty() {
        return String::new();
    }
    match shell_prefix {
        Some(prefix) => trimmed
            .strip_prefix(prefix)
            .map(|rest| rest.trim_start().to_string())
            .unwrap_or_else(|| trimmed.to_string()),
        None => trimmed.to_string(),
    }
}

fn now_ms() -> i64 {
    #[cfg(miri)]
    {
        // Miri isolation does not support realtime clock queries. History
        // ordering tests only need a stable timestamp shape, not wall-clock
        // accuracy.
        0
    }

    #[cfg(not(miri))]
    {
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_else(|_| Duration::from_secs(0));
        now.as_millis() as i64
    }
}

fn is_excluded_command(command: &str, exclude_patterns: &[String]) -> bool {
    let trimmed = command.trim();
    if trimmed.is_empty() {
        return true;
    }
    if split_bang_request(trimmed).is_some() {
        return true;
    }
    // Use the command lexer so help-like text inside a quoted value is not
    // treated as an invocation flag.
    let Ok(pipeline) = crate::dsl::parse_pipeline(trimmed) else {
        return true;
    };
    let Ok(words) = shell_words::split(&pipeline.command) else {
        return true;
    };
    if words
        .iter()
        .any(|word| matches!(word.as_str(), "--help" | "-h"))
    {
        return true;
    }
    // Keep ordinary config writes, while using the existing command grammar and
    // sensitivity policy to avoid retaining inline credentials.
    if is_sensitive_line(trimmed) {
        return true;
    }
    exclude_patterns
        .iter()
        .any(|pattern| matches_pattern(pattern, trimmed))
}

fn is_sensitive_line(line: &str) -> bool {
    shell_words::split(line).map_or(true, |tokens| has_sensitive_arguments(&tokens))
}

fn is_sensitive_argument(token: &str) -> bool {
    let (name, value) = token.split_once('=').unwrap_or((token, ""));
    if (token.contains('=') || name.starts_with('-'))
        && crate::config::is_sensitive_key(name.trim_start_matches('-'))
    {
        return true;
    }
    // Templates, alias bodies and attached --field=NAME=VALUE carry arguments too.
    (!value.is_empty() && is_sensitive_argument(value))
        || shell_words::split(token).is_ok_and(|words| {
            words.len() > 1 && words.iter().any(|word| is_sensitive_argument(word))
        })
}

pub(super) fn has_sensitive_arguments(tokens: &[String]) -> bool {
    if tokens.iter().any(|token| is_sensitive_argument(token)) {
        return true;
    }
    let Ok(scanned) = crate::cli::invocation::scan_command_tokens(tokens) else {
        return true;
    };
    let words = scanned.tokens;
    let Some(config_set) = words
        .windows(2)
        .position(|pair| pair[0] == "config" && pair[1] == "set")
    else {
        return false;
    };
    let Ok(Some(crate::cli::Commands::Config(args))) =
        crate::cli::parse_inline_command_tokens(&words[config_set..])
    else {
        // A config write that cannot be classified is unsafe to persist.
        return true;
    };
    let crate::cli::ConfigCommands::Set(set) = args.command else {
        return false;
    };
    set.store.secrets || crate::config::is_sensitive_key(&set.key)
}

fn matches_clear_pattern(pattern: &str, command: &str) -> bool {
    // Greedy glob matching with an implicit trailing star; ? consumes a Unicode
    // character, not one byte. Only * and ? have special meaning.
    let pattern = pattern
        .chars()
        .chain(std::iter::once('*'))
        .collect::<Vec<_>>();
    let command = command.chars().collect::<Vec<_>>();
    let (mut p, mut c, mut star, mut retry) = (0, 0, None, 0);
    while c < command.len() {
        if p < pattern.len() && pattern[p] == '*' {
            star = Some(p);
            p += 1;
            retry = c;
        } else if p < pattern.len() && (pattern[p] == '?' || pattern[p] == command[c]) {
            p += 1;
            c += 1;
        } else if let Some(index) = star {
            retry += 1;
            c = retry;
            p = index + 1;
        } else {
            return false;
        }
    }
    pattern[p..].iter().all(|ch| *ch == '*')
}

fn matches_pattern(pattern: &str, command: &str) -> bool {
    let pattern = pattern.trim();
    if pattern.is_empty() {
        return false;
    }
    if pattern == "*" {
        return true;
    }
    if !pattern.contains('*') {
        return pattern == command;
    }

    let parts: Vec<&str> = pattern.split('*').collect();
    let mut cursor = 0usize;

    let mut first = true;
    for part in &parts {
        if part.is_empty() {
            continue;
        }
        if first && !pattern.starts_with('*') {
            if !command[cursor..].starts_with(part) {
                return false;
            }
            cursor += part.len();
        } else if let Some(pos) = command[cursor..].find(part) {
            cursor += pos + part.len();
        } else {
            return false;
        }
        first = false;
    }

    if !pattern.ends_with('*')
        && let Some(last) = parts.iter().rev().find(|part| !part.is_empty())
        && !command.ends_with(last)
    {
        return false;
    }

    true
}

#[cfg(test)]
mod tests;
