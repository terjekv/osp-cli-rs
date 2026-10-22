#![allow(missing_docs)]

#[cfg(unix)]
use crate::support::{HANG_TIMEOUT, ReplPtyColorMode, ReplPtyConfig, ReplPtySession};

#[cfg(unix)]
fn spawn_repl_with_color(simple_prompt: bool) -> ReplPtySession {
    ReplPtySession::spawn(
        ReplPtyConfig::default()
            .with_color_mode(ReplPtyColorMode::Always)
            .with_simple_prompt(simple_prompt),
    )
}

#[cfg(unix)]
#[test]
fn repl_prompt_prefix_uses_prompt_text_color() {
    if crate::isolation::isolated_test() {
        return;
    }
    let mut session = spawn_repl_with_color(false);

    let start = 0;
    let expected = "\x1b[38;2;224;222;244m╭─";
    assert!(
        session.wait_for_output_since(start, expected, HANG_TIMEOUT),
        "expected styled prompt prefix in output; output:\n{}",
        session.output_snapshot(2000),
    );

    session.write_bytes(b"\x03");
    session.write_bytes(b"exit\r\r");
    if !session.wait_for_exit(HANG_TIMEOUT) {
        session.kill();
    }
}
