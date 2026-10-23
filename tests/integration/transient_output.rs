//! Process stdio owns transient lines even when a notice bypasses a command sink.
#[cfg(unix)]
#[test]
fn transient_output_clears_before_documents() {
    use osp_cli::app::{StdIoUiSink, UiSink};
    use std::time::Duration;
    if std::env::var_os("OSP_RULE_SPINNER_CHILD").is_some() {
        let mut sink = StdIoUiSink;
        // A parent acknowledgement follows an observed frame, so slow builds
        // and scheduling do not turn a fixed sleep into a missing-frame race.
        let mut attrs = std::mem::MaybeUninit::<libc::termios>::uninit();
        // SAFETY: stdin is the owned PTY; attrs is read only after tcgetattr succeeds.
        unsafe {
            assert_eq!(libc::tcgetattr(libc::STDIN_FILENO, attrs.as_mut_ptr()), 0);
            let mut attrs = attrs.assume_init();
            attrs.c_lflag &= !libc::ECHO;
            assert_eq!(
                libc::tcsetattr(libc::STDIN_FILENO, libc::TCSANOW, &attrs),
                0
            );
        }
        for (text, stderr) in [("ERROR-MARKER", true), ("RESULT-MARKER", false)] {
            let spinner = sink
                .start_progress_spinner(&format!("WORK-{text}"), false)
                .unwrap();
            let mut acknowledgement = String::new();
            std::io::stdin().read_line(&mut acknowledgement).unwrap();
            if stderr {
                sink.write_stderr(&format!("{text}\n"));
            } else {
                sink.write_stdout(&format!("{text}\n"));
            }
            acknowledgement.clear();
            std::io::stdin().read_line(&mut acknowledgement).unwrap();
            drop(spinner);
        }
        return;
    }
    use portable_pty::PtySystem;
    let pair = portable_pty::NativePtySystem::default()
        .openpty(portable_pty::PtySize {
            rows: 24,
            cols: 95,
            pixel_width: 0,
            pixel_height: 0,
        })
        .unwrap();
    let mut command = portable_pty::CommandBuilder::new(std::env::current_exe().unwrap());
    command.args([
        "--exact",
        "transient_output::transient_output_clears_before_documents",
        "--nocapture",
    ]);
    command.env("OSP_RULE_SPINNER_CHILD", "1");
    command.env("TERM", "xterm-256color");
    let (bytes, status) = crate::pty_support::capture_dialogue(
        pair,
        command,
        Duration::from_secs(20),
        &[
            ("WORK-ERROR-MARKER", "\n", false),
            ("WORK-ERROR-MARKER", "\n", false),
            ("WORK-RESULT-MARKER", "\n", false),
            ("WORK-RESULT-MARKER", "\n", false),
        ],
    );
    assert!(status.success(), "{bytes:?}");
    for marker in ["ERROR-MARKER", "RESULT-MARKER"] {
        assert!(
            bytes.contains(&format!("{marker}\r\n\r\x1b[2K")),
            "spinner must redraw after {marker}: {bytes:?}"
        );
    }
    for marker in ["ERROR-MARKER", "RESULT-MARKER"] {
        assert!(
            bytes.contains(&format!("\r\x1b[2K{marker}")),
            "transient line must be cleared before {marker}: {bytes:?}"
        );
    }
    let mut terminal = vt100::Parser::new(24, 95, 1000);
    terminal.process(bytes.as_bytes());
    let screen = terminal.screen().contents();
    assert!(
        screen.contains("ERROR-MARKER") && screen.contains("RESULT-MARKER"),
        "{screen}"
    );
    assert!(!screen.contains("WORK-"), "{screen}");
}

#[cfg(unix)]
#[test]
fn spinner_waits_one_second() {
    use osp_cli::app::{StdIoUiSink, UiSink};
    use std::time::Duration;
    if std::env::var_os("OSP_RULE_DELAY_CHILD").is_some() {
        let mut sink = StdIoUiSink;
        let spinner = sink
            .start_progress_spinner("FAST-WORK-MARKER", false)
            .unwrap();
        std::thread::sleep(Duration::from_millis(500));
        drop(spinner);
        sink.write_stdout("FAST-RESULT-MARKER\n");
        return;
    }
    use portable_pty::PtySystem;
    let pair = portable_pty::NativePtySystem::default()
        .openpty(portable_pty::PtySize::default())
        .unwrap();
    let mut command = portable_pty::CommandBuilder::new(std::env::current_exe().unwrap());
    command.args([
        "--exact",
        "transient_output::spinner_waits_one_second",
        "--nocapture",
    ]);
    command.env("OSP_RULE_DELAY_CHILD", "1");
    command.env("TERM", "xterm-256color");
    let (bytes, status) = crate::pty_support::capture(pair, command, Duration::from_secs(10));
    assert!(status.success(), "{bytes:?}");
    assert!(bytes.contains("FAST-RESULT-MARKER"), "{bytes:?}");
    assert!(
        !bytes.contains("FAST-WORK-MARKER"),
        "spinner appeared before one second: {bytes:?}"
    );
}

#[cfg(unix)]
#[test]
fn password_dialogues_clear_transient_output() {
    use osp_cli::app::{StdIoUiSink, UiSink};
    use std::time::Duration;
    if std::env::var_os("OSP_RULE_PROMPT_CHILD").is_some() {
        let mut sink = StdIoUiSink;
        for hidden in [true, false] {
            let spinner = sink
                .start_progress_spinner("PROMPT-WORK-MARKER", false)
                .unwrap();
            std::thread::sleep(Duration::from_millis(1300));
            let answer = if hidden {
                osp_cli::ui::prompt::read_hidden("Hidden-PASSWORD: ")
            } else {
                osp_cli::ui::prompt::read_text("Visible-PASSWORD: ")
            }
            .unwrap()
            .unwrap();
            assert_eq!(answer.trim(), "synthetic-answer");
            drop(spinner);
        }
        return;
    }
    use portable_pty::PtySystem;
    let pair = portable_pty::NativePtySystem::default()
        .openpty(portable_pty::PtySize::default())
        .unwrap();
    let mut command = portable_pty::CommandBuilder::new(std::env::current_exe().unwrap());
    command.args([
        "--exact",
        "transient_output::password_dialogues_clear_transient_output",
        "--nocapture",
    ]);
    command.env("OSP_RULE_PROMPT_CHILD", "1");
    command.env("TERM", "xterm-256color");
    let (bytes, status) = crate::pty_support::capture_dialogue(
        pair,
        command,
        Duration::from_secs(15),
        &[
            ("Hidden-PASSWORD: ", "synthetic-answer\r", true),
            ("Visible-PASSWORD: ", "synthetic-answer\r", false),
        ],
    );
    assert!(status.success(), "{bytes:?}");
    for prompt in ["Hidden-PASSWORD: ", "Visible-PASSWORD: "] {
        assert!(bytes.contains(&format!("\r\x1b[2K{prompt}")), "{bytes:?}");
    }
}
