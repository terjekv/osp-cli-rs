#[test]
#[cfg(unix)]
fn external_plugin_dispatch_contract() {
    let home = make_temp_dir("plugins-dispatch");
    let dir = home.join("plugins");
    hello(&dir);
    plugin_command(&home, &dir, &["hello", "bob", "alice"]).assert().success().stdout(predicate::str::contains("hello bob").and(predicate::str::contains("hello alice")));
    let rows = json_stdout(&mut plugin_command(&home, &dir, &["hello", "bob", "--json"]));
    assert_eq!(rows, json!([{"name": "bob", "greeting": "hello bob"}]));
    let rows = json_stdout(&mut plugin_command(&home, &dir, &["hello", "bob", "alice", "--json", "|", "F name=bob"]));
    assert_eq!(rows[0]["name"], "bob");
    assert_eq!(rows.as_array().unwrap().len(), 1);
}

#[test]
#[cfg(unix)]
fn plugin_dispatch_propagates_runtime_hints_contract() {
    let home = make_temp_dir("plugins-hints");
    let dir = home.join("plugins");
    write_plugin(&dir, "hints", Some(json!({"output": "json"})), "print(json.dumps({k: v for k, v in os.environ.items() if k.startswith('OSP_')}))\n");
    let rows = json_stdout(&mut plugin_command(&home, &dir, &["-u", "person", "hints", "--json", "--color", "never", "--unicode", "never", "-v", "-d"]));
    let row = &rows[0];
    assert_eq!(row["OSP_COMMAND"], "hints");
    assert_eq!(row["OSP_USER"], "person");
    assert_eq!(row["OSP_FORMAT"], "json");
    assert_eq!(row["OSP_COLOR"], "never");
    assert_eq!(row["OSP_UNICODE"], "never");
    assert_eq!(row["OSP_UI_VERBOSITY"], "info");
    assert_eq!(row["OSP_DEBUG_LEVEL"], "1");
    assert_eq!(row["OSP_TERMINAL_KIND"], "cli");
    assert_eq!(row["OSP_PROFILE"], "default");
}

#[test]
#[cfg(unix)]
fn plugin_dispatch_propagates_config_env_contract() {
    let home = make_temp_dir("plugins-env");
    let dir = home.join("plugins");
    write_plugin(&dir, "cfg", Some(json!({"output": "json"})), "print(json.dumps({k: v for k, v in os.environ.items() if k.startswith('OSP_PLUGIN_CFG_')}))\n");
    write_config(&home, "[default]\nprofile.default = 'default'\nextensions.plugins.env.endpoint = 'shared'\nextensions.plugins.cfg.env.endpoint = 'command'\nextensions.plugins.cfg.env.api.token = 'secret'\nextensions.plugins.env.numbers = [1, 2]\n");
    let rows = json_stdout(&mut plugin_command(&home, &dir, &["cfg", "--json"]));
    assert_eq!(rows[0]["OSP_PLUGIN_CFG_ENDPOINT"], "command");
    assert_eq!(rows[0]["OSP_PLUGIN_CFG_API_TOKEN"], "secret");
    assert_eq!(rows[0]["OSP_PLUGIN_CFG_NUMBERS"], "[1,2]");
}

#[test]
#[cfg(unix)]
fn plugin_dispatch_inherits_environment_and_stdin_contract() {
    let home = make_temp_dir("plugins-inherit");
    let dir = home.join("plugins");
    write_plugin(&dir, "ask", None, "print(os.environ['USER'], os.environ['KRB5CCNAME'], sys.stdin.readline().strip(), *sys.argv[1:])\n");
    plugin_command(&home, &dir, &["ask", "argument"]).env("USER", "tester").env("KRB5CCNAME", "FILE:/tmp/example").write_stdin("yes\n").assert().success().stdout("tester FILE:/tmp/example yes argument\n");
}

#[test]
#[cfg(unix)]
fn plugin_non_zero_exit_surfaces_stderr_contract() {
    let home = make_temp_dir("plugins-exit");
    let dir = home.join("plugins");
    write_plugin(&dir, "plain", None, "print('plain text'); print('backend note', file=sys.stderr); sys.exit(3)\n");
    plugin_command(&home, &dir, &["plain"]).assert().code(3).stdout("plain text\n").stderr("backend note\n");
    plugin_command(&home, &dir, &["plain", "--json"]).assert().code(3).stdout(predicate::str::contains("plain text")).stderr(predicate::str::contains("plain exited with status 3").and(predicate::str::contains("backend note")));
}

#[test]
#[cfg(unix)]
fn plugin_text_output_and_pipeline_contract() {
    let home = make_temp_dir("plugins-text");
    let dir = home.join("plugins");
    write_plugin(&dir, "text", None, "print('plain text')\n");
    plugin_command(&home, &dir, &["text", "--json"]).assert().success().stdout(predicate::str::contains("plain text"));
    plugin_command(&home, &dir, &["text", "|", "F name=bob"]).assert().failure().stderr(predicate::str::contains("text printed text, not JSON; pipes need JSON output"));
}

#[test]
#[cfg(unix)]
fn plugin_messages_stay_on_stderr_when_data_is_json_contract() {
    let home = make_temp_dir("plugins-response");
    let dir = home.join("plugins");
    write_plugin(&dir, "messageful", Some(json!({"output": "json"})), "print(json.dumps({'protocol_version': 1, 'ok': True, 'data': {'message': 'json-from-plugin'}, 'error': None, 'messages': [{'level': 'warning', 'text': 'plugin-warning-line'}]}))\n");
    let output = plugin_command(&home, &dir, &["messageful", "--json"]).assert().success().stderr(predicate::str::contains("plugin-warning-line")).get_output().clone();
    let rows: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(rows[0]["message"], "json-from-plugin");
}

#[test]
#[cfg(unix)]
fn describe_cache_is_reused_and_invalidated_contract() {
    use std::io::Write;
    let home = make_temp_dir("plugins-cache");
    let dir = home.join("plugins");
    let counter = home.join("counter");
    let body = format!("#!/usr/bin/env python3\nimport json, sys\nfrom pathlib import Path\np=Path({:?})\nif sys.argv[1:] == ['--osp-describe']:\n    p.write_text(p.read_text() + 'x' if p.exists() else 'x')\n    print('{{\"about\":\"cached\"}}')\nelse:\n    print('run')\n", counter.display().to_string());
    let path = write_plugin(&dir, "count", None, "");
    std::fs::write(&path, body).unwrap();
    for _ in 0..2 { plugin_command(&home, &dir, &["plugins", "list"]).assert().success(); }
    assert_eq!(std::fs::read_to_string(&counter).unwrap(), "x");
    plugin_command(&home, &dir, &["plugins", "refresh"]).assert().success();
    plugin_command(&home, &dir, &["plugins", "list"]).assert().success();
    assert_eq!(std::fs::read_to_string(&counter).unwrap(), "xx");
    std::fs::OpenOptions::new().append(true).open(path).unwrap().write_all(b"\n# changed\n").unwrap();
    plugin_command(&home, &dir, &["plugins", "list"]).assert().success();
    assert_eq!(std::fs::read_to_string(&counter).unwrap(), "xxx");
}
