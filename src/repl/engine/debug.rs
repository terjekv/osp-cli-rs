//! REPL completion debug surfaces.
//!
//! These helpers build deterministic snapshots of the editor/menu state so
//! operators and tests can inspect completion behavior without relying on a
//! live terminal session.

use super::LineProjector;
use super::adapter::ReplCompleter;
use super::config::ReplAppearance;
use super::overlay::build_completion_menu;
use crate::completion::{CompletionEngine, CompletionTree};
use crate::repl::menu::{MenuDebug, OspCompletionMenu, debug_snapshot, display_text};
use reedline::{Completer, Editor, Menu, MenuEvent, UndoBehavior};
use serde::Serialize;

/// One rendered completion entry in the debug surface.
#[derive(Debug, Clone, Serialize)]
pub struct CompletionDebugMatch {
    /// Stable completion identifier or inserted value.
    pub id: String,
    /// Primary rendered label for the suggestion.
    pub label: String,
    /// Optional secondary description for the suggestion.
    pub description: Option<String>,
    /// Classified suggestion kind used by the debug view.
    pub kind: String,
}

/// Snapshot of completion/menu state for a given line and cursor position.
///
/// This is primarily consumed by REPL debug commands and tests.
#[derive(Debug, Clone, Serialize)]
pub struct CompletionDebug {
    /// Original input line.
    pub line: String,
    /// Cursor position used for analysis.
    pub cursor: usize,
    /// Replacement byte range for the active suggestion set.
    pub replace_range: [usize; 2],
    /// Current completion stub under the cursor.
    pub stub: String,
    /// Candidate matches visible to the menu.
    pub matches: Vec<CompletionDebugMatch>,
    /// Completion menu layout, selection, styling, and rendered lines.
    #[serde(flatten)]
    pub menu: MenuDebug,
    /// Virtual render width.
    pub width: u16,
    /// Virtual render height.
    pub height: u16,
    /// Whether Unicode menu chrome was enabled.
    pub unicode: bool,
    /// Whether ANSI styling was enabled.
    pub color: bool,
}

/// Synthetic editor action used by menu-debug stepping.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DebugStep {
    /// Apply Tab behavior: complete a closed completion menu or advance an open one.
    Tab,
    /// Move to the previous item.
    BackTab,
    /// Move selection upward.
    Up,
    /// Move selection downward.
    Down,
    /// Move selection left.
    Left,
    /// Move selection right.
    Right,
    /// Accept the selected item.
    Accept,
    /// Close the completion menu.
    Close,
}

impl DebugStep {
    /// Parses a step name accepted by REPL debug commands.
    pub fn parse(raw: &str) -> Option<Self> {
        match raw.trim().to_ascii_lowercase().as_str() {
            "tab" => Some(Self::Tab),
            "backtab" | "shift-tab" | "shift_tab" => Some(Self::BackTab),
            "up" => Some(Self::Up),
            "down" => Some(Self::Down),
            "left" => Some(Self::Left),
            "right" => Some(Self::Right),
            "accept" | "enter" => Some(Self::Accept),
            "close" | "esc" | "escape" => Some(Self::Close),
            _ => None,
        }
    }

    /// Returns the stable lowercase name used in debug payloads.
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Tab => "tab",
            Self::BackTab => "backtab",
            Self::Up => "up",
            Self::Down => "down",
            Self::Left => "left",
            Self::Right => "right",
            Self::Accept => "accept",
            Self::Close => "close",
        }
    }
}

/// One frame from a stepped completion-debug session.
#[derive(Debug, Clone, Serialize)]
pub struct CompletionDebugFrame {
    /// Synthetic step that produced this frame.
    pub step: String,
    /// Captured completion state after the step.
    pub state: CompletionDebug,
}

/// Rendering and capture options for completion-debug helpers.
#[derive(Clone)]
#[non_exhaustive]
#[must_use]
pub struct CompletionDebugOptions<'a> {
    /// Virtual render width for the debug menu.
    pub width: u16,
    /// Virtual render height for the debug menu.
    pub height: u16,
    /// Whether ANSI styling should be enabled in captured output.
    pub ansi: bool,
    /// Whether Unicode menu chrome should be enabled in captured output.
    pub unicode: bool,
    /// Optional appearance override used for the debug session.
    pub appearance: Option<&'a ReplAppearance>,
    /// Optional projection used by the live REPL completion surface.
    pub line_projector: Option<&'a LineProjector>,
}

