//! Helpers for editing TOML-backed config stores on disk.
//!
//! This module exists to keep config-file mutation logic separate from config
//! resolution. Callers provide a key, typed value, and scope; this layer validates
//! them against the built-in schema and applies the edit atomically to the right
//! TOML table structure. Custom resolver schemas are not consulted, and writes
//! do not refresh an existing resolved config or runtime.
//!
//! Contract:
//!
//! - this module owns on-disk TOML edits and atomic write behavior
//! - schema validation and scope validation still happen before a write lands
//! - callers should treat these helpers as persistence primitives, not config
//!   resolution APIs

use std::fs::{File, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use crate::config::{
    ConfigError, ConfigValue, Scope, TomlEditResult, TomlParseDiagnostic, normalize_scope,
    validate_key_scope, with_path_context,
};
use crate::normalize::normalize_identifier;

/// Options that control how TOML-backed config edits are applied.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
#[must_use = "TomlStoreEditOptions must be passed to a TOML store edit function to have any effect"]
pub struct TomlStoreEditOptions {
    mode: TomlStoreEditMode,
    secret_permissions: TomlSecretPermissions,
}

impl TomlStoreEditOptions {
    /// Creates edit options for a normal persisted write.
    pub const fn new() -> Self {
        Self {
            mode: TomlStoreEditMode::Persist,
            secret_permissions: TomlSecretPermissions::ProcessDefault,
        }
    }

    /// Creates edit options for a dry run that validates and computes diffs
    /// without writing the file.
    pub const fn dry_run() -> Self {
        Self {
            mode: TomlStoreEditMode::DryRun,
            secret_permissions: TomlSecretPermissions::ProcessDefault,
        }
    }

    /// Fill a missing or blank string without replacing a value set concurrently.
    pub const fn fill_empty() -> Self {
        Self::new().with_mode(TomlStoreEditMode::FillEmpty)
    }

    /// Replaces the edit mode.
    pub const fn with_mode(mut self, mode: TomlStoreEditMode) -> Self {
        self.mode = mode;
        self
    }

    /// Replaces the secret-file permission policy used for atomic temp-file
    /// creation.
    pub const fn with_secret_permissions(
        mut self,
        secret_permissions: TomlSecretPermissions,
    ) -> Self {
        self.secret_permissions = secret_permissions;
        self
    }

    /// Uses owner-only temp-file permissions suitable for secrets stores.
    pub const fn for_secrets(mut self) -> Self {
        self.secret_permissions = TomlSecretPermissions::OwnerOnly;
        self
    }

    pub(crate) const fn should_write(self) -> bool {
        !matches!(self.mode, TomlStoreEditMode::DryRun)
    }

    pub(crate) const fn strict_secret_permissions(self) -> bool {
        matches!(self.secret_permissions, TomlSecretPermissions::OwnerOnly)
    }
}

/// Whether a TOML store edit should be written to disk or treated as a dry
/// run.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum TomlStoreEditMode {
    /// Validate and persist the edit to disk.
    #[default]
    Persist,
    /// Persist only when the key is missing or contains a blank string.
    FillEmpty,
    /// Validate and compute the edit result without writing the file.
    DryRun,
}

/// Permission policy used for temp files created during atomic TOML writes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum TomlSecretPermissions {
    /// Use the process default permission behavior for temp-file creation.
    #[default]
    ProcessDefault,
    /// Request owner-only temp-file permissions (`0o600` on Unix).
    OwnerOnly,
}

