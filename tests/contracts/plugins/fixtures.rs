#[cfg(unix)]
fn write_plugin(dir: &std::path::Path, name: &str, describe: Option<Value>, body: &str) -> std::path::PathBuf {
    use std::os::unix::fs::PermissionsExt;
    std::fs::create_dir_all(dir).unwrap();
    let path = dir.join(format!("osp-{name}"));
    let mut script = "#!/usr/bin/env python3\nimport json, os, sys\n".to_string();
    if let Some(describe) = describe {
        script.push_str(&format!("if sys.argv[1:] == ['--osp-describe']:\n    print({:?})\n    sys.exit(0)\n", describe.to_string()));
    }
    script.push_str(body);
    std::fs::write(&path, script).unwrap();
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).unwrap();
    path
}

#[cfg(unix)]
fn plugin_command(home: &std::path::Path, dir: &std::path::Path, args: &[&str]) -> Command {
    let mut command = Command::new(assert_cmd::cargo::cargo_bin!("osp"));
    command.env_clear().env("PATH", "/usr/bin:/bin").envs(crate::test_env::isolated_env(home))
        .env("OSP_PLUGIN_PATH", dir).args(args);
    command
}

#[cfg(unix)]
fn write_config(home: &std::path::Path, config: &str) {
    let dir = home.join(".config/osp");
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("config.toml"), config).unwrap();
}

#[cfg(unix)]
fn json_stdout(command: &mut Command) -> Value {
    let output = command.assert().success().get_output().clone();
    serde_json::from_slice(&output.stdout).unwrap()
}

#[cfg(unix)]
fn hello(dir: &std::path::Path) {
    write_plugin(dir, "hello", Some(json!({"about": "Greet people", "output": "json", "args": [{"name": "who", "multi": true}]})),
        "print(json.dumps([{'name': name, 'greeting': 'hello ' + name} for name in sys.argv[1:] or ['world']]))\n");
}
