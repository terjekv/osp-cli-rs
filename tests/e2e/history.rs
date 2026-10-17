#![allow(missing_docs)]

#[cfg(unix)]
use crate::support::{
    ReplPtyConfig, ReplPtySession, osp_command, parse_json_stdout, strip_ansi_preserve_newlines,
    write_config,
};
#[cfg(unix)]
use crate::temp_support::make_temp_dir;

#[cfg(unix)]
#[test]
fn repl_history_capture_and_maintenance_survive_processes_with_profile_scope() {
    if crate::isolation::isolated_test() {
        return;
    }
    let home = make_temp_dir("osp-e2e-history-lifecycle-home");
    let history = home.path().join("operator-${profile.active}-history.jsonl");
    write_config(
        home.path(),
        r#"
[default]
profile.default = "uio"
repl.intro = "none"
repl.simple_prompt = true

[profile.uio]
theme.name = "nord"

[profile.tsd]
theme.name = "dracula"
"#,
    );
    let capture = |profile: &str, input: &str| {
        let config = format!(
            "[default]\nprofile.default = {profile:?}\nrepl.history.path = {:?}\n",
            history.to_str().unwrap()
        );
        let mut session = ReplPtySession::spawn(ReplPtyConfig::default().with_config(&config));
        let timeout = std::time::Duration::from_secs(3);
        let prompt = format!("{profile}>");
        assert!(session.wait_for_plain_output(&prompt, timeout));
        session.type_text("config set --session repl.history.enabled true");
        let start = session.output_len();
        session.write_bytes(b"\r");
        assert!(session.wait_for_plain_output_since(start, "for this session only", timeout));
        assert!(session.wait_for_plain_output_since(start, &prompt, timeout));
        let mut results = Vec::new();
        for command in input.lines().filter(|command| *command != "exit") {
            session.type_text(command);
            let start = session.output_len();
            session.write_bytes(b"\r");
            assert!(
                session.wait_for_plain_output_since(start, "} ]", timeout),
                "{}",
                session.output_snapshot(8000)
            );
            let output = strip_ansi_preserve_newlines(&session.output_since(start));
            let json_start = output
                .find("[\n")
                .expect("executed REPL command should return JSON");
            results.push(
                serde_json::Deserializer::from_str(&output[json_start..])
                    .into_iter::<serde_json::Value>()
                    .next()
                    .unwrap()
                    .unwrap(),
            );
            assert!(session.wait_for_plain_output_since(start, &prompt, timeout));
        }
        session.write_bytes(b"exit\r");
        assert!(
            session.wait_for_exit(timeout),
            "{}",
            session.output_snapshot(8000)
        );
        results
    };
    let uio = capture(
        "uio",
        "theme list --json | nord\ntheme list --json | dracula\nexit\n",
    );
    assert_eq!(uio[0][0]["id"], "nord");
    assert_eq!(uio[1][0]["id"], "dracula");
    let tsd = capture(
        "tsd",
        "theme list --json | rose-pine-moon\ntheme list --json | nord\nexit\n",
    );
    assert_eq!(tsd[0][0]["id"], "rose-pine-moon");
    assert_eq!(tsd[1][0]["id"], "nord");

    let list = |profile: &str| {
        let output = osp_command(home.path())
            .timeout(std::time::Duration::from_secs(5))
            .env("OSP__REPL__HISTORY__PATH", &history)
            .args(["--profile", profile, "--json", "history", "list"])
            .assert()
            .success()
            .get_output()
            .clone();
        let entries = parse_json_stdout(&output.stdout);
        for entry in entries.as_array().unwrap() {
            assert!(entry["timestamp_ms"].as_i64().unwrap() > 0);
        }
        entries
            .as_array()
            .unwrap()
            .iter()
            .map(|entry| entry["command"].as_str().unwrap().to_string())
            .collect::<Vec<_>>()
    };
    assert_eq!(
        list("uio"),
        ["theme list --json | nord", "theme list --json | dracula"]
    );
    assert_eq!(
        list("tsd"),
        [
            "theme list --json | rose-pine-moon",
            "theme list --json | nord"
        ]
    );
    let pruned = osp_command(home.path())
        .timeout(std::time::Duration::from_secs(5))
        .env("OSP__REPL__HISTORY__PATH", &history)
        .args(["--profile", "uio", "history", "prune", "1"])
        .assert()
        .success()
        .get_output()
        .clone();
    assert_eq!(
        String::from_utf8(pruned.stdout).unwrap(),
        "Removed 1 entry from root history.\n"
    );
    assert_eq!(list("uio"), ["theme list --json | dracula"]);
    assert_eq!(
        list("tsd"),
        [
            "theme list --json | rose-pine-moon",
            "theme list --json | nord"
        ]
    );
    let cleared = osp_command(home.path())
        .timeout(std::time::Duration::from_secs(5))
        .env("OSP__REPL__HISTORY__PATH", &history)
        .args(["--profile", "tsd", "history", "clear"])
        .assert()
        .success()
        .get_output()
        .clone();
    assert_eq!(
        String::from_utf8(cleared.stdout).unwrap(),
        "Cleared root history.\n"
    );
    let resumed = capture("tsd", "theme list --json | dracula\nexit\n");
    assert_eq!(resumed[0][0]["id"], "dracula");
    assert_eq!(list("tsd"), ["theme list --json | dracula"]);
    assert_eq!(list("uio"), ["theme list --json | dracula"]);
}
