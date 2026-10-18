//! Inline suggestion and status line painted after the REPL input.
//!
//! reedline paints a hinter's output right after the input line, so one
//! hinter carries both pieces: muted text that would complete the line
//! (accepted with Right or End, one word with Alt-Right) and, on the line
//! below, guidance for the command or argument at the cursor.
//! A unique command shows its canonical usage and description on one line.
//! While arguments are being entered, the full positional usage retains its
//! description above it.
//! reedline hides hints while a menu is open and when a line is submitted, so
//! neither reaches the scrollback.
//!
//! The hinter is the only editor hook that sees every painted line, so it also
//! records that line for the edit mode in [`PaintedLine`].

use nu_ansi_term::Style;
use reedline::{Completer, Hinter, History, SearchQuery, Suggestion};
use std::ops::Range;
use unicode_width::UnicodeWidthStr;

use super::adapter::ReplCompleter;
use super::editor::PaintedLine;
use crate::completion::{CompletionAnalysis, CompletionRequest};
use crate::repl::highlight::{LineProblemKind, ReplHighlighter};

/// The hint lines below the input.
struct Status {
    /// The command's description, on its own line above `text`.
    description: Option<String>,
    text: String,
    /// Byte ranges to emphasize after measuring and cropping plain text.
    bold: Vec<Range<usize>>,
}

impl Status {
    fn line(text: String) -> Self {
        Self {
            description: None,
            text,
            bold: Vec::new(),
        }
    }
}

pub(crate) struct ReplHinter {
    completer: ReplCompleter,
    highlighter: ReplHighlighter,
    painted: PaintedLine,
    // `None` turns the inline suggestion off; unstyled it would look typed.
    style: Option<Style>,
    command_limit: usize,
    inline: String,
}

impl ReplHinter {
    pub(crate) fn new(
        completer: ReplCompleter,
        highlighter: ReplHighlighter,
        painted: PaintedLine,
        style: Option<Style>,
    ) -> Self {
        Self {
            completer,
            highlighter,
            painted,
            style,
            command_limit: 3,
            inline: String::new(),
        }
    }

    /// The rest of the line the user most likely wants.
    ///
    /// The newest history line extending the input wins when it agrees with
    /// the command tree; otherwise the tree's only candidate for the current
    /// word does.
    fn inline_suggestion(
        &self,
        line: &str,
        history: &dyn History,
        candidates: &[Suggestion],
    ) -> String {
        let from_tree = only_extension(line, candidates);
        let from_history = history
            .search(SearchQuery::last_with_prefix(line.to_string(), None))
            .ok()
            .and_then(|items| items.into_iter().next())
            .and_then(|item| item.command_line.get(line.len()..).map(str::to_string))
            .filter(|rest| !rest.is_empty());
        match (from_history, from_tree) {
            (Some(past), Some(tree)) if past.starts_with(&tree) => past,
            (_, Some(tree)) => tree,
            (Some(past), None) => past,
            (None, None) => String::new(),
        }
    }

