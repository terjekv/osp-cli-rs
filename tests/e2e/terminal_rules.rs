//! Terminal lifetime contracts, using real PTY bytes and a scrollback-aware VT.
#[cfg(unix)]
#[test]
fn intro_and_clear_preserve_terminal_scrollback() {
    use crate::support::{HANG_TIMEOUT, ReplPtyColorMode, ReplPtyConfig, ReplPtySession};
    if crate::isolation::isolated_test() {
        return;
    }
    let mut session = ReplPtySession::spawn(
        ReplPtyConfig::default()
            .with_color_mode(ReplPtyColorMode::Always)
            .with_intro_override(None)
            .with_config(
                r#"[default]
profile.default = "default"
repl.intro = "full"
repl.intro_template.full = "INTRO-MARKER"
"#,
            ),
    );
    assert!(session.wait_for_plain_output("default>", HANG_TIMEOUT));
    let mut terminal = vt100::Parser::new(24, 80, 1000);
    terminal.process(b"PRE-OSP-SHELL-OUTPUT\r\nlast shell line\r\n");
    let startup = session.output_since(0);
    terminal.process(startup.as_bytes());
    assert!(
        terminal
            .screen()
            .rows(0, 80)
            .next()
            .unwrap()
            .contains("INTRO-MARKER"),
        "intro must start on row one: {}",
        terminal.screen().contents()
    );
    terminal.screen_mut().set_scrollback(usize::MAX);
    assert!(
        terminal
            .screen()
            .contents()
            .contains("PRE-OSP-SHELL-OUTPUT")
    );
    terminal.screen_mut().set_scrollback(0);
    let mark = session.output_len();
    session.write_bytes(b"\x0c");
    assert!(session.wait_for_editor_since(mark, HANG_TIMEOUT));
    terminal.process(session.output_since(mark).as_bytes());
    assert!(
        terminal
            .screen()
            .rows(0, 80)
            .next()
            .unwrap()
            .contains("default>")
    );
    assert!(
        (1..=1000).any(|offset| {
            terminal.screen_mut().set_scrollback(offset);
            terminal.screen().contents().contains("INTRO-MARKER")
        }),
        "Ctrl-L must preserve the intro in scrollback"
    );
    terminal.screen_mut().set_scrollback(0);
    let mark = session.output_len();
    session.write_bytes(b"exit\r");
    assert!(session.wait_for_exit(HANG_TIMEOUT));
    terminal.process(session.output_since(mark).as_bytes());
    terminal.screen_mut().set_scrollback(usize::MAX);
    assert!(
        terminal
            .screen()
            .contents()
            .contains("PRE-OSP-SHELL-OUTPUT")
    );
    let bytes = session.output_since(0);
    assert!(!bytes.contains("\x1b[3J"), "scrollback erase in {bytes:?}");
    assert!(
        !bytes.contains("\x1b[?1049h"),
        "alternate screen in {bytes:?}"
    );
}

#[cfg(unix)]
fn service_plugin() -> crate::temp_support::TestTempDir {
    let directory = crate::temp_support::make_temp_dir("osp-terminal-rule-service");
    crate::support::write_executable_script(
        &directory.join("osp-orch"),
        r#"#!/bin/sh
if [ "$1" = "--osp-describe" ]; then
  echo '{"about":"Service rule fixture","subcommands":[{"name":"recover","about":"Recover fixture","args":[],"flags":{},"subcommands":[]},{"name":"show","about":"Show fixture output","args":[],"flags":{},"subcommands":[]}],"output":"json"}'
elif [ "$1" = "recover" ]; then
  exit 1
else
  echo '{"protocol_version":1,"ok":true,"data":[{"result":"EXECUTED-MARKER"}],"error":null,"meta":{}}'
fi
"#,
    );
    directory
}

