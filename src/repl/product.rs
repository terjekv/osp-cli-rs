//! Product actions use editor suspension, never submitted command text.

/// Input dispatched by the editor to its host.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReplEvent<'a> {
    /// A user-submitted line, with ordinary history and shell semantics.
    Line(&'a str),
    /// A registered action, outside user history and the current shell scope.
    ///
    /// Print/continue resumes the draft. The built-in hint action may replace
    /// the draft; exit/restart ends the editor run.
    Action(&'a str),
}

/// Supported editor action keys.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReplActionKey {
    /// A function key F2 through F12.
    Function(u8),
    /// Alt+Enter, which inserts a suggested command without submitting it.
    AltEnter,
}

impl std::fmt::Display for ReplActionKey {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Function(key) => write!(f, "F{key}"),
            Self::AltEnter => f.write_str("Alt+Enter"),
        }
    }
}

/// One named action, bound to a supported key or disabled.
#[derive(Debug, Clone)]
pub struct ReplAction {
    /// Stable semantic ID, also the suffix of `repl.bindings.<id>`.
    pub id: String,
    /// Human label used by help and resolved binding tips.
    pub label: String,
    /// Editor key; `None` disables the shortcut.
    pub key: Option<ReplActionKey>,
}

impl ReplAction {
    /// Resolves a supported editor key from config, or uses `default_key`.
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
            Some(key) if key == "alt+enter" => Some(ReplActionKey::AltEnter),
            Some(key) => Some(ReplActionKey::Function(
                key.strip_prefix('f')
                    .and_then(|key| key.parse::<u8>().ok())
                    .filter(|key| (2..=12).contains(key))
                    .ok_or_else(|| {
                        anyhow::anyhow!("repl.bindings.{id}: use F2..F12, Alt+Enter or none")
                    })?,
            )),
            None => Some(ReplActionKey::Function(default_key)),
        };
        anyhow::ensure!(
            key.is_none_or(|key| match key {
                ReplActionKey::Function(key) => (2..=12).contains(&key),
                ReplActionKey::AltEnter => true,
            }),
            "unsupported action key"
        );
        Ok(Self {
            id: id.to_string(),
            label: label.to_string(),
            key,
        })
    }
}

/// Structured command feedback shared by native commands and their REPL host.
/// Hints retain root-form commands so changing command shells stays safe.
#[derive(Debug, Clone, Default)]
pub struct ReplFeedback(std::sync::Arc<std::sync::Mutex<Feedback>>);

#[derive(Debug, Default)]
struct Feedback {
    hint: Option<String>,
    history: Vec<String>,
}

impl ReplFeedback {
    /// Remembers a root-form suggested command when its hint is produced.
    pub fn remember_hint(&self, command: impl Into<String>) {
        self.0
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .hint = Some(command.into());
    }

    /// Queues a root-form command for the host's ordinary history policy.
    /// This does not execute it, and disabled history remains disabled.
    pub fn remember_command(&self, command: impl Into<String>) {
        self.0
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .history
            .push(command.into());
    }

    pub(crate) fn hint(&self) -> Option<String> {
        self.0
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .hint
            .clone()
    }

    pub(crate) fn take_history(&self) -> Vec<String> {
        std::mem::take(
            &mut self
                .0
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .history,
        )
    }
}

/// Product status text with semantic emphasis; terminal control sequences are stripped.
#[derive(Debug, Clone)]
pub struct ReplStatus {
    /// Human status text, sanitized by the host before rendering.
    pub text: String,
    /// Error and warning use the active theme; other levels use ordinary text.
    pub level: crate::core::MessageLevel,
}

pub(crate) type ReplStatusRenderer = std::sync::Arc<dyn Fn() -> Option<ReplStatus> + Send + Sync>;
