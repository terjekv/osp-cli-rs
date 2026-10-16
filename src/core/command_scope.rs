//! Command suggestions written for the reader's current prompt.

use std::collections::BTreeSet;

/// Where the reader types the next command.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub enum CommandScope {
    /// A terminal shell running a one-shot invocation.
    #[default]
    OneShot,
    /// The REPL prompt outside any command shell.
    ReplRoot,
    /// A REPL command shell.
    ReplShell {
        /// The shell's root command.
        root: String,
        /// Host commands and aliases resolved before shell scope.
        root_words: BTreeSet<String>,
    },
}

impl CommandScope {
    /// Renders a root-form command for this prompt, accepting a leading `osp `.
    ///
    /// ```
    /// use osp_cli::core::CommandScope;
    /// let orch = CommandScope::ReplShell {
    ///     root: "orch".into(),
    ///     root_words: ["context".to_string()].into(),
    /// };
    /// assert_eq!(CommandScope::OneShot.command("orch task wait 42"), "osp orch task wait 42");
    /// assert_eq!(CommandScope::ReplRoot.command("orch task wait 42"), "orch task wait 42");
    /// assert_eq!(orch.command("orch task wait 42"), "task wait 42");
    /// assert_eq!(orch.command("sudo orch task approve 42"), "sudo task approve 42");
    /// assert_eq!(orch.command("orch context nrec:local"), "orch context nrec:local");
    /// assert_eq!(orch.command("config set ui.format json"), "config set ui.format json");
    /// assert_eq!(orch.command("osp orch task wait 42"), "task wait 42");
    /// ```
    pub fn command(&self, line: &str) -> String {
        let line = line.trim();
        let line = line.strip_prefix("osp ").unwrap_or(line).trim();
        match self {
            Self::OneShot => format!("osp {line}").trim_end().to_string(),
            Self::ReplRoot => line.to_string(),
            Self::ReplShell { root, root_words } => {
                let (sudo, command) = line
                    .strip_prefix("sudo ")
                    .map_or(("", line), |rest| ("sudo ", rest.trim_start()));
                let (first, rest) = command
                    .split_once(char::is_whitespace)
                    .unwrap_or((command, ""));
                let rest = rest.trim_start();
                if first == root
                    && !root_words.contains(rest.split_whitespace().next().unwrap_or(""))
                {
                    format!("{sudo}{rest}").trim_end().to_string()
                } else {
                    line.to_string()
                }
            }
        }
    }
}
