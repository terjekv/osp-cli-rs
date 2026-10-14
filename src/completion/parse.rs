//! Shell-like tokenization and cursor analysis for completion.
//!
//! This module exists to turn a partially typed input line plus a cursor offset
//! into the structured data the completion engine actually needs: command path,
//! tail items, pipe mode, and the active replacement span.
//!
//! Contract:
//!
//! - parsing here stays permissive for interactive use
//! - the parser owns lexical structure, not suggestion ranking
//! - callers should rely on `ParsedCursorLine` and `CursorState` rather than
//!   re-deriving cursor spans themselves

use crate::completion::model::{CommandLine, CursorState, FlagOccurrence, ParsedLine, QuoteStyle};
use std::collections::BTreeMap;

/// Token value with byte offsets into the original input line.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TokenSpan {
    /// Unescaped token text.
    pub value: String,
    /// Inclusive start byte offset.
    pub start: usize,
    /// Exclusive end byte offset.
    pub end: usize,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum LexState {
    Normal,
    SingleQuote,
    DoubleQuote,
    EscapeNormal,
    EscapeDouble,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum LexEventKind {
    Separator,
    Pipe,
    QuoteStart,
    QuoteEnd,
    EscapeStart,
    Text(char),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum FlagArity {
    FlagOnly,
    SingleValue,
    MultipleValues,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct LexEvent {
    index: usize,
    end: usize,
    state_before: LexState,
    kind: LexEventKind,
}

fn scan_shell(input: &str, mut visit: impl FnMut(LexEvent)) -> LexState {
    let mut state = LexState::Normal;
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
        visit(LexEvent {
            index,
            end: index + ch.len_utf8(),
            state_before,
            kind,
        });
    }
    state
}

/// Parsed line assembly after tokenization.
///
/// The parser keeps the command head separate until it sees the first option-like
/// token. After that point the rest of the line is interpreted as flags, args,
/// or pipes. That mirrors how the completer reasons about scope: command path
/// first, then option/value mode.
#[derive(Debug, Default)]
struct ParseState {
    head: Vec<String>,
    tail: Vec<crate::completion::model::TailItem>,
    flag_values: BTreeMap<String, Vec<String>>,
    pipes: Vec<String>,
    has_pipe: bool,
}

impl ParseState {
    fn finish(self) -> CommandLine {
        CommandLine {
            head: self.head,
            tail: self.tail,
            flag_values: self.flag_values,
            pipes: self.pipes,
            has_pipe: self.has_pipe,
        }
    }

    fn start_pipe<'a>(&mut self, iter: &mut std::iter::Peekable<std::slice::Iter<'a, String>>) {
        self.has_pipe = true;
        self.pipes.extend(iter.cloned());
    }

    fn collect_positional_tail<'a>(
        &mut self,
        iter: &mut std::iter::Peekable<std::slice::Iter<'a, String>>,
    ) {
        while let Some(next) = iter.next() {
            if next == "|" {
                self.start_pipe(iter);
                break;
            }
            self.tail
                .push(crate::completion::model::TailItem::Positional(next.clone()));
        }
    }

    fn parse_flag_tail<'a>(
        &mut self,
        first_token: String,
        iter: &mut std::iter::Peekable<std::slice::Iter<'a, String>>,
        flag_arity: &impl Fn(&[String], &str) -> Option<FlagArity>,
    ) {
        // Once the parser has seen the first flag-like token, it keeps the
        // remaining command words in the tail. Known flag arity distinguishes
        // option values from positional words; unknown flags stay permissive.
        let mut current = first_token;
        loop {
            if current == "|" {
                self.start_pipe(iter);
                return;
            }

            if current == "--" {
                self.collect_positional_tail(iter);
                return;
            }

            if !looks_like_flag_start(&current) {
                self.tail
                    .push(crate::completion::model::TailItem::Positional(current));
                let Some(next) = iter.next().cloned() else {
                    break;
                };
                current = next;
                continue;
            }

            if let Some((flag, value)) = split_inline_flag_value(&current) {
                let mut occurrence_values = Vec::new();
                if !value.is_empty() {
                    self.flag_values
                        .entry(flag.clone())
                        .or_default()
                        .push(value.clone());
                    occurrence_values.push(value);
                } else {
                    self.flag_values.entry(flag.clone()).or_default();
                }
                self.tail
                    .push(crate::completion::model::TailItem::Flag(FlagOccurrence {
                        name: flag.clone(),
                        values: occurrence_values,
                    }));
                let Some(next) = iter.next().cloned() else {
                    break;
                };
                current = next;
                continue;
            }

            let flag = current;
            let value_limit = match flag_arity(&self.head, &flag) {
                Some(FlagArity::FlagOnly) => Some(0),
                Some(FlagArity::SingleValue) => Some(1),
                Some(FlagArity::MultipleValues) | None => None,
            };
            let values = self.consume_flag_values(iter, value_limit);
            self.tail
                .push(crate::completion::model::TailItem::Flag(FlagOccurrence {
                    name: flag.clone(),
                    values: values.clone(),
                }));
            self.flag_values
                .entry(flag.clone())
                .or_default()
                .extend(values);

            let Some(next) = iter.next().cloned() else {
                break;
            };
            current = next;
        }
    }

    fn consume_flag_values<'a>(
        &mut self,
        iter: &mut std::iter::Peekable<std::slice::Iter<'a, String>>,
        limit: Option<usize>,
    ) -> Vec<String> {
        let mut values = Vec::new();

        while let Some(next) = iter.peek() {
            if limit.is_some_and(|limit| values.len() >= limit) {
                break;
            }
            if *next == "|" || *next == "--" {
                break;
            }
            if looks_like_flag_start(next) {
                break;
            }

            values.push((*next).clone());
            iter.next();
        }

        values
    }
}

