//! Internal prompt and line-editor adapter mechanics.
//!
//! Host-facing REPL configuration lives in [`super::config`]. This module
//! translates that semantic surface into reedline-specific behavior such as
//! prompt rendering and menu reopening.

use std::borrow::Cow;
use std::io::{self, IsTerminal};
use std::sync::{Arc, Mutex, PoisonError};

use crossterm::event::{Event, KeyCode, KeyEvent, KeyModifiers};
use reedline::{
    EditCommand, EditMode, Emacs, Menu, MenuEvent, Prompt, PromptEditMode, PromptHistorySearch,
    PromptHistorySearchStatus, ReedlineEvent, ReedlineRawEvent,
};

use super::{
    HOST_COMMAND_LAST_ARGUMENT, HOST_COMMAND_PREVIOUS_ARGUMENT, PromptRightRenderer,
    ReplCompleteOn, ReplInputMode,
};
use crate::repl::menu::SharedCompletionMenu;

/// The input line and cursor as reedline last painted them.
///
/// An edit mode only sees key events, never the buffer. The hinter runs on
/// every paint and records the line here, so key handling can measure the
/// word being typed.
#[derive(Clone, Default)]
pub(crate) struct PaintedLine(Arc<Mutex<PaintedLineState>>);

#[derive(Default)]
struct PaintedLineState {
    line: String,
    cursor: usize,
    revision: u64,
    history_suggestions: Vec<String>,
    history_index: usize,
    inline: String,
}

impl PaintedLine {
    pub(crate) fn set(&self, line: &str, cursor: usize) {
        let mut painted = self.0.lock().unwrap_or_else(PoisonError::into_inner);
        if painted.line != line || painted.cursor != cursor {
            painted.history_index = 0;
            painted.history_suggestions.clear();
        }
        painted.line.clear();
        painted.line.push_str(line);
        painted.cursor = cursor;
        painted.revision = painted.revision.wrapping_add(1);
    }

    pub(crate) fn history_suggestion(&self, suggestions: Vec<String>) -> Option<String> {
        let mut painted = self.0.lock().unwrap_or_else(PoisonError::into_inner);
        if painted.history_suggestions != suggestions {
            painted.history_index = 0;
            painted.history_suggestions = suggestions;
        }
        painted
            .history_suggestions
            .get(painted.history_index)
            .cloned()
    }

    pub(crate) fn set_inline(&self, inline: String) {
        self.0.lock().unwrap_or_else(PoisonError::into_inner).inline = inline;
    }

    pub(crate) fn inline(&self) -> String {
        self.0
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .inline
            .clone()
    }

    fn clear_history_suggestions(&self) {
        let mut painted = self.0.lock().unwrap_or_else(PoisonError::into_inner);
        painted.history_suggestions.clear();
        painted.history_index = 0;
    }

    fn cycle_history_suggestion(&self) -> bool {
        let mut painted = self.0.lock().unwrap_or_else(PoisonError::into_inner);
        if painted.cursor != painted.line.len() || painted.history_suggestions.is_empty() {
            return false;
        }
        painted.history_index = (painted.history_index + 1) % painted.history_suggestions.len();
        // Reedline parses a key batch before repainting, including hint acceptance.
        painted.inline = painted.history_suggestions[painted.history_index].clone();
        true
    }

    /// Characters between the start of the current word and the cursor.
    fn word_len_before_cursor(&self) -> (usize, u64) {
        let painted = self.0.lock().unwrap_or_else(PoisonError::into_inner);
        let before = painted.line.get(..painted.cursor).unwrap_or(&painted.line);
        (
            before
                .chars()
                .rev()
                .take_while(|ch| !ch.is_whitespace())
                .count(),
            painted.revision,
        )
    }
}

pub(crate) struct AutoCompleteEmacs {
    inner: Emacs,
    menu: SharedCompletionMenu,
    complete_on: ReplCompleteOn,
    painted: PaintedLine,
    painted_revision: u64,
    pending_word_len: Option<usize>,
    last_argument: bool,
}

impl AutoCompleteEmacs {
    pub(crate) fn new(
        inner: Emacs,
        menu: SharedCompletionMenu,
        complete_on: ReplCompleteOn,
        painted: PaintedLine,
    ) -> Self {
        let (pending_word_len, painted_revision) = painted.word_len_before_cursor();
        Self {
            inner,
            menu,
            complete_on,
            painted,
            painted_revision,
            pending_word_len: Some(pending_word_len),
            last_argument: false,
        }
    }

