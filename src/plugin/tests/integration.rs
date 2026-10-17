#[cfg(unix)]
use crate::plugin::PluginManager;
#[cfg(unix)]
use crate::tests::make_temp_dir;

#[cfg(unix)]
fn write_executable_script(path: &std::path::Path, script: &str) {
    use std::os::unix::fs::PermissionsExt;

    std::fs::write(path, script).expect("plugin script should be written");
    let mut perms = std::fs::metadata(path)
        .expect("plugin metadata should be readable")
        .permissions();
    perms.set_mode(0o755);
    std::fs::set_permissions(path, perms).expect("plugin script should be executable");
}
#[cfg(unix)]
fn write_session_auth_plugin(dir: &std::path::Path, plugin_id: &str) {
    let plugin_path = dir.join(format!("osp-{plugin_id}"));
    let script = format!(
        r#"#!/bin/sh
PATH=/usr/bin:/bin:$PATH
if [ "$1" = "--osp-describe" ]; then
  cat <<'JSON'
{{"about":"{plugin_id} plugin","auth":{{"visibility":"authenticated","visible_session":{{"auth_strength":"strong"}},"run_session":{{"credentials":[{{"state":"fresh","service":"osp","min_ttl_seconds":600}}]}}}},"args":[],"flags":{{}},"subcommands":[],"output":"json"}}
JSON
  exit 0
fi

cat <<'JSON'
{{"protocol_version":1,"ok":true,"data":{{"message":"ok"}},"error":null,"meta":{{"format_hint":"table","columns":["message"]}}}}
JSON
"#,
        plugin_id = plugin_id,
    );
    write_executable_script(&plugin_path, &script);
}
#[cfg(unix)]
#[test]
fn plugin_manager_projects_session_requirements_into_catalog_and_policy_registry() {
    let root = make_temp_dir("osp-cli-plugin-manager-integration-session-policy");
    let plugins_dir = root.join("plugins");
    std::fs::create_dir_all(&plugins_dir).expect("plugin dir should be created");

    write_session_auth_plugin(&plugins_dir, "secure");
    let manager = PluginManager::new(vec![plugins_dir]);

    let catalog = manager.command_catalog();
    let secure = catalog
        .iter()
        .find(|entry| entry.name == "secure")
        .expect("secure command should exist");
    assert_eq!(secure.auth_hint().as_deref(), Some("auth"));
}

#[cfg(unix)]
fn write_auth_plugin(dir: &std::path::Path, plugin_id: &str) {
    let plugin_path = dir.join(format!("osp-{plugin_id}"));
    let script = format!(
        r#"#!/bin/sh
PATH=/usr/bin:/bin:$PATH
if [ "$1" = "--osp-describe" ]; then
  cat <<'JSON'
{{"about":"{plugin_id} plugin","auth":{{"visibility":"authenticated"}},"args":[],"flags":{{}},"subcommands":[{{"name":"approval","about":"approval commands","args":[],"flags":{{}},"subcommands":[{{"name":"decide","about":"decide approvals","auth":{{"visibility":"capability_gated","required_capabilities":["{plugin_id}.approval.decide"],"feature_flags":["{plugin_id}"]}},"args":[],"flags":{{}},"subcommands":[]}}]}}],"output":"json"}}
JSON
  exit 0
fi

cat <<'JSON'
{{"protocol_version":1,"ok":true,"data":{{"message":"ok"}},"error":null,"meta":{{"format_hint":"table","columns":["message"]}}}}
JSON
"#,
        plugin_id = plugin_id,
    );
    write_executable_script(&plugin_path, &script);
}

#[cfg(unix)]
#[test]
fn plugin_manager_projects_recursive_auth_metadata_into_catalog() {
    let root = make_temp_dir("osp-cli-plugin-manager-integration-policy");
    let plugins_dir = root.join("plugins");
    std::fs::create_dir_all(&plugins_dir).expect("plugin dir should be created");

    write_auth_plugin(&plugins_dir, "orch");
    let manager = PluginManager::new(vec![plugins_dir]);

    let catalog = manager.command_catalog();
    let orch = catalog
        .iter()
        .find(|entry| entry.name == "orch")
        .expect("orch command should exist");
    assert_eq!(orch.auth_hint().as_deref(), Some("auth"));
    assert_eq!(orch.subcommands, vec!["approval".to_string()]);

    assert!(orch.completion.children.contains_key("approval"));
}