/// Shell-like parser used by the completion engine.
#[derive(Debug, Clone, Default)]
pub struct CommandLineParser;

/// Parsed command-line state for the full line and the cursor position.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParsedCursorLine {
    /// Parsed tokens and command structure.
    pub parsed: ParsedLine,
    /// Cursor-local replacement information.
    pub cursor: CursorState,
}

#[derive(Debug, Clone)]
struct CursorTokenization {
    full_tokens: Vec<String>,
    cursor_tokens: Vec<String>,
    cursor_state: CursorEditState,
}

#[derive(Debug, Clone)]
struct CursorEditState {
    token_stub: String,
    replace_start: usize,
    quote_style: Option<QuoteStyle>,
}

#[derive(Debug, Default)]
struct ActiveToken {
    value: String,
    start: Option<usize>,
    inline_value_start: Option<usize>,
    inline_value_offset: Option<usize>,
    quote_start: Option<usize>,
}

impl ActiveToken {
    fn reset(&mut self) {
        *self = Self::default();
    }

    fn observe(&mut self, event: LexEvent) {
        match event.kind {
            LexEventKind::Text(ch) => {
                self.start.get_or_insert(event.index);
                if ch == '='
                    && event.state_before == LexState::Normal
                    && self.value.starts_with("--")
                    && self.inline_value_start.is_none()
                {
                    self.inline_value_start = Some(event.end);
                    self.inline_value_offset = Some(self.value.len() + 1);
                }
                self.value.push(ch);
            }
            LexEventKind::QuoteStart => {
                self.start.get_or_insert(event.index);
                self.quote_start = Some(event.end);
            }
            LexEventKind::EscapeStart => {
                self.start.get_or_insert(event.index);
            }
            LexEventKind::Separator | LexEventKind::Pipe | LexEventKind::QuoteEnd => {}
        }
    }

    fn cursor_state(&self, safe_cursor: usize, state: LexState) -> CursorEditState {
        let quote_style = quote_style_for_state(state);
        let token_stub = if self.start.is_some() {
            self.inline_value_offset
                .and_then(|offset| self.value.get(offset..))
                .unwrap_or(&self.value)
                .to_string()
        } else {
            String::new()
        };
        let replace_start = self.start.map_or(safe_cursor, |start| {
            if quote_style.is_some() {
                self.quote_start.unwrap_or(start)
            } else {
                self.inline_value_start.unwrap_or(start)
            }
        });

        CursorEditState {
            token_stub,
            replace_start,
            quote_style,
        }
    }
}