    #[cfg(test)]
    pub(crate) fn opens_menu(&self, commands: &[EditCommand]) -> bool {
        self.opens_menu_at(commands, Some(self.painted.word_len_before_cursor().0))
    }

    /// Whether typing these commands should open a closed menu.
    ///
    /// A `-` inside a word such as a hostname is not a flag trigger.
    /// Spaces are handled by the token commit below.
    fn opens_menu_at(&self, commands: &[EditCommand], word_len: Option<usize>) -> bool {
        let [EditCommand::InsertChar(ch)] = commands else {
            return false;
        };
        (self.complete_on.flag && *ch == '-' && word_len == Some(0))
            || (!ch.is_whitespace()
                && self
                    .complete_on
                    .after_letters
                    .is_some_and(|count| word_len.is_some_and(|len| len + 1 >= count)))
    }

    fn sync_painted_word_len(&mut self) -> Option<usize> {
        let (word_len, revision) = self.painted.word_len_before_cursor();
        if revision != self.painted_revision {
            self.painted_revision = revision;
            self.pending_word_len = Some(word_len);
        }
        self.pending_word_len
    }

    fn track_edit(&mut self, commands: &[EditCommand], word_len: Option<usize>) {
        self.pending_word_len = match commands {
            [EditCommand::InsertChar(ch)] if ch.is_whitespace() => Some(0),
            [EditCommand::InsertChar(_)] => word_len.map(|len| len + 1),
            [EditCommand::InsertString(text)] if text.chars().any(char::is_whitespace) => Some(
                text.chars()
                    .rev()
                    .take_while(|ch| !ch.is_whitespace())
                    .count(),
            ),
            [EditCommand::InsertString(text)] => word_len.map(|len| len + text.chars().count()),
            [EditCommand::InsertNewline] => Some(0),
            _ => None,
        };
    }

    fn auto_open(&self) -> ReedlineEvent {
        ReedlineEvent::Menu(self.menu.name().to_string())
    }

    /// Whether `event` is Tab opening a closed menu.
    fn is_tab_open(&self, event: &ReedlineEvent) -> bool {
        matches!(
            event,
            ReedlineEvent::UntilFound(events)
                if matches!(
                    events.as_slice(),
                    [ReedlineEvent::Menu(name), ReedlineEvent::MenuNext] if name == self.menu.name()
                )
        )
    }
}

/// The menu move a key binding asks for once the completion menu is open.
///
/// With the menu open, reedline resolves `UntilFound` to its first menu move:
/// `Menu(name)` only applies to a closed menu. Menu navigation takes precedence
/// over accepting an inline history hint.
pub(crate) fn menu_navigation(event: &ReedlineEvent) -> Option<MenuEvent> {
    match event {
        ReedlineEvent::MenuNext => Some(MenuEvent::NextElement),
        ReedlineEvent::MenuPrevious => Some(MenuEvent::PreviousElement),
        ReedlineEvent::MenuUp => Some(MenuEvent::MoveUp),
        ReedlineEvent::MenuDown => Some(MenuEvent::MoveDown),
        ReedlineEvent::MenuLeft => Some(MenuEvent::MoveLeft),
        ReedlineEvent::MenuRight => Some(MenuEvent::MoveRight),
        ReedlineEvent::MenuPageNext => Some(MenuEvent::NextPage),
        ReedlineEvent::MenuPagePrevious => Some(MenuEvent::PreviousPage),
        ReedlineEvent::UntilFound(events) => events.iter().find_map(menu_navigation),
        _ => None,
    }
}