    /// One line about the cursor position, below the command's description
    /// while its positional usage is shown.
    fn status(&mut self, line: &str, pos: usize, candidates: &[Suggestion]) -> Option<Status> {
        if line.trim().is_empty() {
            return None;
        }
        if let Some(expanded) = self.completer.expand_bang_request(line) {
            return Some(Status::line(expanded));
        }
        if let Some(problem) = self.highlighter.problem(line, pos) {
            if let LineProblemKind::UnknownCommand { parent } = problem.kind {
                let node = self.completer.engine().node_at(&parent)?;
                let names = node
                    .children
                    .iter()
                    .filter(|(_, child)| !child.hidden && !child.value_key && !child.value_leaf)
                    .map(|(name, _)| name.as_str())
                    .collect::<Vec<_>>();
                let word = &line[problem.start..problem.end];
                let names = crate::command_words::suggestions(word, &names);
                let (last, rest) = names.split_last()?;
                let choices = if rest.is_empty() {
                    last.clone()
                } else {
                    format!("{} or {last}", rest.join(", "))
                };
                return Some(Status::line(format!("Did you mean {choices}?")));
            }
            return None;
        }
        let analysis = self.completer.analyze(line, pos);
        let engine = self.completer.engine();
        if let Some(names) = self.command_matches(&analysis) {
            if names.len() == 1 {
                let name = &names[0];
                let canonical = analysis
                    .context
                    .matched_path
                    .iter()
                    .map(String::as_str)
                    .chain(std::iter::once(name.as_str()))
                    .collect::<Vec<_>>()
                    .join(" ")
                    + " ";
                let selected = engine.analyze(&canonical, canonical.len());
                let path = &selected.context.matched_path;
                let node = engine.node_at(path)?;
                let (mut text, active) = engine
                    .positional_usage(&selected)
                    .unwrap_or_else(|| (path.join(" "), None));
                if let Some(description) = node
                    .tooltip
                    .as_deref()
                    .filter(|text| !text.trim().is_empty())
                {
                    text.push_str("  ");
                    text.push_str(description);
                }
                return Some(Status {
                    bold: active.into_iter().collect(),
                    ..Status::line(text)
                });
            }
            if names.len() < 2 || names.len() > self.command_limit {
                return None;
            }
            let mut status = Status::line(String::new());
            for name in names {
                if !status.text.is_empty() {
                    status.text.push_str("  ");
                }
                let start = status.text.len();
                status.text.push_str(&name);
                if self.completer.command_prefixes_enabled()
                    && let Some(len) = engine.command_prefix_len(&analysis, &name)
                {
                    status.bold.push(start..start + len);
                }
            }
            return Some(status);
        }
        if !matches!(analysis.request, CompletionRequest::FlagValues { .. })
            && let Some((usage, active)) = engine.positional_usage(&analysis)
        {
            let description = engine
                .node_at(&analysis.context.matched_path)
                .and_then(|node| node.tooltip.clone())
                .filter(|tooltip| !tooltip.trim().is_empty());
            return Some(Status {
                description,
                bold: active.into_iter().collect(),
                ..Status::line(usage)
            });
        }
        if !analysis.cursor.token_stub.is_empty()
            && let Some(candidate) = only_candidate(line, candidates)
            && let Some(description) = candidate.description.as_deref()
        {
            return Some(Status::line(format!("{}  {description}", candidate.value)));
        }
        if let CompletionRequest::FlagValues {
            flag_scope_path,
            flag,
        } = &analysis.request
        {
            let tooltip = engine
                .node_at(flag_scope_path)
                .and_then(|node| node.flags.get(flag))
                .and_then(|meta| meta.tooltip.as_deref())?;
            return Some(Status::line(format!("{flag}  {tooltip}")));
        }

        let path = &analysis.context.matched_path;
        if path.is_empty() {
            return None;
        }
        let mut status = path.join(" ");
        if let Some(tooltip) = engine
            .node_at(path)
            .and_then(|node| node.tooltip.as_deref())
        {
            status.push_str("  ");
            status.push_str(tooltip);
        }
        let missing = engine.missing_required_flags(&analysis);
        if !missing.is_empty() {
            status.push_str(" · needs ");
            status.push_str(&missing.join(", "));
        }
        Some(Status::line(status))
    }

    pub(crate) fn with_command_limit(mut self, limit: usize) -> Self {
        self.command_limit = limit;
        self
    }

    fn command_matches(&self, analysis: &CompletionAnalysis) -> Option<Vec<String>> {
        let CompletionRequest::Positionals {
            context_path,
            show_subcommands: true,
            ..
        } = &analysis.request
        else {
            return None;
        };
        let node = self.completer.engine().node_at(context_path)?;
        if node.value_key
            || node.value_leaf
            || !node
                .children
                .values()
                .any(|child| !child.value_key && !child.value_leaf)
        {
            return None;
        }
        if let Some(child) = node.children.get(&analysis.cursor.token_stub)
            && !child.value_key
            && !child.value_leaf
        {
            return Some(vec![analysis.cursor.token_stub.clone()]);
        }
        Some(
            node.children
                .iter()
                .filter(|(name, child)| {
                    !child.hidden
                        && !child.value_key
                        && !child.value_leaf
                        && name.starts_with(&analysis.cursor.token_stub)
                })
                .map(|(name, _)| name.clone())
                .collect(),
        )
    }
}