impl CommandLineParser {
    /// Tokenizes a line using shell-like quoting rules.
    ///
    /// Tokenization is intentionally permissive for interactive use. If the
    /// user is mid-quote while pressing tab, we retry with a synthetic closing
    /// quote before finally falling back to whitespace splitting.
    ///
    /// # Examples
    ///
    /// ```
    /// use osp_cli::completion::CommandLineParser;
    ///
    /// let parser = CommandLineParser;
    ///
    /// assert_eq!(
    ///     parser.tokenize(r#"ldap user "alice smith""#),
    ///     vec!["ldap", "user", "alice smith"]
    /// );
    /// ```
    pub fn tokenize(&self, line: &str) -> Vec<String> {
        self.tokenize_inner(line)
            .or_else(|| self.tokenize_inner(&format!("{line}\"")))
            .or_else(|| self.tokenize_inner(&format!("{line}'")))
            .unwrap_or_else(|| line.split_whitespace().map(str::to_string).collect())
    }

    /// Tokenizes `line` and preserves byte spans for each token when possible.
    ///
    /// # Examples
    ///
    /// ```
    /// use osp_cli::completion::CommandLineParser;
    ///
    /// let spans = CommandLineParser.tokenize_with_spans("ldap user alice");
    ///
    /// assert_eq!(spans[0].value, "ldap");
    /// assert_eq!(spans[1].start, 5);
    /// assert_eq!(spans[2].end, 15);
    /// ```
    pub fn tokenize_with_spans(&self, line: &str) -> Vec<TokenSpan> {
        self.tokenize_with_spans_inner(line)
            .or_else(|| self.tokenize_with_spans_fallback(line))
            .unwrap_or_default()
    }

    /// Parse the full line and the cursor-local prefix from one lexical walk.
    ///
    /// The common case keeps completion analysis in one tokenization pass. If
    /// the line ends in an unmatched quote we fall back to the permissive
    /// tokenization path so interactive behavior stays unchanged.
    ///
    /// `cursor` is clamped to the input length and to a valid UTF-8 character
    /// boundary before the parser slices the line.
    ///
    /// # Examples
    ///
    /// ```
    /// use osp_cli::completion::CommandLineParser;
    ///
    /// let parsed = CommandLineParser.analyze("ldap user ali", 13);
    ///
    /// assert_eq!(parsed.parsed.cursor_tokens, vec!["ldap", "user", "ali"]);
    /// assert_eq!(parsed.cursor.token_stub, "ali");
    /// ```
    pub fn analyze(&self, line: &str, cursor: usize) -> ParsedCursorLine {
        self.analyze_with_flag_arity(line, cursor, |_, _| None)
    }

    pub(crate) fn analyze_with_flag_arity(
        &self,
        line: &str,
        cursor: usize,
        flag_arity: impl Fn(&[String], &str) -> Option<FlagArity>,
    ) -> ParsedCursorLine {
        let safe_cursor = clamp_to_char_boundary(line, cursor.min(line.len()));
        let before_cursor = &line[..safe_cursor];
        let lexical = self.lex_cursor_line(line, before_cursor, safe_cursor);
        self.assemble_parsed_cursor_line(before_cursor, safe_cursor, lexical, &flag_arity)
    }

    fn tokenize_inner(&self, line: &str) -> Option<Vec<String>> {
        let mut out = Vec::new();
        let mut current = String::new();

        let state = scan_shell(line, |event| match event.kind {
            LexEventKind::Separator => push_current(&mut out, &mut current),
            LexEventKind::Pipe => {
                push_current(&mut out, &mut current);
                out.push("|".to_string());
            }
            LexEventKind::Text(ch) => current.push(ch),
            LexEventKind::QuoteStart | LexEventKind::QuoteEnd | LexEventKind::EscapeStart => {}
        });

        if state == LexState::Normal {
            push_current(&mut out, &mut current);
            Some(out)
        } else {
            None
        }
    }

