//! `context` command for values remembered in the active REPL session.

use crate::app::{AppSession, CommandOutput};
use crate::cli::rows::output::rows_to_output_result;
use crate::cli::{ContextArgs, ContextCommands};
use crate::core::row::Row;
use miette::Result;
use serde_json::Value;

pub(crate) fn run_context_command(
    session: &AppSession,
    args: ContextArgs,
) -> Result<CommandOutput> {
    if let Some(ContextCommands::Clear(args)) = args.command {
        session.native_context.clear_context(args.kind.as_deref());
    }

    let rows = session
        .native_context
        .context_entries()
        .into_iter()
        .map(|entry| {
            let mut row = Row::new();
            row.insert("kind".to_string(), Value::String(entry.kind));
            row.insert("value".to_string(), Value::String(entry.value));
            row.insert("command".to_string(), Value::String(entry.command));
            row.insert("when".to_string(), Value::String(entry.when));
            row
        })
        .collect();

    Ok(CommandOutput::output(rows_to_output_result(rows), None))
}