impl Hinter for ReplHinter {
    fn handle(
        &mut self,
        line: &str,
        pos: usize,
        history: &dyn History,
        use_ansi_coloring: bool,
        _cwd: &str,
    ) -> String {
        self.painted.set(line, pos);
        let candidates = if line.trim().is_empty() {
            Vec::new()
        } else {
            self.completer.complete(line, pos).suggestions().to_vec()
        };
        let analysis = self.completer.analyze(line, pos);
        let command_matches = self.command_matches(&analysis);
        let command_valid = command_matches
            .as_ref()
            .is_none_or(|names| names.len() == 1)
            && self.highlighter.problem(line, pos).is_none();
        let candidates = if let Some(names) = &command_matches {
            candidates
                .into_iter()
                .filter(|candidate| names.contains(&candidate.value))
                .collect::<Vec<_>>()
        } else {
            candidates
        };
        self.inline = if command_valid
            && use_ansi_coloring
            && self.style.is_some()
            && pos == line.len()
            && !line.trim().is_empty()
        {
            self.inline_suggestion(line, history, &candidates)
        } else {
            String::new()
        };

        let paint = |text: &str, style: Option<Style>| match style {
            Some(style) if use_ansi_coloring => style.paint(text).to_string(),
            _ => text.to_string(),
        };
        let mut out = paint(&self.inline, self.style);
        if let Some(Status {
            description,
            text: status,
            bold,
        }) = self.status(line, pos, &candidates)
        {
            let style = self.style;
            // Reedline's CRLF normalization drops the text after a leading LF.
            if let Some(description) = description {
                out.push_str("\r\n");
                out.push_str(&paint(&fit_terminal_width(&description), style));
            }
            out.push_str("\r\n");
            let status = fit_terminal_width(&status);
            let mut end = 0;
            for range in bold {
                let range = range.start..range.end.min(status.len());
                if range.start < end
                    || range.start >= range.end
                    || status.get(range.clone()).is_none()
                {
                    continue;
                }
                out.push_str(&paint(&status[end..range.start], style));
                out.push_str(&paint(
                    &status[range.clone()],
                    style.map(|style| style.bold()),
                ));
                end = range.end;
            }
            out.push_str(&paint(&status[end..], style));
        }
        out
    }

    fn complete_hint(&self) -> String {
        self.inline.clone()
    }

    fn next_hint_token(&self) -> String {
        let word = self.inline.trim_start();
        let lead = self.inline.len() - word.len();
        let end = word.find(char::is_whitespace).unwrap_or(word.len());
        self.inline[..lead + end].to_string()
    }
}

/// Candidates that extend the word before the cursor as typed.
fn extending<'a>(
    line: &'a str,
    candidates: &'a [Suggestion],
) -> impl Iterator<Item = &'a Suggestion> {
    candidates.iter().filter(move |candidate| {
        let typed = line
            .get(candidate.span.start..candidate.span.end)
            .unwrap_or_default();
        candidate.span.end == line.len()
            && candidate.value.len() > typed.len()
            && candidate.value.starts_with(typed)
    })
}

fn only_candidate<'a>(line: &'a str, candidates: &'a [Suggestion]) -> Option<&'a Suggestion> {
    let mut extending = extending(line, candidates);
    let first = extending.next()?;
    extending
        .all(|other| other.value == first.value)
        .then_some(first)
}

fn only_extension(line: &str, candidates: &[Suggestion]) -> Option<String> {
    let candidate = only_candidate(line, candidates)?;
    let typed = candidate.span.end - candidate.span.start;
    Some(candidate.value[typed..].to_string())
}

/// Keeps each hint line on one terminal row, marking a cut with `…`; a
/// wrapped line would push the prompt around as it changes length.
fn fit_terminal_width(text: &str) -> String {
    let width = terminal_size::terminal_size()
        .map(|(width, _)| usize::from(width.0))
        .unwrap_or(80)
        .saturating_sub(1);
    let text = text
        .chars()
        .map(|ch| if ch.is_control() { ' ' } else { ch })
        .collect::<String>();
    if text.width() <= width {
        return text;
    }
    let mut cut = crate::repl::menu_core::truncate_to_width(&text, width.saturating_sub(1));
    cut.push('…');
    cut
}