    fn tokenize_with_spans_inner(&self, line: &str) -> Option<Vec<TokenSpan>> {
        let mut out = Vec::new();
        let mut current = String::new();
        let mut current_start = None;

        let state = scan_shell(line, |event| match event.kind {
            LexEventKind::Separator => {
                push_current_span(&mut out, &mut current, &mut current_start, event.index)
            }
            LexEventKind::Pipe => {
                push_current_span(&mut out, &mut current, &mut current_start, event.index);
                out.push(TokenSpan {
                    value: "|".to_string(),
                    start: event.index,
                    end: event.end,
                });
            }
            LexEventKind::Text(ch) => {
                current_start.get_or_insert(event.index);
                current.push(ch);
            }
            LexEventKind::QuoteStart | LexEventKind::EscapeStart => {
                current_start.get_or_insert(event.index);
            }
            LexEventKind::QuoteEnd => {}
        });

        if state == LexState::Normal {
            push_current_span(&mut out, &mut current, &mut current_start, line.len());
            Some(out)
        } else {
            None
        }
    }

    fn tokenize_with_spans_fallback(&self, line: &str) -> Option<Vec<TokenSpan>> {
        let mut out = Vec::new();
        let mut search_from = 0usize;
        for token in line.split_whitespace() {
            let rel = line.get(search_from..)?.find(token)?;
            let start = search_from + rel;
            let end = start + token.len();
            out.push(TokenSpan {
                value: token.to_string(),
                start,
                end,
            });
            search_from = end;
        }
        Some(out)
    }

    /// Parses tokens into command-path, flag, positional, and pipe segments.
    ///
    /// # Examples
    ///
    /// ```
    /// use osp_cli::completion::CommandLineParser;
    ///
    /// let tokens = vec![
    ///     "ldap".to_string(),
    ///     "user".to_string(),
    ///     "--json".to_string(),
    ///     "|".to_string(),
    ///     "P".to_string(),
    /// ];
    /// let parsed = CommandLineParser.parse(&tokens);
    ///
    /// assert_eq!(parsed.head(), &["ldap".to_string(), "user".to_string()]);
    /// assert!(parsed.has_pipe());
    /// ```
    pub fn parse(&self, tokens: &[String]) -> CommandLine {
        self.parse_with_flag_arity(tokens, &|_, _| None)
    }

    fn parse_with_flag_arity(
        &self,
        tokens: &[String],
        flag_arity: &impl Fn(&[String], &str) -> Option<FlagArity>,
    ) -> CommandLine {
        let mut state = ParseState::default();
        let mut iter = tokens.iter().peekable();

        while let Some(token) = iter.next() {
            if token == "|" {
                state.start_pipe(&mut iter);
                return state.finish();
            }
            if token == "--" {
                state.collect_positional_tail(&mut iter);
                return state.finish();
            }
            if looks_like_flag_start(token) {
                state.parse_flag_tail(token.clone(), &mut iter, flag_arity);
                return state.finish();
            }
            state.head.push(token.clone());
        }

        state.finish()
    }

    /// Computes the cursor replacement range and current token stub.
    ///
    /// # Examples
    ///
    /// ```
    /// use osp_cli::completion::CommandLineParser;
    ///
    /// let cursor = CommandLineParser.cursor_state("ldap user ali", 13);
    ///
    /// assert_eq!(cursor.token_stub, "ali");
    /// assert_eq!(cursor.replace_range, 10..13);
    /// ```
    pub fn cursor_state(&self, text_before_cursor: &str, safe_cursor: usize) -> CursorState {
        let safe_cursor = clamp_to_char_boundary(
            text_before_cursor,
            safe_cursor.min(text_before_cursor.len()),
        );
        let prefix = text_before_cursor.get(..safe_cursor).unwrap_or("");
        let mut active = ActiveToken::default();
        let state = scan_shell(prefix, |event| match event.kind {
            LexEventKind::Separator | LexEventKind::Pipe => active.reset(),
            LexEventKind::Text(_)
            | LexEventKind::QuoteStart
            | LexEventKind::QuoteEnd
            | LexEventKind::EscapeStart => active.observe(event),
        });
        self.build_cursor_state(prefix, safe_cursor, active.cursor_state(safe_cursor, state))
    }

