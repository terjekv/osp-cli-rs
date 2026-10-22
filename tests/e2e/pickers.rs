use crate::support::{HANG_TIMEOUT, ReplPtyConfig, ReplPtySession};

fn wait(session: &ReplPtySession, start: usize, text: &str) {
    assert!(
        session.wait_for_picker_since(start, text),
        "waiting for {text:?}:\n{}",
        session.output_snapshot(12000)
    );
}

fn submit(session: &mut ReplPtySession, line: &str) -> usize {
    let start = session.output_len();
    session.write_bytes(format!("\x1b[200~{line}\x1b[201~").as_bytes());
    assert!(session.wait_for_editor_since(start, HANG_TIMEOUT));
    session.write_bytes(b"\r");
    start
}

fn open_picker(session: &mut ReplPtySession, line: &str) {
    let start = submit(session, line);
    wait(session, start, "Tab next/value");
}

fn json_command(session: &mut ReplPtySession, line: &str) -> serde_json::Value {
    let start = submit(session, line);
    assert!(
        session.wait_for_plain_output_since(start, "} ]", HANG_TIMEOUT),
        "{}",
        session.output_snapshot(8000)
    );
    let output = crate::support::strip_ansi_preserve_newlines(&session.output_since(start));
    let start = output.find("[\n").expect("JSON array output");
    serde_json::Deserializer::from_str(&output[start..])
        .into_iter()
        .next()
        .unwrap()
        .unwrap()
}

#[test]
fn config_picker_stages_validated_edits_until_save_and_restores_the_editor() {
    let config = "[default]\nconfig.default-target = 'global'\nui.margin = 0\n";
    let mut session = ReplPtySession::spawn(ReplPtyConfig::default().with_config(config));
    wait(&session, 0, "default>");
    open_picker(&mut session, "config edit ui.margin");
    let start = session.output_len();
    session.write_bytes(b"\r");
    wait(&session, start, "Ctrl-U clear");
    let start = session.output_len();
    session.write_bytes(b"\x159\r");
    wait(&session, start, "Value: 9");
    let start = session.output_len();
    session.write_bytes(b"q");
    wait(&session, start, "default>");
    assert_eq!(
        json_command(&mut session, "config get ui.margin --json")[0]["value"],
        0
    );

    open_picker(&mut session, "config edit ui.margin");
    let start = session.output_len();
    session.write_bytes(b"\r");
    wait(&session, start, "Ctrl-U clear");
    let start = session.output_len();
    session.write_bytes(b"\x15invalid\r");
    wait(&session, start, "integer");
    let start = session.output_len();
    session.write_bytes(b"\x152\r");
    wait(&session, start, "Value: 2");
    let start = session.output_len();
    session.write_bytes(b"\x1b[F\r");
    wait(&session, start, "default>");
    assert_eq!(
        json_command(&mut session, "config get ui.margin --json")[0]["value"],
        2
    );
    submit(&mut session, "exit");
    assert!(session.wait_for_exit(HANG_TIMEOUT));
}

#[test]
fn theme_picker_saves_a_named_preview_and_applies_it_to_the_session() {
    let config = "[default]\nconfig.default-target = 'global'\ntheme.name = 'nord'\n";
    let mut session = ReplPtySession::spawn(ReplPtyConfig::default().with_config(config));
    wait(&session, 0, "default>");
    open_picker(&mut session, "theme preview");
    let start = session.output_len();
    // End selects Save; Right selects Save as theme.
    session.write_bytes(b"\x1b[F\x1b[C\r");
    wait(&session, start, "Theme name:");
    let start = session.output_len();
    session.write_bytes(b"Boundary Theme\r");
    wait(&session, start, "default>");
    assert_eq!(
        json_command(&mut session, "config get theme.name --json")[0]["value"],
        "boundary-theme"
    );
    let theme = json_command(
        &mut session,
        "theme list --json | boundary-theme | P id,name,accent",
    );
    assert_eq!(
        theme,
        serde_json::json!([{"id":"boundary-theme", "name":"Boundary Theme", "accent":"#88c0d0"}])
    );
    submit(&mut session, "exit");
    assert!(session.wait_for_exit(HANG_TIMEOUT));
}