/// Writes one scoped key into a TOML-backed config store.
///
/// The edit runs through built-in schema and scope validation first and returns
/// the previously stored typed value when the key already existed.
/// Custom resolver schemas are not consulted, and the caller must reload any
/// resolved config that should reflect the persisted edit.
///
/// `options` controls whether the edit is a dry run and whether the atomic
/// write path should request owner-only temp-file permissions for secrets
/// stores.
///
/// # Examples
///
/// ```
/// use osp_cli::config::{
///     ConfigValue, Scope, TomlStoreEditOptions, set_scoped_value_in_toml,
///     unset_scoped_value_in_toml,
/// };
///
/// let path = std::env::temp_dir().join(format!(
///     "osp-cli-doc-{}-{}.toml",
///     std::process::id(),
///     std::time::SystemTime::now()
///         .duration_since(std::time::UNIX_EPOCH)
///         .unwrap()
///         .as_nanos()
/// ));
/// let _ = std::fs::remove_file(&path);
///
/// let value = ConfigValue::String("dracula".to_string());
/// let options = TomlStoreEditOptions::new();
///
/// set_scoped_value_in_toml(
///     &path,
///     "theme.name",
///     &value,
///     &Scope::global(),
///     options,
/// )
/// .unwrap();
/// let removed = unset_scoped_value_in_toml(
///     &path,
///     "theme.name",
///     &Scope::global(),
///     options,
/// )
/// .unwrap();
///
/// assert_eq!(removed.previous, Some(value));
/// let _ = std::fs::remove_file(&path);
/// ```
pub fn set_scoped_value_in_toml(
    path: &Path,
    key: &str,
    value: &ConfigValue,
    scope: &Scope,
    options: TomlStoreEditOptions,
) -> Result<TomlEditResult, ConfigError> {
    edit_scoped_value_in_toml(path, key, scope, TomlEditOperation::Set(value), options)
}

/// Removes one scoped key from a TOML-backed config store.
///
/// The returned edit result includes the previous typed value so callers can
/// report or inspect what changed without reparsing the file.
///
/// `options` has the same meaning as on
/// [`set_scoped_value_in_toml`].
pub fn unset_scoped_value_in_toml(
    path: &Path,
    key: &str,
    scope: &Scope,
    options: TomlStoreEditOptions,
) -> Result<TomlEditResult, ConfigError> {
    edit_scoped_value_in_toml(path, key, scope, TomlEditOperation::Unset, options)
}

enum TomlEditOperation<'a> {
    Set(&'a ConfigValue),
    Unset,
}

enum ValidatedTomlEditOperation {
    Set(ConfigValue),
    Unset,
}

fn edit_scoped_value_in_toml(
    path: &Path,
    key: &str,
    scope: &Scope,
    operation: TomlEditOperation<'_>,
    options: TomlStoreEditOptions,
) -> Result<TomlEditResult, ConfigError> {
    let normalized_scope = normalize_scope(scope.clone());
    validate_dotted_key(key)?;
    let schema = crate::config::ConfigSchema::default();
    schema.validate_writable_key(key)?;
    validate_key_scope(key, &normalized_scope)?;
    let operation = match operation {
        TomlEditOperation::Set(value) => {
            ValidatedTomlEditOperation::Set(schema.validate_write_value(key, value)?)
        }
        TomlEditOperation::Unset => ValidatedTomlEditOperation::Unset,
    };
    let _transaction_lock = options
        .should_write()
        .then(|| lock_file_transaction(path))
        .transpose()
        .map_err(|err| ConfigError::FileWrite {
            path: path.display().to_string(),
            reason: err.to_string(),
        })?;
    let mut root =
        load_or_create_toml_root_with_source(path, !options.strict_secret_permissions())?;
    let root_table = root
        .as_table_mut()
        .ok_or(ConfigError::TomlRootMustBeTable)?;

    let previous = match operation {
        ValidatedTomlEditOperation::Set(value) => set_scoped_value(
            root_table,
            &normalized_scope,
            key,
            &value,
            matches!(options.mode, TomlStoreEditMode::FillEmpty),
        )?,
        ValidatedTomlEditOperation::Unset => {
            unset_dotted_value(root_table, &normalized_scope, key)?
        }
    };

    if options.should_write() {
        write_toml_root(path, &root, options.strict_secret_permissions())?;
    }

    Ok(TomlEditResult { previous })
}

fn validate_dotted_key(key: &str) -> Result<(), ConfigError> {
    if key.split('.').all(|part| part.trim().is_empty()) {
        return Err(ConfigError::InvalidConfigKey {
            key: key.to_string(),
            reason: "empty key path".to_string(),
        });
    }
    Ok(())
}

#[cfg(test)]
fn load_or_create_toml_root(path: &Path) -> Result<toml::Value, ConfigError> {
    load_or_create_toml_root_with_source(path, true)
}