#[cfg(unix)]
#[test]
fn recovery_key_inserts_editable_scoped_command() {
    use crate::support::{HANG_TIMEOUT, ReplPtyConfig, ReplPtySession};
    if crate::isolation::isolated_test() {
        return;
    }
    let plugins = service_plugin();
    let history = plugins.join("history.jsonl");
    let mut session = ReplPtySession::spawn(
        ReplPtyConfig::default()
            .with_plugins_dir(&plugins)
            .with_config(&format!(
                "[default]\nrepl.shellable_commands = [\"orch\"]\nrepl.history.path = {history:?}\n"
            )),
    );
    assert!(session.wait_for_plain_output("default>", HANG_TIMEOUT));
    session.type_text("config set --session repl.history.enabled true");
    let mark = session.output_len();
    session.write_bytes(b"\r");
    assert!(session.wait_for_plain_output_since(mark, "for this session only", HANG_TIMEOUT));
    assert!(session.wait_for_editor_since(mark, HANG_TIMEOUT));
    session.type_text("orch");
    let mark = session.output_len();
    session.write_bytes(b"\r");
    assert!(session.wait_for_plain_output_since(mark, "default [orch]>", HANG_TIMEOUT));
    assert!(session.wait_for_editor_since(mark, HANG_TIMEOUT));
    session.type_text("recover");
    let mark = session.output_len();
    session.write_bytes(b"\r");
    assert!(
        session.wait_for_plain_output_since(mark, "Plugin orch crashed", HANG_TIMEOUT),
        "{}",
        session.output_since(mark)
    );
    assert!(session.wait_for_editor_since(mark, HANG_TIMEOUT));
    assert!(
        session.output_since(mark).contains("Alt+Enter"),
        "{}",
        session.output_since(mark)
    );
    let failure = session.output_since(mark);
    assert!(
        failure.find("Plugin orch crashed").unwrap() < failure.find("Alt+Enter").unwrap(),
        "{failure}"
    );
    let mark = session.output_len();
    session.write_bytes(b"\x1b\r");
    assert!(session.wait_for_editor_since(mark, HANG_TIMEOUT));
    let inserted = session.output_since(mark);
    assert!(inserted.contains("doctor plugins"), "{inserted}");
    let mark = session.output_len();
    session.write_bytes(b"\x1b[200~ | C\x1b[201~");
    assert!(session.wait_for_editor_since(mark, HANG_TIMEOUT));
    let mark = session.output_len();
    session.write_bytes(b"\r");
    assert!(session.wait_for_plain_output_since(mark, "count", HANG_TIMEOUT));
    session.write_bytes(b"\x04");
    assert!(session.wait_for_exit(HANG_TIMEOUT));
    // Finalized history is an execution barrier: only the edited recovery
    // command may have run, regardless of how the editor batches its paints.
    let records = std::fs::read_to_string(history).unwrap();
    let commands: Vec<_> = records
        .lines()
        .map(|line| {
            serde_json::from_str::<serde_json::Value>(line).unwrap()["command_line"]
                .as_str()
                .unwrap()
                .to_owned()
        })
        .collect();
    assert_eq!(
        commands,
        ["orch", "orch recover", "orch doctor plugins | C"],
        "{records}"
    );
}

#[cfg(unix)]
#[test]
fn eof_exits_from_root_and_service_shell() {
    use crate::support::{HANG_TIMEOUT, ReplPtyConfig, ReplPtySession};
    if crate::isolation::isolated_test() {
        return;
    }
    for shell in [false, true] {
        let plugins = service_plugin();
        let mut session = ReplPtySession::spawn(
            ReplPtyConfig::default()
                .with_plugins_dir(&plugins)
                .with_config("[default]\nrepl.shellable_commands = [\"orch\"]\n"),
        );
        assert!(session.wait_for_plain_output("default>", HANG_TIMEOUT));
        if shell {
            session.type_text("orch");
            let mark = session.output_len();
            session.write_bytes(b"\r");
            assert!(session.wait_for_plain_output_since(mark, "default [orch]>", HANG_TIMEOUT));
            assert!(session.wait_for_editor_since(mark, HANG_TIMEOUT));
        }
        session.write_bytes(b"\x04");
        assert!(
            session.wait_for_exit(HANG_TIMEOUT),
            "{}",
            session.output_since(0)
        );
    }
}
