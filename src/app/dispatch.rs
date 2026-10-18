//! Top-level command dispatch planning and source selection.
//!
//! This module takes parsed CLI intent and turns it into the coarse action the
//! host should run: builtin command, REPL, or external command surface. It
//! also owns the early native-vs-plugin source choice so later execution paths
//! do not have to repeat visibility and provider-selection logic.

use miette::{Result, miette};

use crate::app::{AppClients, AuthState, TerminalKind};
use crate::cli::{Cli, Commands, ConfigCommands};
use crate::core::command_policy::{AccessReason, CommandAccess, CommandPath};
use crate::normalize::normalize_optional_identifier;
use crate::plugin::CommandCatalogEntry;

pub(crate) enum RunAction {
    Repl,
    ProductAction(String),
    Builtin(Commands),
    External(Vec<String>),
}

impl std::fmt::Debug for RunAction {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(self.name())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ExternalCommandSource {
    Native,
    Plugin,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ExternalPathAccessRequirement {
    Visible,
    Runnable,
}

impl RunAction {
    pub(crate) fn name(&self) -> &'static str {
        match self {
            RunAction::Repl => "repl",
            RunAction::ProductAction(_) => "product action",
            RunAction::Builtin(_) => "builtin",
            RunAction::External(_) => "external",
        }
    }

    pub(crate) fn uses_raw_config(&self) -> bool {
        match self {
            RunAction::Builtin(Commands::Doctor(_)) => true,
            RunAction::Builtin(Commands::Config(args)) => match &args.command {
                ConfigCommands::Unset(_) | ConfigCommands::Doctor => true,
                ConfigCommands::Show(args) => args.output.raw,
                _ => false,
            },
            _ => false,
        }
    }

    pub(crate) fn terminal_kind(&self) -> TerminalKind {
        match self {
            RunAction::Repl | RunAction::Builtin(Commands::Repl(_) | Commands::Intro(_)) => {
                TerminalKind::Repl
            }
            RunAction::Builtin(_) | RunAction::External(_) | RunAction::ProductAction(_) => {
                TerminalKind::Cli
            }
        }
    }
}

pub(crate) struct DispatchPlan {
    pub(crate) action: RunAction,
    pub(crate) profile_override: Option<String>,
}

impl DispatchPlan {
    fn new(action: RunAction, profile_override: Option<String>) -> Self {
        Self {
            action,
            profile_override,
        }
    }