    fn build_cursor_state(
        &self,
        text_before_cursor: &str,
        safe_cursor: usize,
        cursor_state: CursorEditState,
    ) -> CursorState {
        let replace_start = cursor_state.replace_start;
        let raw_stub = text_before_cursor
            .get(replace_start..safe_cursor)
            .unwrap_or("")
            .to_string();

        CursorState::new(
            cursor_state.token_stub,
            raw_stub,
            replace_start..safe_cursor,
            cursor_state.quote_style,
        )
    }

    fn tokenize_with_cursor_inner(
        &self,
        line: &str,
        safe_cursor: usize,
    ) -> Option<CursorTokenization> {
        let mut out = Vec::new();
        let mut active = ActiveToken::default();
        let mut cursor_tokens = None;
        let mut cursor_state = None;

        let state = scan_shell(line, |event| {
            if event.index == safe_cursor && cursor_tokens.is_none() {
                cursor_tokens = Some(snapshot_tokens(&out, &active.value));
                cursor_state = Some(active.cursor_state(safe_cursor, event.state_before));
            }

            match event.kind {
                LexEventKind::Separator => {
                    push_current(&mut out, &mut active.value);
                    active.reset();
                }
                LexEventKind::Pipe => {
                    push_current(&mut out, &mut active.value);
                    out.push("|".to_string());
                    active.reset();
                }
                LexEventKind::Text(_)
                | LexEventKind::QuoteStart
                | LexEventKind::QuoteEnd
                | LexEventKind::EscapeStart => active.observe(event),
            }
        });

        if safe_cursor == line.len() && cursor_tokens.is_none() {
            cursor_tokens = Some(snapshot_tokens(&out, &active.value));
            cursor_state = Some(active.cursor_state(safe_cursor, state));
        }

        match state {
            LexState::Normal => {
                push_current(&mut out, &mut active.value);
                Some(CursorTokenization {
                    full_tokens: out,
                    cursor_tokens: cursor_tokens?,
                    cursor_state: cursor_state?,
                })
            }
            _ => None,
        }
    }

    /// Returns the active quote style for the token being edited, if any.
    ///
    /// # Examples
    ///
    /// ```
    /// use osp_cli::completion::{CommandLineParser, QuoteStyle};
    ///
    /// assert_eq!(
    ///     CommandLineParser.compute_stub_quote(r#"ldap user "ali"#),
    ///     Some(QuoteStyle::Double)
    /// );
    /// ```
    pub fn compute_stub_quote(&self, text_before_cursor: &str) -> Option<QuoteStyle> {
        current_quote_state(text_before_cursor)
    }

    fn lex_cursor_line(
        &self,
        line: &str,
        before_cursor: &str,
        safe_cursor: usize,
    ) -> CursorLexicalState {
        match self.tokenize_with_cursor_inner(line, safe_cursor) {
            Some(tokenized) => CursorLexicalState::Structured(tokenized),
            None => CursorLexicalState::Fallback {
                full_tokens: self.tokenize(line),
                cursor_tokens: self.tokenize(before_cursor),
            },
        }
    }

    fn assemble_parsed_cursor_line(
        &self,
        before_cursor: &str,
        safe_cursor: usize,
        lexical: CursorLexicalState,
        flag_arity: &impl Fn(&[String], &str) -> Option<FlagArity>,
    ) -> ParsedCursorLine {
        match lexical {
            CursorLexicalState::Structured(tokenized) => {
                let full_cmd = self.parse_with_flag_arity(&tokenized.full_tokens, flag_arity);
                let cursor_cmd = self.parse_with_flag_arity(&tokenized.cursor_tokens, flag_arity);
                let cursor =
                    self.build_cursor_state(before_cursor, safe_cursor, tokenized.cursor_state);

                ParsedCursorLine {
                    parsed: ParsedLine {
                        safe_cursor,
                        full_tokens: tokenized.full_tokens,
                        cursor_tokens: tokenized.cursor_tokens,
                        full_cmd,
                        cursor_cmd,
                    },
                    cursor,
                }
            }
            CursorLexicalState::Fallback {
                full_tokens,
                cursor_tokens,
            } => {
                let full_cmd = self.parse_with_flag_arity(&full_tokens, flag_arity);
                let cursor_cmd = self.parse_with_flag_arity(&cursor_tokens, flag_arity);
                let cursor = self.cursor_state(before_cursor, safe_cursor);

                ParsedCursorLine {
                    parsed: ParsedLine {
                        safe_cursor,
                        full_tokens,
                        cursor_tokens,
                        full_cmd,
                        cursor_cmd,
                    },
                    cursor,
                }
            }
        }
    }
}

