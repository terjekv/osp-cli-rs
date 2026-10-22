#![allow(missing_docs)]

#[cfg(unix)]
use crate::support::{HANG_TIMEOUT, ReplPtyConfig, ReplPtySession};

#[cfg(unix)]
type PtySession = ReplPtySession;

#[cfg(unix)]
fn spawn_repl() -> PtySession {
    let session = ReplPtySession::spawn(
        ReplPtyConfig::default().with_config("[default]\nui.format = \"json\"\n"),
    );
    assert!(
        session.wait_for_output_since(0, "\x1b[?25h", HANG_TIMEOUT),
        "expected editor startup repaint; output:\n{}",
        session.output_snapshot(4000),
    );
    session
}

#[cfg(unix)]
fn json_result(session: &ReplPtySession, start: usize) -> serde_json::Value {
    assert!(
        session.wait_for_plain_output_since(start, "} ]", HANG_TIMEOUT),
        "expected completed JSON command result; output:\n{}",
        session.output_snapshot(8000),
    );
    let output = crate::support::strip_ansi_preserve_newlines(&session.output_since(start));
    let json_start = output
        .find("[\n")
        .expect("JSON array should start on its own line");
    serde_json::Deserializer::from_str(&output[json_start..])
        .into_iter::<serde_json::Value>()
        .next()
        .expect("JSON result should exist")
        .expect("command result should parse")
}

#[cfg(unix)]
#[test]
fn repl_tab_completes_single_match_and_exits() {
    if crate::isolation::isolated_test() {
        return;
    }
    let mut session = spawn_repl();

    session.type_text("ex");
    session.write_bytes(b"\t");
    session.write_bytes(b"\t");
    assert!(
        session.wait_for_plain_output("exit Exit REPL", HANG_TIMEOUT),
        "expected tab completion to render `exit` with its status hint; output:\n{}",
        session.plain_output_snapshot(4000),
    );
    session.write_bytes(b"\r");
    session.write_bytes(b"\r");

    if !session.wait_for_exit(HANG_TIMEOUT) {
        session.kill();
        panic!("expected repl to exit after completion");
    }
    assert!(
        !session
            .plain_output_snapshot(4000)
            .contains("unrecognized subcommand"),
        "expected tab completion to turn `ex` into `exit`; output:\n{}",
        session.plain_output_snapshot(4000),
    );
}

#[cfg(unix)]
#[test]
fn repl_tab_accepts_single_visible_completion() {
    if crate::isolation::isolated_test() {
        return;
    }
    let mut session = spawn_repl();

    session.type_text("the");
    let menu = session.output_len();
    session.write_bytes(b"\t");
    assert!(
        session.wait_for_plain_output_since(menu, "theme", HANG_TIMEOUT),
        "expected the single theme completion",
    );
    let selected = session.output_len();
    session.write_bytes(b"\t");
    assert!(
        session.wait_for_output_since(selected, "\x1b[?25h", HANG_TIMEOUT),
        "expected selected completion to repaint the input",
    );
    let accepted = session.output_len();
    session.write_bytes(b"\r");
    assert!(
        session.wait_for_output_since(accepted, "\x1b[?25h", HANG_TIMEOUT),
        "expected accepted completion to repaint the input",
    );
    session.type_text(" list | dracula");
    let start = session.output_len();
    session.write_bytes(b"\r");
    let result = json_result(&session, start);
    assert_eq!(result[0]["id"], "dracula");
    assert_eq!(result[0]["name"], "Dracula");

    session.write_bytes(b"exit\r\r");
    assert!(session.wait_for_exit(HANG_TIMEOUT), "REPL should exit");
}

#[cfg(unix)]
#[test]
fn repl_theme_use_tab_cycles_visible_suggestion_into_prompt_end_to_end() {
    if crate::isolation::isolated_test() {
        return;
    }
    let mut session = spawn_repl();

    session.type_text("theme use ");

    let start = session.output_len();
    session.write_bytes(b"\t");
    assert!(
        session.wait_for_plain_output_since(start, "catppuccin", HANG_TIMEOUT),
        "expected visible theme completion menu for `theme use `; output:\n{}",
        session.plain_output_snapshot(8000),
    );

    let mut selections = Vec::new();
    for _ in 0..2 {
        let start = session.output_len();
        session.write_bytes(b"\t");
        assert!(
            session.wait_for_output_since(start, "\x1b[?25h", HANG_TIMEOUT),
            "expected completion selection repaint; output:\n{}",
            session.output_snapshot(8000),
        );
        let output = crate::support::strip_terminal_noise(&session.output_since(start));
        let selected = output
            .rsplit_once("theme use ")
            .expect("selected theme should appear in the input")
            .1
            .split_whitespace()
            .next()
            .expect("theme selection should not be empty")
            .to_string();
        selections.push(selected);
    }
    assert_ne!(
        selections[0], selections[1],
        "Tab should move the selection"
    );

    let accepted = session.output_len();
    session.write_bytes(b"\r");
    assert!(
        session.wait_for_output_since(accepted, "\x1b[?25h", HANG_TIMEOUT),
        "expected accepted completion to repaint the input",
    );
    let start = session.output_len();
    session.write_bytes(b"\r");
    assert!(session.wait_for_plain_output_since(
        start,
        &format!("active theme set to: {}", selections[1]),
        HANG_TIMEOUT
    ));
    assert!(session.wait_for_editor_since(start, HANG_TIMEOUT));

    session.write_bytes(b"exit\r\r");
    assert!(session.wait_for_exit(HANG_TIMEOUT), "REPL should exit");
}

#[cfg(unix)]
#[test]
fn repl_close_menu_keeps_typed_input() {
    if crate::isolation::isolated_test() {
        return;
    }
    let mut session = spawn_repl();

    let start = session.output_len();
    session.write_bytes(b"\t");
    assert!(
        session.wait_for_plain_output_since(start, "help", HANG_TIMEOUT),
        "expected visible root completion menu; output:\n{}",
        session.plain_output_snapshot(2000),
    );

    let start = session.output_len();
    session.write_bytes(b"\x1b");
    assert!(
        session.wait_for_output_since(start, "\x1b[?25h", HANG_TIMEOUT),
        "expected menu to close before typing; output:\n{}",
        session.plain_output_snapshot(2000),
    );

    session.type_text("co");
    session.type_text("nfig get theme.name");
    let start = session.output_len();
    session.write_bytes(b"\r");
    let result = json_result(&session, start);
    assert_eq!(result[0]["key"], "theme.name");
    assert_eq!(result[0]["value"], "rose-pine-moon");

    session.write_bytes(b"exit\r\r");
    assert!(session.wait_for_exit(HANG_TIMEOUT), "REPL should exit");
}