    fn repl(profile_override: Option<String>) -> Self {
        Self::new(RunAction::Repl, profile_override)
    }
}

pub(crate) fn build_dispatch_plan(cli: &mut Cli) -> Result<DispatchPlan> {
    let explicit_profile = normalize_cli_profile(cli);
    if let Some(action) = cli.product_action.take() {
        return Ok(DispatchPlan::new(
            RunAction::ProductAction(action),
            explicit_profile,
        ));
    }
    let command = cli.command.take();

    if let Some(command) = &command {
        command.validate_dispatch_surface(false)?;
    }
    match command {
        None => Ok(DispatchPlan::repl(explicit_profile)),
        Some(Commands::External(tokens)) => Ok(DispatchPlan::new(
            RunAction::External(tokens),
            explicit_profile,
        )),
        Some(command) => Ok(DispatchPlan::new(
            RunAction::Builtin(command),
            explicit_profile,
        )),
    }
}

pub(crate) fn normalize_cli_profile(cli: &mut Cli) -> Option<String> {
    let normalized = normalize_profile_override(cli.profile.clone());
    cli.profile = normalized.clone();
    normalized
}

/// Checks visibility before invoking the product for an execution attempt.
/// The final runnable check follows any native metadata refresh.
pub(crate) fn before_external_dispatch(
    runtime: &mut crate::app::AppRuntime,
    path: &CommandPath,
    kind: &'static str,
) -> Result<()> {
    ensure_external_access(runtime, path, ExternalPathAccessRequirement::Visible, kind)?;
    if let Some(policy) = runtime.access_policy() {
        policy.before_dispatch(path.as_slice(), runtime)?;
    }
    Ok(())
}

pub(crate) fn before_external_execution(
    runtime: &mut crate::app::AppRuntime,
    path: &CommandPath,
    requirement: ExternalPathAccessRequirement,
    kind: &'static str,
) -> Result<()> {
    ensure_external_access(runtime, path, ExternalPathAccessRequirement::Visible, kind)?;
    if requirement == ExternalPathAccessRequirement::Runnable
        && let Some(policy) = runtime.access_policy()
    {
        policy.before_run(path.as_slice(), runtime)?;
    }
    Ok(())
}

pub(crate) fn before_external_run(
    runtime: &mut crate::app::AppRuntime,
    path: &CommandPath,
    requirement: ExternalPathAccessRequirement,
    kind: &'static str,
) -> Result<()> {
    let diagnostics = crate::diagnostics::DiagnosticSink::default();
    let _scope = diagnostics.enter();
    before_external_dispatch(runtime, path, kind)
        .and_then(|()| before_external_execution(runtime, path, requirement, kind))
        .map_err(|error| super::host::report_with_evidence(error, diagnostics))
}

pub(crate) fn ensure_external_access(
    runtime: &crate::app::AppRuntime,
    path: &CommandPath,
    requirement: ExternalPathAccessRequirement,
    kind: &'static str,
) -> Result<()> {
    check_access(
        path,
        kind,
        runtime.auth().external_command_path_access(path),
        requirement,
    )
}

pub(crate) fn check_access(
    path: &CommandPath,
    kind: &'static str,
    access: CommandAccess,
    requirement: ExternalPathAccessRequirement,
) -> Result<()> {
    if match requirement {
        ExternalPathAccessRequirement::Visible => access.is_visible(),
        ExternalPathAccessRequirement::Runnable => access.is_runnable(),
    } {
        return Ok(());
    }
    let reason = access.reason();
    if let Some(AccessReason::HiddenByProduct(Some(message))) = reason {
        return Err(miette!("{message}"));
    }
    let (detail, action) = match reason {
        Some(AccessReason::Unauthenticated) => (
            "requires authentication".to_string(),
            "authenticate, then retry".to_string(),
        ),
        Some(AccessReason::MissingCredential(service)) => (
            format!("requires credential `{service}`"),
            format!("configure the `{service}` credential, then retry"),
        ),
        Some(AccessReason::InvalidCredential(service)) => (
            format!("requires a valid `{service}` credential"),
            format!("renew the `{service}` credential, then retry"),
        ),
        _ => (
            "is hidden by current auth policy".to_string(),
            "ask the command owner to enable access, then retry".to_string(),
        ),
    };
    let command = path.as_slice().join(" ");
    Err(miette::Report::new(
        crate::core::CommandError::new(format!("{kind} {command} {detail}.")).with_action(action),
    ))
}

pub(crate) fn resolve_external_command_source(
    auth: &AuthState,
    clients: &AppClients,
    command: &str,
    scope: &crate::core::CommandScope,
) -> Result<ExternalCommandSource> {
    check_access(
        &CommandPath::new([command]),
        "command",
        auth.external_command_access(command),
        ExternalPathAccessRequirement::Visible,
    )?;
    if clients.native_commands().command(command).is_some() {
        return Ok(ExternalCommandSource::Native);
    }
    let catalog = invocation_command_catalog(auth, clients, command)?;
    let matching = matching_external_command_entries(&catalog, command);
    let has_native = matching.iter().any(|entry| is_native_command_entry(entry));

    if matching.is_empty() {
        return Err(match closest_external_command(&catalog, command) {
            Some(suggestion) => miette::Report::new(
                crate::core::CommandError::new(format!("Unknown command '{command}'."))
                    .with_action(format!("Did you mean {}?", scope.command(&suggestion))),
            ),
            None => miette::Report::new(scope.run_hint(
                format!("Unknown command '{command}'."),
                "help",
                " to list commands.",
            )),
        });
    }

    if has_native {
        Ok(ExternalCommandSource::Native)
    } else {
        Ok(ExternalCommandSource::Plugin)
    }
}

pub(crate) fn canonical_external_command_name(
    auth: &AuthState,
    clients: &AppClients,
    command: &str,
) -> Result<String> {
    if let Some((name, _)) = clients
        .native_commands()
        .iter()
        .find(|(name, _)| name.eq_ignore_ascii_case(command.trim()))
        && auth.is_external_command_visible(name)
    {
        return Ok(name.to_string());
    }
    let catalog = invocation_command_catalog(auth, clients, command)?;
    Ok(matching_external_command_entries(&catalog, command)
        .first()
        .map(|entry| entry.name.clone())
        .unwrap_or_else(|| command.to_string()))
}

/// The authorized catalog for invoking `command`. When nothing matches
/// passively, PATH plugins found only by execution-aware discovery are added.
fn invocation_command_catalog(
    auth: &AuthState,
    clients: &AppClients,
    command: &str,
) -> Result<Vec<CommandCatalogEntry>> {
    let _ = command;
    Ok(super::authorized_command_catalog_for(
        auth,
        clients,
        clients.native_commands().catalog(),
    ))
}

fn is_native_command_entry(entry: &CommandCatalogEntry) -> bool {
    matches!(entry.source, crate::plugin::CommandSource::Native)
}

fn matching_external_command_entries<'a>(
    catalog: &'a [CommandCatalogEntry],
    command: &str,
) -> Vec<&'a CommandCatalogEntry> {
    catalog
        .iter()
        .filter(|entry| entry.name.eq_ignore_ascii_case(command))
        .collect()
}

