#![allow(missing_docs)]

#[cfg(unix)]
use crate::support::{HANG_TIMEOUT, ReplPtyColorMode, ReplPtyConfig, ReplPtySession};

#[cfg(unix)]
fn spawn_repl_with_color() -> ReplPtySession {
    ReplPtySession::spawn(ReplPtyConfig::default().with_color_mode(ReplPtyColorMode::Always))
}

#[cfg(unix)]
#[test]
fn repl_highlights_commands_with_success_color() {
    if crate::isolation::isolated_test() {
        return;
    }
    let mut session = spawn_repl_with_color();

    let start = session.output_len();
    session.write_bytes(b"config");

    let expected = "\x1b[38;2;139;213;202mconfig";
    assert!(
        session.wait_for_output_since(start, expected, HANG_TIMEOUT),
        "expected success-colored command in output; output:\n{}",
        session.output_snapshot(2000),
    );

    session.write_bytes(b"\x03");
    session.write_bytes(b"exit\r\r");
    if !session.wait_for_exit(HANG_TIMEOUT) {
        session.kill();
    }
}

#[cfg(unix)]
#[test]
fn repl_help_history_highlights_help_keyword() {
    if crate::isolation::isolated_test() {
        return;
    }
    let mut session = spawn_repl_with_color();

    let start = session.output_len();
    session.write_bytes(b"help history");
    assert!(
        session.wait_for_output_since(start, "\x1b[38;2;139;213;202mhelp\x1b[0m", HANG_TIMEOUT),
        "expected `help` to be highlighted with the command color; output:\n{}",
        session.output_snapshot(4000),
    );

    session.write_bytes(b"\x03");
    session.write_bytes(b"exit\r\r");
    if !session.wait_for_exit(HANG_TIMEOUT) {
        session.kill();
    }
}