enum CursorLexicalState {
    Structured(CursorTokenization),
    Fallback {
        full_tokens: Vec<String>,
        cursor_tokens: Vec<String>,
    },
}

fn snapshot_tokens(out: &[String], current: &str) -> Vec<String> {
    let mut tokens = out.to_vec();
    if !current.is_empty() {
        tokens.push(current.to_string());
    }
    tokens
}

fn clamp_to_char_boundary(input: &str, cursor: usize) -> usize {
    if input.is_char_boundary(cursor) {
        return cursor;
    }
    let mut safe = cursor;
    while safe > 0 && !input.is_char_boundary(safe) {
        safe -= 1;
    }
    safe
}

fn quote_style_for_state(state: LexState) -> Option<QuoteStyle> {
    match state {
        LexState::SingleQuote => Some(QuoteStyle::Single),
        LexState::DoubleQuote | LexState::EscapeDouble => Some(QuoteStyle::Double),
        LexState::Normal | LexState::EscapeNormal => None,
    }
}

fn split_inline_flag_value(token: &str) -> Option<(String, String)> {
    if !token.starts_with("--") || !token.contains('=') {
        return None;
    }

    let mut split = token.splitn(2, '=');
    let flag = split.next().unwrap_or_default().to_string();
    let value = split.next().unwrap_or_default().to_string();
    Some((flag, value))
}

fn push_current(out: &mut Vec<String>, current: &mut String) {
    if !current.is_empty() {
        out.push(std::mem::take(current));
    }
}

fn push_current_span(
    out: &mut Vec<TokenSpan>,
    current: &mut String,
    current_start: &mut Option<usize>,
    end: usize,
) {
    if !current.is_empty() {
        out.push(TokenSpan {
            value: std::mem::take(current),
            start: current_start.take().unwrap_or(end),
            end,
        });
    } else {
        *current_start = None;
    }
}

fn looks_like_flag_start(token: &str) -> bool {
    token.starts_with('-') && token != "-" && !is_number(token)
}

fn is_number(text: &str) -> bool {
    text.parse::<f64>().is_ok()
}

fn current_quote_state(text: &str) -> Option<QuoteStyle> {
    quote_style_for_state(scan_shell(text, |_| {}))
}

#[cfg(test)]
mod tests {
    use crate::completion::model::{FlagOccurrence, QuoteStyle};

    use super::CommandLineParser;

    fn parser() -> CommandLineParser {
        CommandLineParser
    }

    mod scanner_contracts {
        use super::*;

        #[test]
        fn scanner_preserves_token_values_offsets_and_unmatched_quote_recovery() {
            let parser = parser();

            assert_eq!(
                parser.tokenize("service deploy --request 'name=a|b' | F name"),
                vec![
                    "service",
                    "deploy",
                    "--request",
                    "name=a|b",
                    "|",
                    "F",
                    "name",
                ]
            );
            assert_eq!(parser.tokenize("--image 'small"), vec!["--image", "small"]);

            let spans = parser.tokenize_with_spans("cmd --name 'alice");
            assert_eq!(spans.len(), 3);
            assert_eq!(spans[0].value, "cmd");
            assert_eq!(spans[1].value, "--name");
            assert_eq!(spans[2].value, "'alice");
            let source = r#"ldap user "alice smith" | P uid"#;
            let spans = parser.tokenize_with_spans(source);

            assert_eq!(spans[0].value, "ldap");
            assert_eq!(spans[0].start, 0);
            assert_eq!(spans[2].value, "alice smith");
            assert_eq!(&source[spans[2].start..spans[2].end], "\"alice smith\"");
            assert_eq!(spans[3].value, "|");
        }

