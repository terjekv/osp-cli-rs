//! Line input shared by command prompts and the REPL terminal protocol.

use std::io::{self, IsTerminal, Write};

/// Read a response, excluding terminal cursor reports left by an editor query.
///
/// Only a well-formed CPR sequence is consumed; arbitrary escape sequences and
/// user text remain intact. Returns zero on EOF, like `Stdin::read_line`.
pub fn read_line(answer: &mut String) -> io::Result<usize> {
    let mut line = String::new();
    io::stdin().read_line(&mut line)?;
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
    let answer = read_text(&format!("{question} [y/N] "))?.unwrap_or_default();
    Ok(matches!(
        answer.trim().to_ascii_lowercase().as_str(),
        "y" | "yes"
    ))
}

/// Asks the reader to type a phrase exactly. EOF declines.
/// The caller must decide whether prompting is permitted and required.
pub fn confirm_phrase(phrase: &str) -> io::Result<bool> {
    let answer = read_text(&format!("Type '{phrase}' to confirm: "))?.unwrap_or_default();
    Ok(!answer.is_empty() && answer.trim() == phrase)
}

fn ask_line(question: &str) -> io::Result<Option<String>> {
    let mut stderr = io::stderr().lock();
    stderr.write_all(super::sanitize_human_text(question).as_bytes())?;
    stderr.flush()?;
    let mut answer = String::new();
    Ok((read_line(&mut answer)? != 0).then_some(answer))
}

fn require_interactive() -> io::Result<()> {
    if input_is_replay() || !io::stderr().is_terminal() {
        return Err(io::Error::other("prompt requires an interactive terminal"));
    }
    Ok(())
}

/// Asks a visible question, including any dialogue choices, without history.
/// Returns `None` on EOF and rejects replay or redirected input.
pub fn read_text(question: &str) -> io::Result<Option<String>> {
    require_interactive()?;
    ask_line(question)
}

/// Asks a hidden question without echo or history. EOF and interruption cancel.
/// Replay and redirected input are rejected before writing the question.
pub fn read_hidden(question: &str) -> io::Result<Option<String>> {
    require_interactive()?;
    match rpassword::prompt_password(super::sanitize_human_text(question)) {
        Ok(answer) => Ok(Some(answer)),
        Err(error)
            if matches!(
                error.kind(),
                io::ErrorKind::UnexpectedEof | io::ErrorKind::Interrupted
            ) =>
        {
            Ok(None)
        }
        Err(error) => Err(error),
    }
}

/// Reads a visible file path using the REPL editor and filesystem completion.
/// The standalone input has no history; Esc and Ctrl-C cancel.
pub fn read_path(label: &str) -> io::Result<Option<String>> {
    if input_is_replay() || !io::stdout().is_terminal() || !io::stderr().is_terminal() {
        return Err(io::Error::other(
            "file prompt requires an interactive terminal",
        ));
    }
    crate::repl::read_path(label)
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

/// Rings the terminal during an explicit interactive sound preview.
/// Notification policy and rate limiting remain owned by the REPL.
pub fn preview_bell() -> io::Result<()> {
    let mut stderr = io::stderr().lock();
    stderr.write_all(b"\x07")?;
    stderr.flush()
}
