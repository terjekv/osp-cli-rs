use crate::process_support::{osp_command, write_config, write_executable_script};
use crate::temp_support::make_temp_dir;
use serde_json::{Value, json};
use std::os::unix::fs::PermissionsExt;

#[test]
fn recording_persists_private_redacted_command_groups_across_source_execution() {
    let home = make_temp_dir("osp-recording");
    let plugins = home.path().join("plugins");
    std::fs::create_dir(&plugins).unwrap();
    write_executable_script(
        &plugins.join("osp-record-probe"),
        r#"#!/bin/sh
case "$*" in
  --osp-describe) echo '{"name":"record-probe","about":"Recording fixture","args":[],"flags":{},"subcommands":[],"output":"json"}' ;;
  '') echo '{"protocol_version":1,"ok":true,"data":{"name":"fixture","token":"response-secret","mirror":"response-secret"},"error":null}' ;;
  *) exit 91 ;;
esac
"#,
    );
    write_config(
        home.path(),
        "[default]\nextensions.record.password = 'config-secret'\n",
    );
    let source = home.path().join("batch.osp");
    std::fs::write(
        &source,
        "theme list --json | nord | P id,name\nrecord-probe --json\n",
    )
    .unwrap();
    let recording = home.path().join("session.jsonl");
    let input = format!(
        "recording start '{}'\nrecording status\nsource '{}'\nconfig get extensions.record.password --show-secrets\n theme list\nrecording stop\nrecording status\ntheme list\nexit\n",
        recording.display(),
        source.display()
    );
    let output = osp_command(home.path())
        .env("OSP_PLUGIN_PATH", &plugins)
        .write_stdin(input)
        .assert()
        .success()
        .get_output()
        .clone();
    let stdout = String::from_utf8(output.stdout).unwrap();
    assert!(stdout.contains("Recording stopped."), "{stdout}");
    assert!(stdout.contains("Recording is not active."), "{stdout}");
    assert_eq!(
        std::fs::metadata(&recording).unwrap().permissions().mode() & 0o777,
        0o600
    );
    let transcript = std::fs::read_to_string(&recording).unwrap();
    let events: Vec<Value> = transcript
        .lines()
        .map(|line| serde_json::from_str(line).unwrap())
        .collect();
    let commands: Vec<_> = events
        .iter()
        .filter(|event| event["event"] == "command")
        .map(|event| event["line"].clone())
        .collect();
    assert_eq!(
        commands,
        vec![json!(format!("source '{}'", source.display()))]
    );
    let rendered: Vec<Value> = events
        .iter()
        .filter(|event| event["event"] == "output" && event["stream"] == "stdout")
        .filter_map(|event| serde_json::from_str(event["text"].as_str().unwrap()).ok())
        .collect();
    assert!(
        rendered.contains(&json!([{"id":"nord","name":"Nord"}])),
        "{transcript}"
    );
    assert!(
        rendered.contains(&json!([{"name":"fixture","token":"[redacted]","mirror":"[redacted]"}])),
        "{transcript}"
    );
    assert!(!transcript.contains("response-secret"), "{transcript}");
    assert!(!transcript.contains("config-secret"), "{transcript}");
}
