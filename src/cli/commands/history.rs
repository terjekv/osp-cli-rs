//! `history` builtin command wrapper.
//!
//! The real history behavior lives in the REPL subsystem. This module adapts
//! its results into ordinary command results for both interactive and one-shot
//! callers.

use miette::Result;

use crate::app::AppSession;
use crate::app::CommandOutput;
use crate::cli::HistoryArgs;
use crate::repl::history as repl_history;

pub(crate) fn run_history_repl_command(
    session: &mut AppSession,
    args: HistoryArgs,
    history: &crate::repl::SharedHistory,
) -> Result<CommandOutput> {
    repl_history::run_history_repl_command(session, args, history)
}

#[cfg(test)]
#[cfg_attr(coverage_nightly, coverage(off))]
mod tests {
    use super::run_history_repl_command;
    use crate::app::AppSession;
    use crate::app::CommandOutput;
    use crate::cli::{HistoryArgs, HistoryCommands, HistoryListArgs};
    use crate::core::row::Row;
    use crate::repl::HistoryConfig;

    fn extract_output_rows(result: CommandOutput) -> Option<Vec<Row>> {
        match result.body {
            crate::app::CommandBody::Output(output) => output.into_rows(),
            _ => None,
        }
    }

    #[test]
    fn history_output_helpers_wrap_rows_and_ignore_text_results_unit() {
        let history = crate::repl::SharedHistory::new(
            HistoryConfig {
                legacy_path: None,
                path: None,
                max_entries: 32,
                enabled: true,
                dedupe: true,
                exclude_patterns: Vec::new(),
                shell_context: crate::repl::HistoryShellContext::default(),
            }
            .normalized(),
        );
        history
            .with(|store| store.save_command_line("config show"))
            .expect("history lock should be available")
            .expect("history seed should save");

        let mut session = AppSession::default();
        let result = run_history_repl_command(
            &mut session,
            HistoryArgs {
                command: HistoryCommands::List(HistoryListArgs::default()),
            },
            &history,
        )
        .expect("history list should succeed");

        assert_eq!(result.exit_code, 0);
        let rows = extract_output_rows(result).expect("history list should emit rows");
        assert_eq!(rows.len(), 1);
        assert_eq!(
            rows[0]["command"],
            serde_json::Value::String("config show".into())
        );
        assert!(extract_output_rows(CommandOutput::text("hello")).is_none());
    }
}
