//! Line input shared by command prompts and the REPL terminal protocol.

use std::collections::VecDeque;
use std::io::{self, IsTerminal, Write};
use std::sync::{Mutex, PoisonError};

static PENDING_INPUT: Mutex<VecDeque<u8>> = Mutex::new(VecDeque::new());

pub(crate) enum PendingLine {
    Typed { text: String, terminated: bool },
    Eof,
}

#[cfg(unix)]
pub(crate) fn preserve_pending_input(bytes: &[u8]) {
    if !bytes.is_empty() {
        PENDING_INPUT
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .extend(bytes.iter().copied());
    }
}

/// Takes pending input for the rich editor, preserving EOF on an empty line.
/// Typed lines have their edits applied and their line break removed.
pub(crate) fn take_pending_line() -> Option<PendingLine> {
    take_pending().filter(|line| match line {
        PendingLine::Typed { text, .. } => !text.is_empty(),
        PendingLine::Eof => true,
    })
}

/// Takes the next pending line or EOF, with typed line edits applied.
fn take_pending() -> Option<PendingLine> {
    let mut pending = PENDING_INPUT.lock().unwrap_or_else(PoisonError::into_inner);
    if pending.is_empty() {
        return None;
    }
    let mut bytes = Vec::new();
    let mut terminated = false;
    while let Some(byte) = pending.pop_front() {
        if byte == b'\x04' && apply_line_edits(&String::from_utf8_lossy(&bytes)).is_empty() {
            return Some(PendingLine::Eof);
        }
        if matches!(byte, b'\r' | b'\n') {
            if byte == b'\r' && pending.front() == Some(&b'\n') {
                pending.pop_front();
            }
            terminated = true;
            break;
        }
        bytes.push(byte);
    }
    Some(PendingLine::Typed {
        text: apply_line_edits(&String::from_utf8_lossy(&bytes)),
        terminated,
    })
}

/// Applies the keys a terminal line editor would: Backspace deletes, Ctrl-U
/// clears, and escape sequences such as arrow keys are dropped.
fn apply_line_edits(raw: &str) -> String {
    let mut out = String::new();
    let mut chars = raw.chars();
    while let Some(ch) = chars.next() {
        match ch {
            '\u{7f}' | '\u{8}' => {
                out.pop();
            }
            '\u{15}' => out.clear(),
            '\u{1b}' => match chars.next() {
                Some('[') => {
                    for ch in chars.by_ref() {
                        if ('@'..='~').contains(&ch) {
                            break;
                        }
                    }
                }
                Some('O') => {
                    chars.next();
                }
                _ => {}
            },
            ch if ch.is_control() => {}
            ch => out.push(ch),
        }
    }
    out
}

/// Read a response, excluding terminal cursor reports left by an editor query.
///
/// Only a well-formed CPR sequence is consumed; arbitrary escape sequences and
/// user text remain intact. Keys typed before the prompt appeared are echoed
/// and continue the line. Returns zero on EOF, like `Stdin::read_line`.
pub fn read_line(answer: &mut String) -> io::Result<usize> {
    let mut line = String::new();
    match take_pending() {
        Some(PendingLine::Eof) => return Ok(0),
        Some(PendingLine::Typed { text, terminated }) => {
            let mut stdout = io::stdout().lock();
            stdout.write_all(text.as_bytes())?;
            line.push_str(&text);
            if terminated {
                stdout.write_all(b"\n")?;
                line.push('\n');
            }
            stdout.flush()?;
        }
        None => {}
    }
    if !line.ends_with('\n') {
        io::stdin().read_line(&mut line)?;
    }
    let count = line.len();
    let mut rest = line.as_str();
    while let Some(start) = rest.find("\x1b[") {
        answer.push_str(&rest[..start]);
        rest = &rest[start..];
        if let Some(end) =
            parse_cursor_position_report(rest.as_bytes()).and_then(|_| rest.find('R'))
        {
            rest = &rest[end + 1..];
        } else {
            answer.push('\x1b');
            rest = &rest[1..];
        }
    }
    answer.push_str(rest);
    Ok(count)
}

pub(crate) fn parse_cursor_position_report(bytes: &[u8]) -> Option<(u16, u16)> {
    let rest = bytes.strip_prefix(b"\x1b[")?;
    let row_end = rest.iter().position(|byte| !byte.is_ascii_digit())?;
    if row_end == 0 || *rest.get(row_end)? != b';' {
        return None;
    }
    let row = std::str::from_utf8(&rest[..row_end])
        .ok()?
        .parse::<u16>()
        .ok()?;
    let col_rest = &rest[row_end + 1..];
    let col_end = col_rest.iter().position(|byte| !byte.is_ascii_digit())?;
    if col_end == 0 || *col_rest.get(col_end)? != b'R' {
        return None;
    }
    let col = std::str::from_utf8(&col_rest[..col_end])
        .ok()?
        .parse::<u16>()
        .ok()?;
    Some((col, row))
}

/// Writes a yes/no question to stderr and reads stdin. Only `y` or `yes`
/// (case insensitive) confirms; EOF declines. The caller owns consent policy.
pub fn confirm(question: &str) -> io::Result<bool> {
    let answer = ask(&format!("{question} [y/N] "))?;
    Ok(matches!(
        answer.trim().to_ascii_lowercase().as_str(),
        "y" | "yes"
    ))
}

/// Asks the reader to type a phrase exactly. EOF declines.
/// The caller must decide whether prompting is permitted and required.
pub fn confirm_phrase(phrase: &str) -> io::Result<bool> {
    let answer = ask(&format!("Type '{phrase}' to confirm: "))?;
    Ok(!answer.is_empty() && answer.trim() == phrase)
}

fn ask(question: &str) -> io::Result<String> {
    let mut stderr = io::stderr().lock();
    stderr.write_all(question.as_bytes())?;
    stderr.flush()?;
    let mut answer = String::new();
    read_line(&mut answer)?;
    Ok(answer)
}

thread_local! { static REPLAY: std::cell::Cell<bool> = const { std::cell::Cell::new(false) }; }

/// Whether input comes from replay or redirected stdin rather than a person.
pub fn input_is_replay() -> bool {
    REPLAY.with(std::cell::Cell::get) || !io::stdin().is_terminal()
}

pub(crate) fn replay<R>(run: impl FnOnce() -> R) -> R {
    struct Restore(bool);
    impl Drop for Restore {
        fn drop(&mut self) {
            REPLAY.with(|active| active.set(self.0));
        }
    }
    let _restore = Restore(REPLAY.with(|active| active.replace(true)));
    run()
}
