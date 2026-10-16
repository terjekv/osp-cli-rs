//! Product-owned visibility, startup and session acquisition.

use super::AppRuntime;
use crate::core::command_policy::SessionFacts;

/// Product verdict about offering an external command path.
pub enum Visibility {
    /// Offer the command subject to its declared authentication requirements.
    Shown,
    /// Hide the command everywhere, using the optional message on dispatch.
    Hidden(Option<String>),
}

/// Product access decisions shared by native commands and external plugins.
///
/// Visibility and session facts must be side-effect free: help, completion,
/// the prompt and doctor read them without acquiring credentials.
pub trait AccessPolicy: Send + Sync {
    /// Refreshes side-effect-free product state from resolved configuration.
    ///
    /// Called only by the shared `AuthState` policy-attachment path, before
    /// visibility is read in root help, runtime startup or REPL rebuilds.
    /// Root help has resolved config but no runtime: it cannot call `startup`,
    /// which may acquire credentials, and `visibility` has no config argument.
    /// Implementations must not acquire credentials or perform network I/O.
    fn configure(&self, _config: &crate::config::ResolvedConfig) {}

    /// Whether the canonical command path is offered to the current identity.
    fn visibility(&self, _path: &[String]) -> Visibility {
        Visibility::Shown
    }

    /// Runs once after startup state assembly, never on REPL rebuilds.
    fn startup(&self, _runtime: &mut AppRuntime) -> miette::Result<()> {
        Ok(())
    }

    /// Runs before external metadata, help, execution, or REPL scope entry.
    ///
    /// Native invocations supply the canonical root followed by input tokens,
    /// before parsing; plugins and scope entry supply the known command path.
    /// Implementations must not log tokens, which may contain sensitive input.
    /// This hook handles explicit user invocations only, never root help,
    /// completion, or background refresh. Products may acquire credentials here
    /// when their interaction policy permits it. An error cancels dispatch.
    fn before_dispatch(&self, _path: &[String], _runtime: &mut AppRuntime) -> miette::Result<()> {
        Ok(())
    }

    /// Runs once before an external execution and any metadata refresh.
    ///
    /// The path is canonical when the cached grammar resolves it. If dynamic
    /// grammar must be refreshed first, the path is the known command prefix.
    /// Builtins, help and completion never call this hook.
    fn before_run(&self, _path: &[String], _runtime: &mut AppRuntime) -> miette::Result<()> {
        Ok(())
    }

    /// Returns current session and credential facts for access, doctor and prompts.
    fn session_facts(&self) -> SessionFacts {
        SessionFacts::default()
    }
}
