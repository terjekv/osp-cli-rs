//! Line input shared by command prompts and the REPL terminal protocol.

use std::collections::VecDeque;
use std::io::{self, Write};
use std::sync::{Mutex, PoisonError};

static PENDING_INPUT: Mutex<VecDeque<u8>> = Mutex::new(VecDeque::new());

#[cfg(unix)]
pub(crate) fn preserve_pending_input(bytes: &[u8]) {
    if !bytes.is_empty() {
        PENDING_INPUT
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .extend(bytes.iter().copied());
    }
}

/// Takes the next pending typed line for the rich editor, with its edits
/// applied and without its line break, and whether the user ended it with
/// Enter.
pub(crate) fn take_pending_line() -> Option<(String, bool)> {
    take_pending().filter(|(text, _)| !text.is_empty())
}

/// Takes the next pending typed line with its edits applied, and whether the
/// user ended it with Enter.
fn take_pending() -> Option<(String, bool)> {
    let mut pending = PENDING_INPUT.lock().unwrap_or_else(PoisonError::into_inner);
    if pending.is_empty() {
        return None;
    }
    let mut bytes = Vec::new();
    let mut terminated = false;
    while let Some(byte) = pending.pop_front() {
        if matches!(byte, b'\r' | b'\n') {
            if byte == b'\r' && pending.front() == Some(&b'\n') {
                pending.pop_front();
            }
            terminated = true;
            break;
        }
        bytes.push(byte);
    }
    Some((
        apply_line_edits(&String::from_utf8_lossy(&bytes)),
        terminated,
    ))
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
    if let Some((typed, terminated)) = take_pending() {
        let mut stdout = io::stdout().lock();
        stdout.write_all(typed.as_bytes())?;
        line.push_str(&typed);
        if terminated {
            stdout.write_all(b"\n")?;
            line.push('\n');
        }
        stdout.flush()?;
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