impl EditMode for AutoCompleteEmacs {
    fn parse_event(&mut self, event: ReedlineRawEvent) -> ReedlineEvent {
        let word_len = self.sync_painted_word_len();
        let event: Event = event.into();
        if matches!(&event, Event::Key(KeyEvent { code: KeyCode::Right, modifiers, .. })
            if *modifiers == KeyModifiers::CONTROL)
            && !self.menu.is_active()
            && self.painted.cycle_history_suggestion()
        {
            self.last_argument = false;
            return ReedlineEvent::Repaint;
        }
        let parsed = self.inner.parse_event(
            event
                .try_into()
                .expect("previously validated reedline event"),
        );
        // Keys can arrive before the next paint; never cycle a stale prefix.
        if !matches!(parsed, ReedlineEvent::Resize(..) | ReedlineEvent::Repaint) {
            self.painted.clear_history_suggestions();
        }
        if matches!(&parsed, ReedlineEvent::ExecuteHostCommand(name) if name == HOST_COMMAND_LAST_ARGUMENT)
        {
            let command = if self.last_argument {
                HOST_COMMAND_PREVIOUS_ARGUMENT
            } else {
                HOST_COMMAND_LAST_ARGUMENT
            };
            self.last_argument = true;
            self.pending_word_len = None;
            return ReedlineEvent::Multiple(vec![
                ReedlineEvent::Esc,
                ReedlineEvent::ExecuteHostCommand(command.to_string()),
            ]);
        }
        if !matches!(parsed, ReedlineEvent::Resize(..)) {
            self.last_argument = false;
        }
        // Selection moves become buffer edits here, before reedline paints;
        // see `OspCompletionMenu::navigate`.
        if let Some(commands) =
            menu_navigation(&parsed).and_then(|nav| self.menu.navigate_painted_line(nav))
        {
            self.pending_word_len = None;
            return ReedlineEvent::Edit(commands);
        }
        // Only Tab completes like a shell; menus opened by typing just list.
        if self.is_tab_open(&parsed) {
            self.menu.mark_tab_open();
        }
        match parsed {
            ReedlineEvent::Edit(commands) if commands == [EditCommand::InsertChar(' ')] => {
                self.track_edit(&commands, word_len);
                // Space commits the token: close the menu, keeping any cycled
                // selection already in the buffer, instead of letting reedline
                // refresh it with the next token's candidates.
                let mut events = vec![ReedlineEvent::Esc, ReedlineEvent::Edit(commands)];
                if self.complete_on.space && word_len.is_some_and(|len| len > 0) {
                    events.push(self.auto_open());
                }
                ReedlineEvent::Multiple(events)
            }
            // An open menu already refilters on every edit.
            ReedlineEvent::Edit(commands)
                if !self.menu.is_active() && self.opens_menu_at(&commands, word_len) =>
            {
                self.track_edit(&commands, word_len);
                ReedlineEvent::Multiple(vec![ReedlineEvent::Edit(commands), self.auto_open()])
            }
            ReedlineEvent::Edit(commands) => {
                self.track_edit(&commands, word_len);
                ReedlineEvent::Edit(commands)
            }
            other => other,
        }
    }

    fn edit_mode(&self) -> PromptEditMode {
        self.inner.edit_mode()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum BasicInputReason {
    Explicit,
    NotATerminal,
}

pub(crate) fn basic_input_reason(input_mode: ReplInputMode) -> Option<BasicInputReason> {
    if matches!(input_mode, ReplInputMode::Basic) {
        return Some(BasicInputReason::Explicit);
    }

    if !io::stdin().is_terminal() || !io::stdout().is_terminal() {
        return Some(BasicInputReason::NotATerminal);
    }

    None
}

pub(crate) struct OspPrompt {
    left: String,
    indicator: String,
    right: Option<PromptRightRenderer>,
}

impl OspPrompt {
    pub(crate) fn new(left: String, indicator: String, right: Option<PromptRightRenderer>) -> Self {
        Self {
            left,
            indicator,
            right,
        }
    }

    pub(crate) fn left(&self) -> &str {
        &self.left
    }

    pub(crate) fn indicator(&self) -> &str {
        &self.indicator
    }
}

impl Prompt for OspPrompt {
    fn render_prompt_left(&self) -> Cow<'_, str> {
        Cow::Borrowed(self.left.as_str())
    }

    fn render_prompt_right(&self) -> Cow<'_, str> {
        match &self.right {
            Some(render) => Cow::Owned(render()),
            None => Cow::Borrowed(""),
        }
    }

    fn render_prompt_indicator(&self, _prompt_mode: PromptEditMode) -> Cow<'_, str> {
        Cow::Borrowed(self.indicator.as_str())
    }

    fn render_prompt_multiline_indicator(&self) -> Cow<'_, str> {
        Cow::Borrowed("... ")
    }

    fn render_prompt_history_search_indicator(
        &self,
        history_search: PromptHistorySearch,
    ) -> Cow<'_, str> {
        let prefix = match history_search.status {
            PromptHistorySearchStatus::Passing => "",
            PromptHistorySearchStatus::Failing => "failing ",
        };
        Cow::Owned(format!(
            "({prefix}reverse-search: {}) ",
            history_search.term
        ))
    }

    fn get_prompt_color(&self) -> nu_ansi_term::Color {
        nu_ansi_term::Color::Default
    }

    fn get_indicator_color(&self) -> nu_ansi_term::Color {
        nu_ansi_term::Color::Default
    }
}