impl<'a> CompletionDebugOptions<'a> {
    /// Creates a new debug snapshot configuration for a virtual terminal size.
    ///
    /// # Examples
    ///
    /// ```text
    /// use osp_cli::repl::CompletionDebugOptions;
    ///
    /// let options = CompletionDebugOptions::new(100, 12)
    ///     .with_ansi(true)
    ///     .with_unicode(true);
    ///
    /// assert_eq!(options.width, 100);
    /// assert_eq!(options.height, 12);
    /// assert!(options.ansi);
    /// assert!(options.unicode);
    /// ```
    pub fn new(width: u16, height: u16) -> Self {
        Self {
            width,
            height,
            ansi: false,
            unicode: false,
            appearance: None,
            line_projector: None,
        }
    }

    /// Enables ANSI styling in captured menu output.
    pub fn with_ansi(mut self, ansi: bool) -> Self {
        self.ansi = ansi;
        self
    }

    /// Selects unicode or ASCII menu chrome when rendering debug output.
    pub fn with_unicode(mut self, unicode: bool) -> Self {
        self.unicode = unicode;
        self
    }

    /// Applies REPL appearance overrides to the debug menu session.
    pub fn with_appearance(mut self, appearance: Option<&'a ReplAppearance>) -> Self {
        self.appearance = appearance;
        self
    }

    /// Applies the same line projection used by the live REPL menu.
    pub fn with_line_projector(mut self, line_projector: Option<&'a LineProjector>) -> Self {
        self.line_projector = line_projector;
        self
    }
}

/// Builds a single completion-debug snapshot for `line` at `cursor`.
pub fn debug_completion(
    tree: &CompletionTree,
    line: &str,
    cursor: usize,
    options: CompletionDebugOptions<'_>,
) -> CompletionDebug {
    let (editor, mut completer, mut menu) = build_debug_completion_session(
        tree,
        line,
        cursor,
        options.appearance,
        options.line_projector.cloned(),
    );
    let mut editor = editor;

    menu.menu_event(MenuEvent::Activate(false));
    menu.apply_event(&mut editor, completer.as_mut());

    snapshot_completion_debug(tree, &mut menu, &editor, &options)
}

/// Replays a sequence of synthetic editor actions and captures each frame.
pub fn debug_completion_steps(
    tree: &CompletionTree,
    line: &str,
    cursor: usize,
    options: CompletionDebugOptions<'_>,
    steps: &[DebugStep],
) -> Vec<CompletionDebugFrame> {
    capture_debug_steps(
        build_debug_completion_session(
            tree,
            line,
            cursor,
            options.appearance,
            options.line_projector.cloned(),
        ),
        steps,
        |menu, editor| snapshot_completion_debug(tree, menu, editor, &options),
    )
}

fn capture_debug_steps(
    (mut editor, mut completer, mut menu): (Editor, Box<dyn Completer>, OspCompletionMenu),
    steps: &[DebugStep],
    snapshot: impl Fn(&mut OspCompletionMenu, &Editor) -> CompletionDebug,
) -> Vec<CompletionDebugFrame> {
    steps
        .iter()
        .map(|&step| {
            apply_debug_step(step, &mut menu, &mut editor, completer.as_mut());
            CompletionDebugFrame {
                step: step.as_str().to_string(),
                state: snapshot(&mut menu, &editor),
            }
        })
        .collect()
}

fn build_debug_completion_session(
    tree: &CompletionTree,
    line: &str,
    cursor: usize,
    appearance: Option<&ReplAppearance>,
    line_projector: Option<LineProjector>,
) -> (Editor, Box<dyn Completer>, OspCompletionMenu) {
    let mut editor = Editor::default();
    editor.edit_buffer(
        |buf| {
            buf.set_buffer(line.to_string());
            buf.set_insertion_point(cursor.min(buf.get_buffer().len()));
        },
        UndoBehavior::CreateUndoPoint,
    );

    let completer = Box::new(
        ReplCompleter::new(tree.clone(), line_projector)
            .with_command_prefixes(appearance.is_none_or(|appearance| appearance.command_prefixes)),
    );
    let menu = if let Some(appearance) = appearance {
        build_completion_menu(appearance)
    } else {
        OspCompletionMenu::default()
    };

    (editor, completer, menu)
}

