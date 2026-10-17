//! REPL hooks for native commands hosted by [`crate::App`].
//!
//! Products provide [`crate::repl::ReplStatus`] and [`crate::repl::ReplAction`] through
//! [`crate::NativeCommand::repl_status`] and
//! [`crate::NativeCommand::repl_actions`]. [`crate::repl::ReplSession`] tracks the editor
//! lifetime, [`crate::repl::post_notice`] delivers background notices, and [`crate::repl::ReplFeedback`]
//! carries command feedback. The host owns editor configuration, history,
//! shell dispatch and debug views.

mod bell;
pub(crate) mod completion;
pub(crate) mod dispatch;
mod engine;
pub(crate) mod help;
mod highlight;
pub(crate) mod history;
mod history_store;
mod host;
pub(crate) mod input;
pub(crate) mod lifecycle;
mod menu;
mod menu_core;
pub(crate) mod notices;
pub(crate) mod presentation;
mod product;
pub(crate) mod surface;

#[cfg(test)]
pub(crate) use dispatch::apply_repl_shell_prefix;
#[cfg(test)]
pub(crate) use engine::ReplCompleter;
// Primary host-facing entry points.
pub(crate) use engine::{
    HistoryConfig, HistoryEntry, HistoryShellContext, LineProjection, LineProjector,
    PromptRightRenderer, ReplAppearance, ReplInputMode, ReplPrompt, ReplReloadKind, ReplResult,
    ReplRunConfig, ReplTabMode, SharedHistory, color_from_style_spec, default_pipe_verbs, run_repl,
};
pub use notices::{ReplSession, post_notice};
pub(crate) use product::ReplEvent;
pub use product::{ReplAction, ReplActionKey, ReplFeedback, ReplStatus};
// Debug surfaces for REPL completion, highlight, and history inspection.
pub(crate) use engine::{
    CompletionDebugOptions, DebugStep, debug_completion, debug_completion_steps, debug_highlight,
};
pub(crate) use engine::{
    CompletionTraceEvent, CompletionTraceMenuState, trace_completion, trace_completion_enabled,
};
pub(crate) use host::{ReplViewContext, run_plugin_repl, run_repl_debug_command_for};
#[cfg(test)]
pub(crate) use input::ReplParsedLine;
pub(crate) use input::is_repl_shellable_command;
#[cfg(test)]
pub(crate) use presentation::render_repl_prompt_right_for_test;

#[cfg(test)]
pub(crate) use crate::app::command_plan::repl_command_spec;

#[cfg(test)]
mod tests {
    #[test]
    fn repl_internal_example_1() {
        let _example = || {
            use crate::repl::{HistoryConfig, ReplPrompt, ReplResult, ReplRunConfig, run_repl};
            use anyhow::Result;

            let config =
                ReplRunConfig::builder(ReplPrompt::simple("osp> "), HistoryConfig::default())
                    .build();

            let _result = run_repl(config, |line, _history| -> Result<ReplResult> {
                match line {
                    crate::repl::ReplEvent::Eof => Ok(ReplResult::Exit(0)),
                    crate::repl::ReplEvent::Line(line) | crate::repl::ReplEvent::RootLine(line) => {
                        match line.trim() {
                            "exit" | "quit" => Ok(ReplResult::Exit(0)),
                            _ => Ok(ReplResult::Continue),
                        }
                    }
                    crate::repl::ReplEvent::Action(_) => Ok(ReplResult::Continue),
                }
            })?;
            Ok::<(), anyhow::Error>(())
        };
    }
}
