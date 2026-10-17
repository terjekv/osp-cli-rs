#![allow(dead_code)]

#[cfg(unix)]
use assert_cmd::Command;
#[cfg(unix)]
use std::path::{Path, PathBuf};
#[cfg(unix)]
use std::time::Duration;

#[cfg(unix)]
pub(crate) fn osp_command(home: &Path) -> Command {
    let mut cmd = Command::new(assert_cmd::cargo::cargo_bin!("osp"));
    cmd.timeout(Duration::from_secs(20))
        .env_clear()
        .env("PATH", "/usr/bin:/bin")
        .env("TERM", "xterm-256color")
        .env("LANG", "C.UTF-8")
        .env("NO_COLOR", "1");
    for (key, value) in crate::test_env::isolated_env(home) {
        cmd.env(key, value);
    }
    cmd
}

#[cfg(unix)]
pub(crate) fn write_config(home: &Path, config: &str) {
    let config_dir = home.join(".config").join("osp");
    std::fs::create_dir_all(&config_dir).expect("config dir should be created");
    std::fs::write(config_dir.join("config.toml"), config).expect("config should be written");
}

#[cfg(unix)]
pub(crate) fn stderr_utf8(bytes: Vec<u8>) -> String {
    String::from_utf8(bytes).expect("stderr should be utf-8")
}

#[cfg(unix)]
pub(crate) fn write_executable_script(path: &Path, script: &str) {
    use std::os::unix::fs::PermissionsExt;

    std::fs::write(path, script).expect("script should be written");
    let mut perms = std::fs::metadata(path)
        .expect("script metadata should be readable")
        .permissions();
    perms.set_mode(0o755);
    std::fs::set_permissions(path, perms).expect("script should be executable");
}

#[cfg(unix)]
pub(crate) fn write_table_plugin(
    dir: &Path,
    plugin_id: &str,
    _command_name: &str,
    message: &str,
) -> PathBuf {
    let plugin_path = dir.join(format!("osp-{plugin_id}"));
    let script = format!(
        r#"#!/bin/sh
PATH=/usr/bin:/bin:$PATH
if [ "$1" = "--osp-describe" ]; then
  cat <<'JSON'
{{"about":"{plugin_id} plugin","args":[],"flags":{{}},"subcommands":[],"output":"json"}}
JSON
  exit 0
fi

cat <<'JSON'
{{"protocol_version":1,"ok":true,"data":{{"message":"{message}"}},"error":null,"meta":{{"format_hint":"table","columns":["message"]}}}}
JSON
"#,
        plugin_id = plugin_id,
        message = message,
    );
    write_executable_script(&plugin_path, &script);
    plugin_path
}

#[cfg(unix)]
pub(crate) fn write_nonzero_plugin(
    dir: &Path,
    plugin_id: &str,
    _command_name: &str,
    status_code: i32,
    stderr: &str,
) -> PathBuf {
    let plugin_path = dir.join(format!("osp-{plugin_id}"));
    let script = format!(
        r#"#!/bin/sh
PATH=/usr/bin:/bin:$PATH
if [ "$1" = "--osp-describe" ]; then
  cat <<'JSON'
{{"about":"{plugin_id} plugin","args":[],"flags":{{}},"subcommands":[],"output":"json"}}
JSON
  exit 0
fi

echo "{stderr}" >&2
exit {status_code}
"#,
        plugin_id = plugin_id,
        stderr = stderr,
        status_code = status_code,
    );
    write_executable_script(&plugin_path, &script);
    plugin_path
}

#[cfg(unix)]
pub(crate) fn write_timeout_plugin(dir: &Path, plugin_id: &str, _command_name: &str) -> PathBuf {
    let plugin_path = dir.join(format!("osp-{plugin_id}"));
    let script = format!(
        r#"#!/bin/sh
PATH=/usr/bin:/bin:$PATH
if [ "$1" = "--osp-describe" ]; then
  sleep 1
  cat <<'JSON'
{{"about":"{plugin_id} plugin","args":[],"flags":{{}},"subcommands":[],"output":"json"}}
JSON
  exit 0
fi

sleep 1
cat <<'JSON'
{{"protocol_version":1,"ok":true,"data":{{"message":"late"}},"error":null,"meta":{{"format_hint":"table","columns":["message"]}}}}
JSON
"#,
        plugin_id = plugin_id,
    );
    write_executable_script(&plugin_path, &script);
    plugin_path
}
