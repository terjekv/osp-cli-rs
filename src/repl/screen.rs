//! Scroll visible output into history before clearing the REPL display.

pub(super) fn clear_sequence() -> String {
    // There is no cached CPR row here; crossterm's query can block for two seconds.
    let rows = crossterm::terminal::size()
        .map(|(_, rows)| rows)
        .unwrap_or(24)
        .max(1);
    format!(
        "\x1b[{rows};1H{}\x1b[H\x1b[2J",
        "\n".repeat(usize::from(rows))
    )
}
