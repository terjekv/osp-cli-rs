//! `context` command for values remembered in the active REPL session.

use crate::app::{AppSession, CommandOutput};
use crate::cli::rows::output::rows_to_output_result;
use crate::cli::{ContextArgs, ContextCommands};
use crate::core::row::Row;
use miette::Result;
use serde_json::Value;

pub(crate) fn run_context_command(
    session: &AppSession,
    runtime: &crate::app::AppRuntime,
    args: ContextArgs,
) -> Result<CommandOutput> {
    session.native_context.activate(runtime);
    match args.command {
        Some(ContextCommands::Clear(args)) => {
            session.native_context.clear_context(args.kind.as_deref())
        }
        Some(ContextCommands::Set(args)) => {
            if !crate::context::valid_value(&args.value) {
                return Err(miette::miette!(
                    "context value must be 1–256 printable characters"
                ));
            }
            if !crate::context::KINDS.contains(&args.kind.as_str()) {
                return Err(miette::miette!("unknown context kind: {}", args.kind));
            }
            let service = session
                .native_context
                .binding
                .read()
                .unwrap_or_else(|e| e.into_inner())
                .owners
                .get(&args.kind)
                .cloned()
                .unwrap_or_default();
            session
                .native_context
                .for_service(service)
                .set_context_value(args.kind, args.value);
        }
        None => {}
    }
    session
        .native_context
        .save_context()
        .map_err(|e| miette::miette!("{e:#}"))?;

    let rows = session
        .native_context
        .context_entries()
        .into_iter()
        .map(|entry| {
            let mut row = Row::new();
            row.insert("kind".to_string(), Value::String(entry.kind));
            row.insert("value".to_string(), Value::String(entry.value));
            row.insert("service".into(), Value::String(entry.service));
            row.insert(
                "scope".into(),
                Value::String(format!("{}@{}", entry.identity, entry.profile)),
            );
            row.insert("used_by".into(), Value::String(entry.used_by));
            row.insert("command".to_string(), Value::String(entry.command));
            let age = chrono::DateTime::parse_from_rfc3339(&entry.when)
                .map(|when| {
                    format!(
                        "{}s",
                        (chrono::Utc::now() - when.with_timezone(&chrono::Utc)).num_seconds()
                    )
                })
                .unwrap_or_default();
            row.insert("age".to_string(), Value::String(age));
            row
        })
        .collect();

    Ok(CommandOutput::output(rows_to_output_result(rows), None))
}
