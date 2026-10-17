#[test]
#[cfg(unix)]
fn plugins_list_and_doctor_contract() {
    let home = make_temp_dir("plugins-list");
    let dir = home.join("plugins");
    hello(&dir);
    let rows = json_stdout(&mut plugin_command(&home, &dir, &["plugins", "list", "--json"]));
    assert_eq!(rows[0]["name"], "hello");
    assert_eq!(rows[0]["about"], "Greet people");
    assert_eq!(rows[0]["status"], "ok");
    assert_eq!(rows[0]["source"], "env");
    assert!(rows[0]["path"].as_str().unwrap().ends_with("osp-hello"));
    assert_eq!(json_stdout(&mut plugin_command(&home, &dir, &["doctor", "plugins", "--json"])), json!([]));
}

#[test]
#[cfg(unix)]
fn plugins_enable_and_disable_contract() {
    let home = make_temp_dir("plugins-toggle");
    let dir = home.join("plugins");
    hello(&dir);
    write_plugin(&dir, "other", None, "print('other runs')\n");
    plugin_command(&home, &dir, &["plugins", "disable", "hello"]).assert().success().stderr(predicate::str::contains("disabled plugin: hello"));
    let rows = json_stdout(&mut plugin_command(&home, &dir, &["plugins", "list", "--json"]));
    assert_eq!(rows[0]["status"], "disabled");
    plugin_command(&home, &dir, &["hello"]).assert().failure().stderr(predicate::str::contains("Unknown command"));
    plugin_command(&home, &dir, &["other"]).assert().success().stdout("other runs\n");
    let config = std::fs::read_to_string(home.join(".config/osp/config.toml")).unwrap();
    assert!(config.contains("disabled"));
    assert!(config.contains("hello"));
    plugin_command(&home, &dir, &["plugins", "enable", "hello"]).assert().success().stderr(predicate::str::contains("enabled plugin: hello"));
    plugin_command(&home, &dir, &["hello"]).assert().success().stdout(predicate::str::contains("hello world"));
}

#[test]
#[cfg(unix)]
fn quiet_hides_success_messages_contract() {
    let home = make_temp_dir("plugins-quiet");
    let dir = home.join("plugins");
    hello(&dir);
    plugin_command(&home, &dir, &["plugins", "disable", "hello", "-q"]).assert().success().stderr("");
}

#[test]
#[cfg(unix)]
fn plugins_refresh_reports_success_contract() {
    let home = make_temp_dir("plugins-refresh");
    let dir = home.join("plugins");
    hello(&dir);
    plugin_command(&home, &dir, &["plugins", "refresh"]).assert().success().stderr(predicate::str::contains("refreshed plugin discovery cache"));
}