        #[test]
        fn completion_lexical_views_share_quote_escape_pipe_and_unicode_contract() {
            let parser = parser();
            for line in [
                r#"service deploy --request 'name=a|b' | F name"#,
                r#"cmd --name "alice \"ops\"" --city Oslo"#,
                "ldap user 'Øistein Søvik' | P uid,cn",
                r#"cmd escaped\ value --path "a\\b""#,
            ] {
                let tokens = parser.tokenize(line);
                let span_values = parser
                    .tokenize_with_spans(line)
                    .into_iter()
                    .map(|token| token.value)
                    .collect::<Vec<_>>();
                let analyzed = parser.analyze(line, line.len());

                assert_eq!(span_values, tokens, "span token drift for {line:?}");
                assert_eq!(
                    analyzed.parsed.full_tokens, tokens,
                    "full token drift for {line:?}"
                );
                assert_eq!(
                    analyzed.parsed.cursor_tokens, tokens,
                    "cursor token drift for {line:?}"
                );
                assert_eq!(
                    analyzed.cursor.quote_style, None,
                    "quote drift for {line:?}"
                );
            }
        }
    }

    mod command_shape_contracts {
        use super::*;

        #[test]
        fn parse_tracks_flag_values_pipes_and_repeated_occurrence_boundaries() {
            let parser = parser();

            let tokens = parser.tokenize("service deploy --provider alpha --image red | F name");
            let cmd = parser.parse(&tokens);
            assert_eq!(cmd.head(), ["service".to_string(), "deploy".to_string()]);
            assert_eq!(
                cmd.flag_values("--provider"),
                Some(&["alpha".to_string()][..])
            );
            assert_eq!(cmd.flag_values("--image"), Some(&["red".to_string()][..]));
            assert!(cmd.has_pipe());
            assert_eq!(cmd.pipes(), ["F".to_string(), "name".to_string()]);

            let repeated = parser.parse(&parser.tokenize("cmd --tag red --mode fast --tag blue"));
            assert_eq!(
                repeated.flag_occurrences().cloned().collect::<Vec<_>>(),
                vec![
                    FlagOccurrence {
                        name: "--tag".to_string(),
                        values: vec!["red".to_string()],
                    },
                    FlagOccurrence {
                        name: "--mode".to_string(),
                        values: vec!["fast".to_string()],
                    },
                    FlagOccurrence {
                        name: "--tag".to_string(),
                        values: vec!["blue".to_string()],
                    },
                ]
            );
        }

        #[test]
        fn parse_respects_option_boundaries_inline_values_and_negative_numbers() {
            let parser = parser();

            let after_double_dash = parser.parse(&parser.tokenize("cmd -- --not-a-flag"));
            assert_eq!(after_double_dash.head(), ["cmd".to_string()]);
            assert_eq!(
                after_double_dash
                    .positional_args()
                    .cloned()
                    .collect::<Vec<_>>(),
                vec!["--not-a-flag".to_string()]
            );

            let negative_value = parser.parse(&parser.tokenize("cmd --count -5"));
            assert_eq!(
                negative_value.flag_values("--count"),
                Some(&["-5".to_string()][..])
            );

            let inline =
                parser.parse(&parser.tokenize("cmd --format=json --image= --format=table"));
            assert_eq!(inline.flag_values("--image"), Some(&[][..]));
            assert_eq!(
                inline.flag_occurrences().cloned().collect::<Vec<_>>(),
                vec![
                    FlagOccurrence {
                        name: "--format".to_string(),
                        values: vec!["json".to_string()],
                    },
                    FlagOccurrence {
                        name: "--image".to_string(),
                        values: vec![],
                    },
                    FlagOccurrence {
                        name: "--format".to_string(),
                        values: vec!["table".to_string()],
                    },
                ]
            );
        }

