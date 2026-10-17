//! Two-column terminal choices with a staged Save/Exit boundary.
//!
//! Models own values, validation and preview rendering. This component owns
//! navigation, inline editing and terminal restoration, and never writes config.

use std::io::{self, IsTerminal, Write};

use crossterm::{
    cursor::{Hide, MoveTo, Show},
    event::{self, Event, KeyCode, KeyEventKind, KeyModifiers},
    execute, queue,
    terminal::{
        self, Clear, ClearType, DisableLineWrap, EnableLineWrap, EnterAlternateScreen,
        LeaveAlternateScreen,
    },
};

use super::settings::resolve_settings;
use super::style::{StyleToken, ThemeStyler};
use super::{RenderProfile, RenderSettings, crop_display_width, sanitize_human_text};

/// Content and optional value editing for a terminal picker.
pub trait PickerModel {
    /// Effective theme and rendering policy for the selected entry.
    fn settings(&self, selected: usize) -> RenderSettings;
    /// Render the detail pane within `width` terminal columns.
    fn detail(&self, selected: usize, width: usize) -> String;
    /// Editable text, or `None` for entries without a text editor.
    fn edit_value(&self, _selected: usize) -> Option<String> {
        None
    }
    /// Whether the inline editor must hide typed characters.
    fn secret(&self, _selected: usize) -> bool {
        false
    }
    /// Stage an edited value, returning a user-facing validation failure.
    fn set_value(&mut self, _selected: usize, _value: &str) -> Result<(), String> {
        Ok(())
    }
    /// Cycle a constrained value. Return false when this entry has no choices.
    fn cycle_value(&mut self, _selected: usize, _backwards: bool) -> bool {
        false
    }
}

