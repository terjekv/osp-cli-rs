use crate::temp_support::make_temp_dir;
use crate::test_env::isolated_env;

#[test]
fn one_shot_json_trace_reports_config_and_execution_contract() {
    let home = make_temp_dir("osp-cli-startup-trace");
    let mut command = crate::test_env::command(assert_cmd::cargo::cargo_bin!("osp"));
    command
        .env_clear()
        .env("PATH", "/usr/bin:/bin")
        .env("TERM", "xterm-256color")
        .env("LANG", "C.UTF-8")
        .env("OSP_TRACE_STARTUP", "json");
    for (key, value) in isolated_env(home.path()) {
        command.env(key, value);
    }

    let output = command
        .args(["--defaults-only", "config", "show"])
        .output()
        .expect("osp should run");
    assert!(
        output.status.success(),
        "osp failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let trace: serde_json::Value =
        serde_json::from_slice(&output.stderr).expect("stderr should contain one JSON trace");
    let phases = trace["phases"]
        .as_array()
        .expect("trace should contain a phases array");
    assert!(
        phases
            .iter()
            .any(|phase| phase["name"] == "config resolution")
    );
    assert!(
        phases
            .iter()
            .any(|phase| phase["name"] == "command execution")
    );
}