fn load_or_create_toml_root_with_source(
    path: &Path,
    include_source: bool,
) -> Result<toml::Value, ConfigError> {
    if !path.exists() {
        return Ok(toml::Value::Table(toml::value::Table::new()));
    }

    let raw = std::fs::read_to_string(path).map_err(|err| ConfigError::FileRead {
        path: path.display().to_string(),
        reason: err.to_string(),
    })?;

    raw.parse::<toml::Value>().map_err(|err| {
        let diagnostic = TomlParseDiagnostic::new(err.message());
        with_path_context(
            path.display().to_string(),
            ConfigError::TomlParse(if include_source {
                diagnostic.with_source(path.display().to_string(), raw.clone(), err.span())
            } else {
                diagnostic.with_location_from_source(&raw, err.span())
            }),
        )
    })
}

pub(crate) fn lock_file_transaction(path: &Path) -> std::io::Result<File> {
    let parent = path
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."));
    std::fs::create_dir_all(parent)?;
    let file_name = path.file_name().ok_or_else(|| {
        std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            format!("path has no file name: {}", path.display()),
        )
    })?;
    let lock_path = parent.join(format!(".{}.lock", file_name.to_string_lossy()));
    let lock = open_lock_file(&lock_path)?;
    lock.lock()?;
    Ok(lock)
}

fn open_lock_file(path: &Path) -> std::io::Result<File> {
    let mut options = OpenOptions::new();
    options.read(true).write(true).create(true).truncate(false);

    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;

        options.mode(0o600);
    }
    options.open(path)
}

fn write_toml_root(
    path: &Path,
    root: &toml::Value,
    strict_secret_permissions: bool,
) -> Result<(), ConfigError> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|err| ConfigError::FileWrite {
            path: parent.display().to_string(),
            reason: err.to_string(),
        })?;
    }

    let payload = toml::to_string_pretty(root)
        .map_err(|err| ConfigError::TomlParse(TomlParseDiagnostic::new(err.to_string())))?;
    write_text_atomic(path, payload.as_bytes(), strict_secret_permissions).map_err(|err| {
        ConfigError::FileWrite {
            path: path.display().to_string(),
            reason: err.to_string(),
        }
    })?;

    Ok(())
}

pub(crate) fn write_text_atomic(
    path: &Path,
    payload: &[u8],
    strict_secret_permissions: bool,
) -> std::io::Result<()> {
    let parent = path.parent().unwrap_or_else(|| Path::new("."));
    let file_name = path.file_name().ok_or_else(|| {
        std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            format!("path has no file name: {}", path.display()),
        )
    })?;
    let pid = std::process::id();
    let nonce = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();

    for attempt in 0..16u8 {
        let temp_name = format!(
            ".{}.tmp-{pid}-{nonce}-{attempt}",
            file_name.to_string_lossy()
        );
        let temp_path = parent.join(temp_name);
        match create_temp_file(&temp_path, strict_secret_permissions) {
            Ok(mut file) => {
                let write_result = file.write_all(payload).and_then(|()| file.sync_all());
                drop(file);
                let result = write_result
                    .and_then(|()| replace_file_atomic(&temp_path, path))
                    .and_then(|()| sync_parent_dir(parent));
                if result.is_err() {
                    let _ = std::fs::remove_file(&temp_path);
                }
                return result;
            }
            Err(err) if err.kind() == std::io::ErrorKind::AlreadyExists => continue,
            Err(err) => return Err(err),
        }
    }

    Err(std::io::Error::new(
        std::io::ErrorKind::AlreadyExists,
        format!("failed to allocate temp file for {}", path.display()),
    ))
}

#[cfg(not(windows))]
fn replace_file_atomic(source: &Path, destination: &Path) -> std::io::Result<()> {
    std::fs::rename(source, destination)
}

#[cfg(windows)]
fn replace_file_atomic(source: &Path, destination: &Path) -> std::io::Result<()> {
    use std::os::windows::ffi::OsStrExt;

    const MOVEFILE_REPLACE_EXISTING: u32 = 0x1;
    const MOVEFILE_WRITE_THROUGH: u32 = 0x8;

    unsafe extern "system" {
        fn MoveFileExW(
            lp_existing_file_name: *const u16,
            lp_new_file_name: *const u16,
            dw_flags: u32,
        ) -> i32;
    }

    let source_wide = source
        .as_os_str()
        .encode_wide()
        .chain(std::iter::once(0))
        .collect::<Vec<_>>();
    let destination_wide = destination
        .as_os_str()
        .encode_wide()
        .chain(std::iter::once(0))
        .collect::<Vec<_>>();

    let replaced = unsafe {
        MoveFileExW(
            source_wide.as_ptr(),
            destination_wide.as_ptr(),
            MOVEFILE_REPLACE_EXISTING | MOVEFILE_WRITE_THROUGH,
        )
    };
    if replaced != 0 {
        Ok(())
    } else {
        Err(std::io::Error::last_os_error())
    }
}

