#[test]
#[cfg(unix)]
fn unknown_domain_command_shows_command_hint_contract() {
    let home = make_temp_dir("plugins-unknown");
    plugin_command(&home, &home, &["missing"]).assert().failure().stderr(predicate::str::contains("Unknown command 'missing'").and(predicate::str::contains("list commands")));
}

#[test]
#[cfg(unix)]
fn external_plugin_help_is_passed_through_contract() {
    let home = make_temp_dir("plugins-help");
    let dir = home.join("plugins");
    write_plugin(&dir, "hello", Some(json!({"output": "json", "about": "Greet people"})), "import argparse\np=argparse.ArgumentParser(prog='osp hello', description='Greet people')\np.add_argument('--loud', action='store_true')\np.parse_args()\n");
    plugin_command(&home, &dir, &["hello", "--help"]).assert().success().stdout(predicate::str::starts_with("usage: osp hello").and(predicate::str::contains("--loud"))).stderr("");
    plugin_command(&home, &dir, &["hello", "-h"]).assert().success().stdout(predicate::str::starts_with("usage: osp hello"));
    plugin_command(&home, &dir, &["--help"]).assert().success().stdout(predicate::str::contains("Greet people"));
}

#[test]
#[cfg(unix)]
fn external_plugin_help_keeps_raw_stderr_contract() {
    let home = make_temp_dir("plugins-help-stderr");
    let dir = home.join("plugins");
    write_plugin(&dir, "hello", None, "print('hello help'); print('help note', file=sys.stderr)\n");
    plugin_command(&home, &dir, &["hello", "--help"]).assert().success().stdout("hello help\n").stderr("help note\n");
}

#[test]
#[cfg(unix)]
fn first_plugin_wins_and_builtins_are_shadowed_contract() {
    let home = make_temp_dir("plugins-order");
    let first = home.join("first");
    let later = home.join("later");
    write_plugin(&first, "hello", None, "print('first')\n");
    hello(&later);
    write_plugin(&first, "config", None, "print('not builtin')\n");
    plugin_command(&home, &later, &["--plugin-dir", first.to_str().unwrap(), "hello"]).assert().success().stdout("first\n");
    let rows = json_stdout(&mut plugin_command(&home, &later, &["--plugin-dir", first.to_str().unwrap(), "plugins", "list", "--json"]));
    assert!(rows.as_array().unwrap().iter().any(|row| row["name"] == "config" && row["status"] == "shadowed by builtin config"));
    assert!(rows.as_array().unwrap().iter().any(|row| row["name"] == "hello" && row["status"].as_str().unwrap().starts_with("shadowed by ")));
    plugin_command(&home, &first, &["config", "--help"]).assert().success().stdout(predicate::str::contains("not builtin").not());
}

#[test]
#[cfg(unix)]
fn path_plugins_work_without_describe_and_describe_errors_stay_runnable_contract() {
    let home = make_temp_dir("plugins-path");
    let dir = home.join("plugins");
    let marker = home.join("ran");
    write_plugin(&dir, "plain", None, &format!("from pathlib import Path\nPath({:?}).touch()\nprint('plain')\n", marker.display().to_string()));
    let path = format!("{}:/usr/bin:/bin", dir.display());
    let rows = json_stdout(plugin_command(&home, &home, &["plugins", "list", "--json"]).env("PATH", &path));
    assert_eq!(rows[0]["source"], "path");
    assert!(!marker.exists(), "plain executables must not run during listing");
    plugin_command(&home, &home, &["plain"]).env("PATH", path).assert().success().stdout("plain\n");
    let broken = write_plugin(&dir, "broken", Some(json!({})), "print('still runs')\n");
    std::fs::write(broken, "#!/bin/sh\nif [ \"$1\" = --osp-describe ]; then echo invalid; else echo 'still runs'; fi\n").unwrap();
    let rows = json_stdout(&mut plugin_command(&home, &dir, &["doctor", "plugins", "--json"]));
    assert!(rows[0]["status"].as_str().unwrap().starts_with("describe error: invalid describe JSON metadata"));
    plugin_command(&home, &dir, &["broken"]).assert().success().stdout("still runs\n");
}

#[test]
#[cfg(unix)]
fn bundled_plugin_runs_without_manifest_contract() {
    let home = make_temp_dir("plugins-bundled");
    let bundled = home.join("bundled");
    hello(&bundled);
    let rows = json_stdout(plugin_command(&home, &home, &["plugins", "list", "--json"])
        .env("OSP_BUNDLED_PLUGIN_DIR", &bundled));
    assert_eq!(rows[0]["name"], "hello");
    assert_eq!(rows[0]["source"], "bundled");
    assert_eq!(rows[0]["status"], "ok");
    plugin_command(&home, &home, &["hello", "bob"])
        .env("OSP_BUNDLED_PLUGIN_DIR", bundled).assert().success()
        .stdout(predicate::str::contains("hello bob"));
}
