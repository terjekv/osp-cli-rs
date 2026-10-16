//! Shared shell syntax scanning and quoting for command-entry surfaces.
//!
//! Completion and the DSL share quote, escape, whitespace, and pipe events with
//! byte spans. Callers own word assembly, incomplete-input recovery, and DSL
//! operators. POSIX command-word decoding stays with `shell-words`: comments,
//! line continuations, whitespace, and double-quote escapes differ there.

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum LexState {
    Normal,
    SingleQuote,
    DoubleQuote,
    EscapeNormal,
    EscapeDouble,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum LexEventKind {
    Separator,
    Pipe,
    QuoteStart,
    QuoteEnd,
    EscapeStart,
    Text(char),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct LexEvent {
    pub(crate) index: usize,
    pub(crate) end: usize,
    pub(crate) state_before: LexState,
    pub(crate) kind: LexEventKind,
    pub(crate) ch: char,
}

#[derive(Debug, Clone, Copy)]
pub(crate) struct LexEnd {
    pub(crate) state: LexState,
    pub(crate) quote_start: usize,
}

/// Visits shell syntax without deciding how callers assemble or recover words.
pub(crate) fn scan_shell(input: &str, mut visit: impl FnMut(LexEvent)) -> LexEnd {
    let mut state = LexState::Normal;
    let mut quote_start = 0;
    for (index, ch) in input.char_indices() {
        let state_before = state;
        let kind = match state {
            LexState::Normal if ch.is_whitespace() => LexEventKind::Separator,
            LexState::Normal => match ch {
                '|' => LexEventKind::Pipe,
                '\\' => {
                    state = LexState::EscapeNormal;
                    LexEventKind::EscapeStart
                }
                '\'' => {
                    state = LexState::SingleQuote;
                    LexEventKind::QuoteStart
                }
                '"' => {
                    state = LexState::DoubleQuote;
                    LexEventKind::QuoteStart
                }
                _ => LexEventKind::Text(ch),
            },
            LexState::SingleQuote if ch == '\'' => {
                state = LexState::Normal;
                LexEventKind::QuoteEnd
            }
            LexState::SingleQuote => LexEventKind::Text(ch),
            LexState::DoubleQuote => match ch {
                '"' => {
                    state = LexState::Normal;
                    LexEventKind::QuoteEnd
                }
                '\\' => {
                    state = LexState::EscapeDouble;
                    LexEventKind::EscapeStart
                }
                _ => LexEventKind::Text(ch),
            },
            LexState::EscapeNormal => {
                state = LexState::Normal;
                LexEventKind::Text(ch)
            }
            LexState::EscapeDouble => {
                state = LexState::DoubleQuote;
                LexEventKind::Text(ch)
            }
        };
        if kind == LexEventKind::QuoteStart {
            quote_start = index;
        }
        visit(LexEvent {
            index,
            end: index + ch.len_utf8(),
            state_before,
            kind,
            ch,
        });
    }
    LexEnd { state, quote_start }
}

/// Quoting style to use when formatting a shell argument.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum QuoteStyle {
    /// Wraps the value in single quotes and escapes embedded single quotes.
    Single,
    /// Wraps the value in double quotes and escapes shell-sensitive characters.
    Double,
}

/// Quotes `value` for shell reuse using the requested quoting style.
///
/// Use [`QuoteStyle::Single`] when you want the most literal shell-safe output,
/// and [`QuoteStyle::Double`] when interpolation-style shell syntax should stay
/// visually recognizable to the user.
///
/// # Examples
///
/// ```
/// use osp_cli::core::shell_words::{QuoteStyle, quote_for_shell};
///
/// assert_eq!(quote_for_shell("O'Brien", QuoteStyle::Single), "'O'\\''Brien'");
/// assert_eq!(quote_for_shell("hello world", QuoteStyle::Double), "\"hello world\"");
/// ```
pub fn quote_for_shell(value: &str, style: QuoteStyle) -> String {
    match style {
        QuoteStyle::Single => quote_single(value),
        QuoteStyle::Double => quote_double(value),
    }
}

/// Escapes shell-sensitive characters, using single quotes when needed to
/// preserve empty arguments or line breaks.
///
/// This is useful for tab-completion and history displays where adding full
/// quotes would be noisier than backslash-escaping.
///
/// # Examples
///
/// ```
/// use osp_cli::core::shell_words::escape_for_shell;
///
/// assert_eq!(
///     escape_for_shell("team docs/file name.txt"),
///     "team\\ docs/file\\ name.txt"
/// );
/// ```
pub fn escape_for_shell(value: &str) -> String {
    if value.is_empty() || value.contains('\n') || value.contains('\r') {
        return quote_single(value);
    }

    let mut out = String::with_capacity(value.len());
    for ch in value.chars() {
        if is_unquoted_safe(ch) {
            out.push(ch);
        } else {
            out.push('\\');
            out.push(ch);
        }
    }
    out
}

fn quote_single(value: &str) -> String {
    let mut out = String::from("'");
    for ch in value.chars() {
        if ch == '\'' {
            out.push_str("'\\''");
        } else {
            out.push(ch);
        }
    }
    out.push('\'');
    out
}

fn quote_double(value: &str) -> String {
    let mut out = String::from("\"");
    for ch in value.chars() {
        match ch {
            '"' | '\\' | '$' | '`' => {
                out.push('\\');
                out.push(ch);
            }
            _ => out.push(ch),
        }
    }
    out.push('"');
    out
}

fn is_unquoted_safe(ch: char) -> bool {
    ch.is_alphanumeric() || matches!(ch, '/' | '.' | '_' | '-' | '~' | ':' | '+')
}

#[cfg(test)]
mod tests {
    use super::{QuoteStyle, escape_for_shell, quote_for_shell};

    #[test]
    fn escape_for_shell_backslashes_unsafe_characters() {
        assert_eq!(
            escape_for_shell("team docs/file name.txt"),
            "team\\ docs/file\\ name.txt"
        );
        assert_eq!(escape_for_shell("rød"), "rød");
    }

    #[test]
    fn quote_for_shell_handles_single_quotes() {
        assert_eq!(
            quote_for_shell("O'Brien", QuoteStyle::Single),
            "'O'\\''Brien'"
        );
    }

    #[test]
    fn quote_for_shell_handles_double_quotes() {
        assert_eq!(
            quote_for_shell("a\"b$`c\\d", QuoteStyle::Double),
            "\"a\\\"b\\$\\`c\\\\d\""
        );
    }
}