#[cfg(unix)]
fn sync_parent_dir(path: &Path) -> std::io::Result<()> {
    std::fs::File::open(path)?.sync_all()
}

#[cfg(not(unix))]
fn sync_parent_dir(_path: &Path) -> std::io::Result<()> {
    Ok(())
}

#[cfg(unix)]
fn create_temp_file(
    path: &Path,
    strict_secret_permissions: bool,
) -> std::io::Result<std::fs::File> {
    use std::os::unix::fs::OpenOptionsExt;

    let mut options = std::fs::OpenOptions::new();
    options.write(true).create_new(true);
    if strict_secret_permissions {
        options.mode(0o600);
    }
    options.open(path)
}

#[cfg(not(unix))]
fn create_temp_file(
    path: &Path,
    _strict_secret_permissions: bool,
) -> std::io::Result<std::fs::File> {
    let mut options = std::fs::OpenOptions::new();
    options.write(true).create_new(true);
    options.open(path)
}

#[cfg(unix)]
/// Returns the Unix permission bits for a secrets file.
///
/// Unix-only.
pub fn secret_file_mode(path: &Path) -> Result<u32, ConfigError> {
    use std::os::unix::fs::PermissionsExt;

    let metadata = std::fs::metadata(path).map_err(|err| ConfigError::FileRead {
        path: path.display().to_string(),
        reason: err.to_string(),
    })?;
    Ok(metadata.permissions().mode() & 0o777)
}

fn canonical_scope_path(scope: &Scope) -> Vec<String> {
    let path = match (scope.profile.as_deref(), scope.terminal.as_deref()) {
        (None, None) => vec!["default"],
        (Some(profile), None) => vec!["profile", profile],
        (None, Some(terminal)) => vec!["terminal", terminal],
        (Some(profile), Some(terminal)) => vec!["terminal", terminal, "profile", profile],
    };
    path.into_iter().map(str::to_string).collect()
}

/// Returns every stored table path that holds a normalized scope, in the
/// order the loader reads them.
///
/// The loader normalizes profile and terminal names, so `[profile.TSD]` and
/// `[profile.tsd]` both hold the `tsd` scope and the later table wins.
fn stored_scope_paths(root: &toml::value::Table, scope: &Scope) -> Vec<Vec<String>> {
    let mut paths = vec![(Vec::new(), root)];
    for (index, section) in canonical_scope_path(scope).into_iter().enumerate() {
        let mut next = Vec::new();
        for (path, table) in paths {
            // Profile and terminal names follow their `profile`/`terminal` table.
            let keys = table.keys().filter(|key| {
                if index % 2 == 1 {
                    normalize_identifier(key) == section
                } else {
                    **key == section
                }
            });
            for key in keys {
                if let Some(child) = table.get(key).and_then(toml::Value::as_table) {
                    let mut child_path = path.clone();
                    child_path.push(key.clone());
                    next.push((child_path, child));
                }
            }
        }
        paths = next;
    }
    paths.into_iter().map(|(path, _)| path).collect()
}

/// Sets a value so that it is the one the loader sees for the scope: written
/// to the exact table if one exists, otherwise to an existing spelling, and
/// removed from any other table holding the same scope.
fn set_scoped_value(
    root: &mut toml::value::Table,
    scope: &Scope,
    dotted_key: &str,
    value: &ConfigValue,
    only_if_empty: bool,
) -> Result<Option<ConfigValue>, ConfigError> {
    let parts = dotted_key_parts(dotted_key)?;
    let stored = stored_scope_paths(root, scope);
    // Report and test the value the loader sees, before any table changes.
    let previous = scoped_value(root, &stored, &parts).cloned();
    if only_if_empty
        && let Some(existing) = &previous
        && !matches!(existing, toml::Value::String(text) if text.trim().is_empty())
    {
        return ConfigValue::from_toml(dotted_key, existing).map(Some);
    }
    let canonical = canonical_scope_path(scope);
    let target = if stored.contains(&canonical) {
        canonical
    } else {
        stored.first().cloned().unwrap_or(canonical)
    };
    for path in stored.iter().filter(|path| **path != target) {
        remove_dotted_value(ensure_table_path(root, path)?, &parts)?;
        prune_empty_table_path(root, path)?;
    }
    set_dotted_value(ensure_table_path(root, &target)?, dotted_key, value, false)?;
    Ok(previous.and_then(|existing| ConfigValue::from_toml(dotted_key, &existing).ok()))
}