#[cfg(test)]
mod tests {
    use super::{ReplHinter, only_extension};
    use crate::completion::{ArgNode, CompletionNode, CompletionTree, FlagNode, SuggestionEntry};
    use crate::repl::engine::adapter::ReplCompleter;
    use crate::repl::engine::editor::PaintedLine;
    use crate::repl::highlight::ReplHighlighter;
    use nu_ansi_term::{Color, Style};
    use reedline::{FileBackedHistory, Hinter, Span, Suggestion};

    fn tree() -> CompletionTree {
        let create = CompletionNode {
            tooltip: Some("Create a VM".to_string()),
            ..CompletionNode::default()
        }
        .with_flag("--name", FlagNode::new().tooltip("VM name"));
        let vm = CompletionNode::default()
            .with_child("create", create)
            .with_child("find", CompletionNode::default())
            .with_child(
                "power",
                CompletionNode {
                    tooltip: Some("Control VM power".to_string()),
                    args: vec![
                        ArgNode {
                            required: true,
                            ..ArgNode::named("HOSTNAME")
                        },
                        ArgNode::named("OPERATION").suggestions([
                            SuggestionEntry::value("on"),
                            SuggestionEntry::value("off"),
                        ]),
                        ArgNode::named("EXTRA").multi(),
                    ],
                    ..CompletionNode::default()
                },
            );
        CompletionTree {
            root: CompletionNode::default()
                .with_child("orch", CompletionNode::default().with_child("vm", vm)),
            ..CompletionTree::default()
        }
    }

    fn hinter() -> ReplHinter {
        ReplHinter::new(
            ReplCompleter::new(tree(), None),
            ReplHighlighter::new(tree(), Color::Green, Color::Red, None),
            PaintedLine::default(),
            Some(Style::new()),
        )
    }

    fn paint(line: &str) -> (String, String) {
        let mut hinter = hinter();
        let history = FileBackedHistory::new(10).expect("history");
        let out = hinter.handle(line, line.len(), &history, true, "");
        (hinter.complete_hint(), out)
    }

    #[test]
    fn hint_completes_the_only_candidate_and_names_it_unit() {
        let (inline, out) = paint("orch vm cr");
        assert_eq!(inline, "eate");
        assert_eq!(out, "eate\r\norch vm create  Create a VM");
        for (line, expected, slot) in [
            (
                "orch vm power ",
                "\r\nControl VM power\r\norch vm power <HOSTNAME> [OPERATION] [EXTRA]…",
                "<HOSTNAME>",
            ),
            (
                "orch vm power web01 ",
                "\r\nControl VM power\r\norch vm power HOSTNAME [OPERATION] [EXTRA]… · on | off",
                "[OPERATION]",
            ),
            (
                "orch vm power web01 on ",
                "\r\nControl VM power\r\norch vm power HOSTNAME OPERATION [EXTRA]…",
                "[EXTRA]…",
            ),
        ] {
            let (_, out) = paint(line);
            assert_eq!(
                out,
                expected.replace(slot, &Style::new().bold().paint(slot).to_string())
            );
        }
    }

    #[test]
    fn hint_suggests_unknown_command_words_unit() {
        let (inline, out) = paint("orch vm fnd ");
        assert_eq!(inline, "");
        assert_eq!(out, "\r\nDid you mean find?");
    }

    #[test]
    fn hint_describes_the_flag_being_given_a_value_unit() {
        let (_, out) = paint("orch vm create --name ");
        assert_eq!(out, "\r\n--name  VM name");
    }

    #[test]
    fn only_extension_ignores_candidates_that_do_not_extend_the_word_unit() {
        let span = Span { start: 0, end: 2 };
        let candidate = |value: &str| Suggestion {
            value: value.to_string(),
            span,
            ..Suggestion::default()
        };
        assert_eq!(
            only_extension("ld", &[candidate("ldap"), candidate("old")]),
            Some("ap".to_string())
        );
        assert_eq!(
            only_extension("ld", &[candidate("ldap"), candidate("ldif")]),
            None
        );
    }
}