/// Opens a picker and returns the selected index only after Save is activated.
///
/// Up/Down and Tab/Shift-Tab move through entries and the Save/Exit buttons.
/// Models with constrained values consume Tab and Left/Right to cycle them.
/// End focuses Save; Enter on an entry edits it. Esc, q and Ctrl-C cancel.
/// The terminal is restored on success, cancellation, I/O failure and unwind.
pub fn run(
    labels: &[String],
    initial: usize,
    model: &mut impl PickerModel,
) -> io::Result<Option<usize>> {
    if !io::stdin().is_terminal() || !io::stdout().is_terminal() || !io::stderr().is_terminal() {
        return Err(io::Error::other("interactive picker requires a terminal"));
    }
    if labels.is_empty() {
        return Err(io::Error::other("no entries to select"));
    }
    let _terminal = Terminal::enter()?;
    let mut out = io::stderr();
    let mut selected = initial.min(labels.len() - 1);
    let mut focus = selected;
    let mut edit: Option<String> = None;
    let mut error = String::new();
    loop {
        let (cols, rows) = terminal::size()?;
        if cols < 60 || rows < 12 {
            return Err(io::Error::other(
                "picker needs a terminal of at least 60 columns and 12 rows",
            ));
        }
        let settings = model.settings(selected);
        let resolved = resolve_settings(&settings, RenderProfile::Normal);
        let styler = ThemeStyler::new(resolved.color, &resolved.theme, &resolved.style_overrides);
        let left = usize::from(cols / 3).min(42);
        let detail_width = usize::from(cols).saturating_sub(left + 4);
        let height = usize::from(rows).saturating_sub(5);
        let start = selected.saturating_sub(height - 1);
        queue!(out, MoveTo(0, 0), Clear(ClearType::All))?;
        let detail = model.detail(selected, detail_width);
        let detail = detail.lines().collect::<Vec<_>>();
        for line in 0..height {
            queue!(out, MoveTo(0, line as u16))?;
            if let Some(label) = labels.get(start + line) {
                let text = crop_display_width(&sanitize_human_text(label), left - 3);
                let text = format!(
                    "{} {text}",
                    if start + line == selected { ">" } else { " " }
                );
                write!(
                    out,
                    "{}",
                    styler.paint(
                        &text,
                        if start + line == selected {
                            StyleToken::Key
                        } else {
                            StyleToken::TextMuted
                        }
                    )
                )?;
            }
            queue!(out, MoveTo(left as u16, line as u16))?;
            write!(out, "{}", styler.paint("|", StyleToken::Border))?;
            queue!(out, MoveTo((left + 2) as u16, line as u16))?;
            if let Some(text) = detail.get(line) {
                write!(out, "{text}")?;
            }
        }
        queue!(out, MoveTo(0, rows - 4))?;
        if let Some(value) = &edit {
            let value = if model.secret(selected) {
                "*".repeat(value.chars().count())
            } else {
                sanitize_human_text(value)
            };
            let value = crop_display_width(&value, usize::from(cols).saturating_sub(4));
            write!(
                out,
                "{}",
                styler.paint(&format!("> {value}_"), StyleToken::Text)
            )?;
        }
        if !error.is_empty() {
            queue!(out, MoveTo(0, rows - 2))?;
            write!(
                out,
                "{}",
                styler.paint(
                    &crop_display_width(&sanitize_human_text(&error), usize::from(cols)),
                    StyleToken::Error
                )
            )?;
        }
        queue!(out, MoveTo(0, rows - 3))?;
        for (index, button) in ["[ Save ]", "[ Exit ]"].iter().enumerate() {
            let text = format!(
                "{} {button}  ",
                if focus == labels.len() + index {
                    ">"
                } else {
                    " "
                }
            );
            write!(
                out,
                "{}",
                styler.paint(
                    &text,
                    if focus == labels.len() + index {
                        StyleToken::Key
                    } else {
                        StyleToken::Text
                    }
                )
            )?;
        }
        queue!(out, MoveTo(0, rows - 1))?;
        let hint = if edit.is_some() {
            "Enter accept | Ctrl-U clear | Esc discard edit"
        } else {
            "Up/Down select | Tab next/value | Left/Right value | Enter edit | End buttons | Esc/q cancel"
        };
        write!(
            out,
            "{}",
            styler.paint(
                &crop_display_width(hint, usize::from(cols)),
                StyleToken::Muted
            )
        )?;
        out.flush()?;
        let Event::Key(key) = event::read()? else {
            continue;
        };
        if key.kind == KeyEventKind::Release {
            continue;
        }
        if key.modifiers.contains(KeyModifiers::CONTROL) && key.code == KeyCode::Char('c') {
            return Ok(None);
        }
        if let Some(value) = &mut edit {
            match key.code {
                KeyCode::Esc => {
                    edit = None;
                    error.clear();
                }
                KeyCode::Enter => match model.set_value(selected, value) {
                    Ok(()) => {
                        edit = None;
                        error.clear();
                    }
                    Err(message) => error = message,
                },
                KeyCode::Char('u') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                    value.clear()
                }
                KeyCode::Backspace => {
                    value.pop();
                }
                KeyCode::Char(ch)
                    if !key
                        .modifiers
                        .intersects(KeyModifiers::CONTROL | KeyModifiers::ALT) =>
                {
                    value.push(ch)
                }
                _ => {}
            }
            continue;
        }
        error.clear();
        match key.code {
            KeyCode::Esc | KeyCode::Char('q') => return Ok(None),
            KeyCode::Enter if focus == labels.len() => return Ok(Some(selected)),
            KeyCode::Enter if focus == labels.len() + 1 => return Ok(None),
            KeyCode::Enter => {
                if !model.cycle_value(selected, false) {
                    edit = model.edit_value(selected);
                }
            }
            KeyCode::Home => focus = 0,
            KeyCode::End => focus = labels.len(),
            KeyCode::Left | KeyCode::Right if focus < labels.len() => {
                if !model.cycle_value(selected, key.code == KeyCode::Left) {
                    focus = labels.len();
                }
            }
            KeyCode::Left | KeyCode::Right => {
                focus = if focus == labels.len() {
                    labels.len() + 1
                } else {
                    labels.len()
                }
            }
            KeyCode::Tab | KeyCode::BackTab
                if focus < labels.len()
                    && model.cycle_value(selected, key.code == KeyCode::BackTab) => {}
            KeyCode::Down | KeyCode::Tab => focus = (focus + 1) % (labels.len() + 2),
            KeyCode::Up | KeyCode::BackTab => {
                focus = (focus + labels.len() + 1) % (labels.len() + 2)
            }
            _ => {}
        }
        if focus < labels.len() {
            selected = focus;
        }
    }
}

struct Terminal {
    was_raw: bool,
}

impl Terminal {
    fn enter() -> io::Result<Self> {
        let was_raw = terminal::is_raw_mode_enabled()?;
        terminal::enable_raw_mode()?;
        let guard = Self { was_raw };
        execute!(io::stderr(), EnterAlternateScreen, Hide, DisableLineWrap)?;
        Ok(guard)
    }
}

impl Drop for Terminal {
    fn drop(&mut self) {
        let _ = execute!(io::stderr(), Show, EnableLineWrap, LeaveAlternateScreen);
        if !self.was_raw {
            let _ = terminal::disable_raw_mode();
        }
    }
}