/// The value the loader sees for `parts` across the scope's stored tables.
fn scoped_value<'a>(
    root: &'a toml::value::Table,
    stored: &[Vec<String>],
    parts: &[&str],
) -> Option<&'a toml::Value> {
    stored
        .iter()
        .rev()
        .filter_map(|path| get_table_path(root, path).ok().flatten())
        .find_map(|table| read_dotted_value(table, parts))
}

fn ensure_table_path<'a>(
    table: &'a mut toml::value::Table,
    path: &[impl AsRef<str>],
) -> Result<&'a mut toml::value::Table, ConfigError> {
    let mut cursor = table;
    for section in path {
        cursor = ensure_table(cursor, section.as_ref())?;
    }
    Ok(cursor)
}

fn get_table_path<'a>(
    table: &'a toml::value::Table,
    path: &[impl AsRef<str>],
) -> Result<Option<&'a toml::value::Table>, ConfigError> {
    let mut cursor = table;
    for section in path {
        let Some(next) = get_table(cursor, section.as_ref())? else {
            return Ok(None);
        };
        cursor = next;
    }
    Ok(Some(cursor))
}

fn ensure_table<'a>(
    table: &'a mut toml::value::Table,
    key: &str,
) -> Result<&'a mut toml::value::Table, ConfigError> {
    let entry = table
        .entry(key.to_string())
        .or_insert_with(|| toml::Value::Table(toml::value::Table::new()));
    match entry {
        toml::Value::Table(inner) => Ok(inner),
        _ => Err(ConfigError::InvalidSection {
            section: key.to_string(),
            expected: "table".to_string(),
        }),
    }
}

fn get_table<'a>(
    table: &'a toml::value::Table,
    key: &str,
) -> Result<Option<&'a toml::value::Table>, ConfigError> {
    let Some(entry) = table.get(key) else {
        return Ok(None);
    };
    match entry {
        toml::Value::Table(inner) => Ok(Some(inner)),
        _ => Err(ConfigError::InvalidSection {
            section: key.to_string(),
            expected: "table".to_string(),
        }),
    }
}

fn set_dotted_value(
    table: &mut toml::value::Table,
    dotted_key: &str,
    value: &ConfigValue,
    only_if_empty: bool,
) -> Result<Option<ConfigValue>, ConfigError> {
    let parts = dotted_key
        .split('.')
        .map(str::trim)
        .filter(|part| !part.is_empty())
        .collect::<Vec<&str>>();

    if parts.is_empty() {
        return Err(ConfigError::InvalidConfigKey {
            key: dotted_key.to_string(),
            reason: "empty key path".to_string(),
        });
    }

    let mut cursor = table;
    for key in &parts[..parts.len() - 1] {
        cursor = ensure_table(cursor, key)?;
    }

    let leaf = parts[parts.len() - 1];
    if only_if_empty
        && let Some(existing) = cursor.get(leaf)
        && !matches!(existing, toml::Value::String(text) if text.trim().is_empty())
    {
        return ConfigValue::from_toml(dotted_key, existing).map(Some);
    }
    let previous = cursor
        .insert(leaf.to_string(), config_value_to_toml(value))
        .and_then(|existing| ConfigValue::from_toml(dotted_key, &existing).ok());

    Ok(previous)
}

fn unset_dotted_value(
    root: &mut toml::value::Table,
    scope: &Scope,
    dotted_key: &str,
) -> Result<Option<ConfigValue>, ConfigError> {
    let parts = dotted_key_parts(dotted_key)?;
    let stored = stored_scope_paths(root, scope);
    let previous = scoped_value(root, &stored, &parts)
        .and_then(|value| ConfigValue::from_toml(dotted_key, value).ok());

    for path in &stored {
        remove_dotted_value(ensure_table_path(root, path)?, &parts)?;
        prune_empty_table_path(root, path)?;
    }

    Ok(previous)
}

