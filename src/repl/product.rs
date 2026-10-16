//! Product actions use editor suspension, never submitted command text.

/// Input dispatched by the editor to its host.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReplEvent<'a> {
    /// A user-submitted line, with ordinary history and shell semantics.
    Line(&'a str),
    /// A registered action, outside user history and the current shell scope.
    ///
    /// Print/continue resumes the draft. Draft replacement is reserved for line
    /// submissions; exit/restart ends the editor run.
    Action(&'a str),
}

/// One named product action. Keys are function keys F2 through F12 or disabled.
#[derive(Debug, Clone)]
pub struct ReplAction {
    /// Stable semantic ID, also the suffix of `repl.bindings.<id>`.
    pub id: String,
    /// Human label used by help and resolved binding tips.
    pub label: String,
    /// Function-key number; `None` disables the shortcut.
    pub key: Option<u8>,
}

impl ReplAction {
    /// Resolves a supported function key from config, or uses `default_key`.
    ///
    /// Rejects executable strings and unsupported keys. The editor additionally
    /// rejects collisions with other actions and its existing keybindings.
    pub fn resolve(
        id: &str,
        label: &str,
        default_key: u8,
        config: &crate::config::ResolvedConfig,
    ) -> anyhow::Result<Self> {
        let key = config.get_string(&format!("repl.bindings.{id}"));
        let key = match key.map(|key| key.trim().to_ascii_lowercase()) {
            Some(key) if key == "none" => None,
            Some(key) => Some(
                key.strip_prefix('f')
                    .and_then(|key| key.parse::<u8>().ok())
                    .filter(|key| (2..=12).contains(key))
                    .ok_or_else(|| anyhow::anyhow!("repl.bindings.{id}: use F2..F12 or none"))?,
            ),
            None => Some(default_key),
        };
        anyhow::ensure!(
            key.is_none_or(|key| (2..=12).contains(&key)),
            "unsupported action key"
        );
        Ok(Self {
            id: id.to_string(),
            label: label.to_string(),
            key,
        })
    }
}