fn closest_external_command(catalog: &[CommandCatalogEntry], command: &str) -> Option<String> {
    let command = command.trim().to_ascii_lowercase();
    let names = catalog
        .iter()
        .map(|entry| entry.name.as_str())
        .collect::<Vec<_>>();
    crate::command_words::suggestions(&command, &names)
        .into_iter()
        .next()
}

pub(crate) fn normalize_profile_override(value: Option<String>) -> Option<String> {
    normalize_optional_identifier(value)
}

#[cfg(test)]
mod tests {
    use super::{
        DispatchPlan, RunAction, build_dispatch_plan, normalize_cli_profile,
        normalize_profile_override,
    };
    use crate::cli::{Cli, Commands};
    use clap::Parser;
    fn parse_cli(args: &[&str]) -> Cli {
        Cli::try_parse_from(args).unwrap()
    }
    #[test]
    fn normalize_profile_helpers_trim_blank_values_and_rewrite_cli_unit() {
        assert_eq!(
            normalize_profile_override(Some("  Dev  ".to_string())),
            Some("dev".to_string())
        );
        assert_eq!(normalize_profile_override(Some("   ".to_string())), None);
        assert_eq!(normalize_profile_override(None), None);
        let mut cli = parse_cli(&["osp", "--profile", "  DEV  ", "theme", "list"]);

        let normalized = normalize_cli_profile(&mut cli);

        assert_eq!(normalized.as_deref(), Some("dev"));
        assert_eq!(cli.profile.as_deref(), Some("dev"));
    }

    #[test]
    fn build_dispatch_plan_routes_profiles_builtins_external_and_errors_unit() {
        let mut repl_cli = parse_cli(&["osp", "--profile", "dev"]);
        let DispatchPlan {
            action,
            profile_override,
        } = build_dispatch_plan(&mut repl_cli).expect("explicit profile repl should work");
        assert!(matches!(action, RunAction::Repl));
        assert_eq!(profile_override.as_deref(), Some("dev"));

        let mut config_cli = parse_cli(&["osp", "--profile", "dev", "config", "show"]);
        let DispatchPlan {
            action,
            profile_override,
        } = build_dispatch_plan(&mut config_cli)
            .expect("explicit profile config command should work");
        assert!(matches!(action, RunAction::Builtin(Commands::Config(_))));
        assert_eq!(profile_override.as_deref(), Some("dev"));

        let mut explicit_profile_cli =
            parse_cli(&["osp", "--profile", "prod", "dev", "config", "show"]);
        let DispatchPlan {
            action,
            profile_override,
        } = build_dispatch_plan(&mut explicit_profile_cli)
            .expect("explicit profile should leave command tokens untouched");
        assert!(
            matches!(action, RunAction::External(tokens) if tokens == vec!["dev", "config", "show"])
        );
        assert_eq!(profile_override.as_deref(), Some("prod"));

        let mut unknown_profile_cli = parse_cli(&["osp", "stage", "config", "show"]);
        let DispatchPlan {
            action,
            profile_override,
        } = build_dispatch_plan(&mut unknown_profile_cli)
            .expect("unknown prefix should stay external");
        assert!(
            matches!(action, RunAction::External(tokens) if tokens == vec!["stage", "config", "show"])
        );
        assert!(profile_override.is_none());

        let mut repl_cli = parse_cli(&["osp", "repl", "debug-complete", "--line", "ldap"]);
        let DispatchPlan {
            action,
            profile_override,
        } = build_dispatch_plan(&mut repl_cli).expect("repl subcommand should parse");
        assert!(matches!(action, RunAction::Builtin(Commands::Repl(_))));
        assert!(profile_override.is_none());

        let mut bad_shorthand_cli = parse_cli(&["osp", "dev", "config", "set", "ui.format"]);
        let plan = build_dispatch_plan(&mut bad_shorthand_cli)
            .expect("command arguments belong to the external command");
        assert!(matches!(plan.action, RunAction::External(tokens)
            if tokens == vec!["dev", "config", "set", "ui.format"]));
        assert!(plan.profile_override.is_none());
    }
}
