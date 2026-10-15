//! Shared inline parsing and command pipeline policy for CLI and REPL hosts.

use std::borrow::Cow;

use miette::{Result, miette};

use super::{
    CMD_CONFIG, CMD_CONTEXT, CMD_DOCTOR, CMD_HISTORY, CMD_INTRO, CMD_PLUGINS, ReplCommandSpec,
};
use crate::cli::{
    Commands, ConfigCommands, ContextCommands, DoctorCommands, HistoryCommands, PluginsCommands,
    ThemeCommands, parse_inline_command_tokens,
};

#[derive(Clone, Copy)]
pub(crate) enum CommandSurface {
    Repl,
    External,
}

pub(crate) enum InlineCommandPlan {
    Command(Option<Commands>),
    Help(clap::Error),
}

pub(crate) fn parse_inline_command(
    tokens: &[String],
    surface: CommandSurface,
) -> Result<InlineCommandPlan> {
    match parse_inline_command_tokens(tokens) {
        Ok(command) => Ok(InlineCommandPlan::Command(command)),
        Err(error) => {
            let help = matches!(
                error.kind(),
                clap::error::ErrorKind::DisplayHelp
                    | clap::error::ErrorKind::DisplayVersion
                    | clap::error::ErrorKind::DisplayHelpOnMissingArgumentOrSubcommand
            );
            // Help is a document; callers apply any stages to it once.
            if help {
                return Ok(InlineCommandPlan::Help(error));
            }
            Err(super::report_std_error_with_context(
                error,
                match surface {
                    CommandSurface::Repl => "failed to parse REPL command",
                    CommandSurface::External => "failed to parse external inline command",
                },
            ))
        }
    }
}

pub(crate) fn validate_command_pipeline(command: &Commands, stages: &[String]) -> Result<()> {
    let spec = repl_command_spec(command);
    if stages.is_empty() || spec.supports_dsl {
        Ok(())
    } else {
        Err(miette!(
            "`{}` does not support DSL pipeline stages",
            spec.name
        ))
    }
}

pub(crate) fn repl_command_spec(command: &Commands) -> ReplCommandSpec {
    let (name, supports_dsl) = match command {
        Commands::Context(args) => (
            CMD_CONTEXT,
            matches!(args.command, None | Some(ContextCommands::Clear(_))),
        ),
        Commands::External(tokens) => {
            return ReplCommandSpec {
                name: Cow::Owned(
                    tokens
                        .first()
                        .cloned()
                        .unwrap_or_else(|| "external".to_string()),
                ),
                supports_dsl: true,
            };
        }
        Commands::Plugins(args) => (
            CMD_PLUGINS,
            matches!(
                args.command,
                PluginsCommands::List
                    | PluginsCommands::Commands
                    | PluginsCommands::Doctor
                    | PluginsCommands::Config(_)
            ),
        ),
        Commands::Doctor(args) => (
            CMD_DOCTOR,
            matches!(
                args.command,
                Some(DoctorCommands::Config)
                    | Some(DoctorCommands::Plugins)
                    | Some(DoctorCommands::Theme)
            ),
        ),
        Commands::History(args) => (
            CMD_HISTORY,
            matches!(args.command, HistoryCommands::List(_)),
        ),
        Commands::Completions(_) => ("completions", false),
        Commands::Config(args) => (
            CMD_CONFIG,
            matches!(
                args.command,
                ConfigCommands::Show(_) | ConfigCommands::Get(_) | ConfigCommands::Doctor
            ),
        ),
        Commands::Alias(args) => (
            "alias",
            matches!(args.command, crate::cli::AliasCommands::List(_)),
        ),
        Commands::Theme(args) => (
            "theme",
            matches!(args.command, ThemeCommands::List | ThemeCommands::Show(_)),
        ),
        Commands::Intro(_) => (CMD_INTRO, true),
        Commands::Repl(_) => ("repl", false),
    };
    ReplCommandSpec {
        name: Cow::Borrowed(name),
        supports_dsl,
    }
}