fn dotted_key_parts(dotted_key: &str) -> Result<Vec<&str>, ConfigError> {
    let parts = dotted_key
        .split('.')
        .map(str::trim)
        .filter(|part| !part.is_empty())
        .collect::<Vec<&str>>();
    if parts.is_empty() {
        return Err(ConfigError::InvalidConfigKey {
            key: dotted_key.to_string(),
            reason: "empty key path".to_string(),
        });
    }
    Ok(parts)
}

fn remove_dotted_value(
    table: &mut toml::value::Table,
    parts: &[&str],
) -> Result<bool, ConfigError> {
    if parts.is_empty() {
        return Ok(false);
    }

    if parts.len() == 1 {
        return Ok(table.remove(parts[0]).is_some());
    }

    let Some(entry) = table.get_mut(parts[0]) else {
        return Ok(false);
    };
    let child = match entry {
        toml::Value::Table(inner) => inner,
        _ => {
            return Err(ConfigError::InvalidSection {
                section: parts[0].to_string(),
                expected: "table".to_string(),
            });
        }
    };

    let removed = remove_dotted_value(child, &parts[1..])?;
    if removed && child.is_empty() {
        table.remove(parts[0]);
    }
    Ok(removed)
}

fn prune_empty_table_path(
    table: &mut toml::value::Table,
    path: &[impl AsRef<str>],
) -> Result<(), ConfigError> {
    let Some((head, tail)) = path.split_first() else {
        return Ok(());
    };
    let head = head.as_ref();
    if tail.is_empty() {
        remove_empty_table(table, head);
        return Ok(());
    }

    let should_remove = if let Some(value) = table.get_mut(head) {
        let child = as_table_mut(value, head)?;
        prune_empty_table_path(child, tail)?;
        child.is_empty()
    } else {
        false
    };
    if should_remove {
        table.remove(head);
    }
    Ok(())
}

fn remove_empty_table(table: &mut toml::value::Table, key: &str) {
    let should_remove = table
        .get(key)
        .and_then(toml::Value::as_table)
        .is_some_and(|inner| inner.is_empty());
    if should_remove {
        table.remove(key);
    }
}

fn as_table_mut<'a>(
    value: &'a mut toml::Value,
    section: &str,
) -> Result<&'a mut toml::value::Table, ConfigError> {
    match value {
        toml::Value::Table(inner) => Ok(inner),
        _ => Err(ConfigError::InvalidSection {
            section: section.to_string(),
            expected: "table".to_string(),
        }),
    }
}

fn read_dotted_value<'a>(table: &'a toml::value::Table, parts: &[&str]) -> Option<&'a toml::Value> {
    let (head, tail) = parts.split_first()?;
    let value = table.get(*head)?;
    if tail.is_empty() {
        return Some(value);
    }
    read_dotted_value(value.as_table()?, tail)
}

fn config_value_to_toml(value: &ConfigValue) -> toml::Value {
    match value {
        ConfigValue::String(v) => toml::Value::String(v.clone()),
        ConfigValue::Bool(v) => toml::Value::Boolean(*v),
        ConfigValue::Integer(v) => toml::Value::Integer(*v),
        ConfigValue::Float(v) => toml::Value::Float(*v),
        ConfigValue::List(values) => {
            toml::Value::Array(values.iter().map(config_value_to_toml).collect())
        }
        ConfigValue::Secret(secret) => config_value_to_toml(secret.expose()),
    }
}

#[cfg(unix)]
pub(crate) fn validate_secrets_permissions(
    path: &PathBuf,
    strict: bool,
) -> Result<(), ConfigError> {
    use std::os::unix::fs::PermissionsExt;

    if !strict {
        return Ok(());
    }

    let metadata = std::fs::metadata(path).map_err(|err| ConfigError::FileRead {
        path: path.display().to_string(),
        reason: err.to_string(),
    })?;
    let mode = metadata.permissions().mode() & 0o777;
    if mode & 0o077 != 0 {
        return Err(ConfigError::InsecureSecretsPermissions {
            path: path.display().to_string(),
            mode,
        });
    }

    Ok(())
}

#[cfg(not(unix))]
pub(crate) fn validate_secrets_permissions(
    _path: &PathBuf,
    _strict: bool,
) -> Result<(), ConfigError> {
    Ok(())
}

#[cfg(test)]
mod tests;