fn apply_debug_step(
    step: DebugStep,
    menu: &mut OspCompletionMenu,
    editor: &mut Editor,
    completer: &mut dyn Completer,
) {
    match step {
        DebugStep::Tab => {
            menu.tab(editor, completer);
        }
        DebugStep::BackTab => {
            if menu.is_active() {
                menu.navigate(MenuEvent::PreviousElement, editor);
            } else {
                dispatch_menu_event(menu, editor, completer, MenuEvent::Activate(false));
            }
        }
        DebugStep::Up => {
            if menu.is_active() {
                menu.navigate(MenuEvent::MoveUp, editor);
            }
        }
        DebugStep::Down => {
            if menu.is_active() {
                menu.navigate(MenuEvent::MoveDown, editor);
            }
        }
        DebugStep::Left => {
            if menu.is_active() {
                menu.navigate(MenuEvent::MoveLeft, editor);
            }
        }
        DebugStep::Right => {
            if menu.is_active() {
                menu.navigate(MenuEvent::MoveRight, editor);
            }
        }
        DebugStep::Accept => {
            if menu.is_active() {
                menu.accept_selection_in_buffer(editor);
                dispatch_menu_event(menu, editor, completer, MenuEvent::Deactivate);
            }
        }
        DebugStep::Close => {
            dispatch_menu_event(menu, editor, completer, MenuEvent::Deactivate);
        }
    }
}

fn dispatch_menu_event(
    menu: &mut OspCompletionMenu,
    editor: &mut Editor,
    completer: &mut dyn Completer,
    event: MenuEvent,
) {
    menu.menu_event(event);
    menu.apply_event(editor, completer);
}

fn snapshot_completion_debug(
    tree: &CompletionTree,
    menu: &mut OspCompletionMenu,
    editor: &Editor,
    options: &CompletionDebugOptions<'_>,
) -> CompletionDebug {
    let cursor = editor.line_buffer().insertion_point();
    let engine = CompletionEngine::new(tree.clone());
    let analysis = engine.analyze(editor.get_buffer(), cursor);
    snapshot_menu_debug(
        menu,
        editor,
        cursor,
        options,
        (
            analysis.cursor.raw_stub.clone(),
            [
                analysis.cursor.replace_range.start,
                analysis.cursor.replace_range.end,
            ],
        ),
        |value| engine.classify_match(&analysis, value).as_str().to_string(),
    )
}

fn snapshot_menu_debug(
    menu: &mut OspCompletionMenu,
    editor: &Editor,
    cursor: usize,
    options: &CompletionDebugOptions<'_>,
    fallback: (String, [usize; 2]),
    classify: impl Fn(&str) -> String,
) -> CompletionDebug {
    let line = editor.get_buffer().to_string();
    let values = menu.get_values();
    let (stub, replace_range) = if let Some(first) = values.first() {
        let start = first.span.start;
        let end = first.span.end;
        (line.get(start..end).unwrap_or("").to_string(), [start, end])
    } else {
        fallback
    };
    let matches = values
        .iter()
        .map(|item| CompletionDebugMatch {
            id: item.value.clone(),
            label: display_text(item).to_string(),
            description: item.description.clone(),
            kind: classify(&item.value),
        })
        .collect::<Vec<_>>();

    let menu = debug_snapshot(menu, editor, options.width, options.height, options.ansi);

    CompletionDebug {
        line,
        cursor,
        replace_range,
        stub,
        matches,
        menu,
        width: options.width,
        height: options.height,
        unicode: options.unicode,
        color: options.ansi,
    }
}

#[cfg(test)]
mod tests {

    #[test]
    fn repl_internal_example_1() {
        use crate::repl::CompletionDebugOptions;

        let options = CompletionDebugOptions::new(100, 12)
            .with_ansi(true)
            .with_unicode(true);

        assert_eq!(options.width, 100);
        assert_eq!(options.height, 12);
        assert!(options.ansi);
        assert!(options.unicode);
    }
}