        #[test]
        fn parse_distinguishes_tail_mode_from_dsl_boundaries() {
            let parser = parser();

            let tail =
                parser.parse(&parser.tokenize("directory user --provider alpha region north"));
            assert_eq!(tail.head(), ["directory".to_string(), "user".to_string()]);
            assert_eq!(
                tail.flag_values("--provider"),
                Some(
                    &[
                        "alpha".to_string(),
                        "region".to_string(),
                        "north".to_string(),
                    ][..]
                )
            );

            let dsl = parser.parse(&parser.tokenize("cmd -- literal | F name"));
            assert_eq!(dsl.head(), ["cmd".to_string()]);
            assert_eq!(
                dsl.positional_args().cloned().collect::<Vec<_>>(),
                vec!["literal".to_string()]
            );
            assert!(dsl.has_pipe());
            assert_eq!(dsl.pipes(), ["F".to_string(), "name".to_string()]);
        }
    }

    mod cursor_analysis_contracts {
        use super::*;

        #[test]
        fn cursor_state_tracks_equals_boundaries_and_open_quote_ranges() {
            let parser = parser();

            let cursor = parser.cursor_state("cmd --flag=", "cmd --flag=".len());
            assert_eq!(cursor.token_stub, "");

            assert_eq!(
                parser.compute_stub_quote("cmd --name \"al"),
                Some(QuoteStyle::Double)
            );
            assert_eq!(
                parser.compute_stub_quote("cmd --name 'al"),
                Some(QuoteStyle::Single)
            );
            assert_eq!(parser.compute_stub_quote("cmd --name al"), None);

            let line = "ldap user \"oi";
            let cursor = parser.cursor_state(line, line.len());
            assert_eq!(cursor.token_stub, "oi");
            assert_eq!(cursor.raw_stub, "oi");
            assert_eq!(cursor.replace_range, 11..13);
            assert_eq!(cursor.quote_style, Some(QuoteStyle::Double));
        }

        #[test]
        fn analyze_reuses_safe_cursor_snapshots_for_prefix_and_balanced_quotes() {
            let parser = parser();

            let line = "service deploy --provider alpha --image red | F name";
            let cursor = "service deploy --provider alpha".len();
            let analyzed = parser.analyze(line, cursor);
            assert_eq!(
                analyzed.parsed.full_tokens,
                vec![
                    "service",
                    "deploy",
                    "--provider",
                    "alpha",
                    "--image",
                    "red",
                    "|",
                    "F",
                    "name",
                ]
            );
            assert_eq!(
                analyzed.parsed.cursor_tokens,
                vec!["service", "deploy", "--provider", "alpha"]
            );
            assert_eq!(
                analyzed.parsed.cursor_cmd.flag_values("--provider"),
                Some(&["alpha".to_string()][..])
            );

            let balanced = parser.analyze(
                r#"ldap user "oi ste" --format json"#,
                r#"ldap user "oi"#.len(),
            );
            assert_eq!(balanced.cursor.token_stub, "oi");
            assert_eq!(balanced.cursor.raw_stub, "oi");
            assert_eq!(balanced.cursor.quote_style, Some(QuoteStyle::Double));
        }

        #[test]
        fn analyze_recovers_from_unbalanced_quotes_and_non_char_boundaries() {
            let parser = parser();

            let unbalanced = parser.analyze(r#"ldap user "alice"#, r#"ldap user "alice"#.len());
            assert_eq!(unbalanced.parsed.full_tokens, vec!["ldap", "user", "alice"]);
            assert_eq!(
                unbalanced.parsed.cursor_tokens,
                vec!["ldap", "user", "alice"]
            );
            assert_eq!(unbalanced.cursor.quote_style, Some(QuoteStyle::Double));
            assert_eq!(unbalanced.cursor.token_stub, "alice");

            let line = "ldap user å";
            let analyzed = parser.analyze(line, line.len() - 1);
            assert!(analyzed.parsed.safe_cursor < line.len());
            assert_eq!(analyzed.cursor.token_stub, "");
        }
    }
}
