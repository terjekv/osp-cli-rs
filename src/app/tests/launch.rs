use crate::app::{AppSession, AppStateBuilder, LaunchContext, RuntimeContext, TerminalKind};
use crate::config::{ConfigLayer, ConfigResolver, ResolveOptions};
use crate::tests::make_temp_dir;

#[cfg(unix)]
use std::path::Path;

use super::plugin_support::write_executable_script;

fn resolved_config(entries: &[(&str, &str)]) -> crate::config::ResolvedConfig {
    let mut defaults = ConfigLayer::default();
    defaults.set("profile.default", "default");
    for (key, value) in entries {
        defaults.set(*key, (*value).to_string());
    }

    let mut resolver = ConfigResolver::default();
    resolver.set_defaults(defaults);
    resolver
        .resolve(ResolveOptions::default().with_terminal("cli"))
        .expect("config should resolve")
}

#[cfg(unix)]
fn write_named_plugin(dir: &std::path::Path, name: &str) {
    let plugin_path = dir.join(format!("osp-{name}"));
    let script = format!(
        r#"#!/bin/sh
PATH=/usr/bin:/bin:$PATH
if [ "$1" = "--osp-describe" ]; then
  cat <<'JSON'
{{"about":"{name} plugin","args":[],"flags":{{}},"subcommands":[],"output":"json"}}
JSON
  exit 0
fi

cat <<'JSON'
{{"protocol_version":1,"ok":true,"data":{{"message":"ok"}},"error":null,"meta":{{"format_hint":"table","columns":["message"]}}}}
JSON
"#,
        name = name,
    );
    write_executable_script(&plugin_path, &script);
}

#[cfg(unix)]
#[test]
fn app_state_builder_uses_launch_context_for_plugin_roots_and_path_discovery() {
    if crate::tests::isolated_test() {
        return;
    }
    let explicit_dir = make_temp_dir("osp-cli-launch-explicit");
    let path_dir = make_temp_dir("osp-cli-launch-path");
    let config_root = make_temp_dir("osp-cli-launch-config-root");
    let cache_root = make_temp_dir("osp-cli-launch-cache-root");
    write_named_plugin(explicit_dir.path(), "explicit-probe");
    write_named_plugin(path_dir.path(), "path-probe");

    let config = resolved_config(&[]);
    let launch = LaunchContext::default()
        .with_plugin_dir(explicit_dir.path().to_path_buf())
        .with_config_root(Some(config_root.path().to_path_buf()))
        .with_cache_root(Some(cache_root.path().to_path_buf()));

    with_path_prefix(path_dir.path(), || {
        let state = AppStateBuilder::from_resolved_config(
            RuntimeContext::new(None, TerminalKind::Cli, None),
            config.clone(),
        )
        .expect("app state builder should derive host inputs")
        .with_launch(launch.clone())
        .build();

        assert_eq!(
            state.runtime.launch().plugin_dirs,
            vec![explicit_dir.path().to_path_buf()]
        );
        assert_eq!(
            state.clients.plugins().config_root(),
            Some(config_root.path())
        );
        assert_eq!(
            state.clients.plugins().cache_root(),
            Some(cache_root.path())
        );

        let plugins = state.clients.plugins().list();
        assert!(plugins.iter().any(|plugin| plugin.name == "explicit-probe"));
        assert!(plugins.iter().any(|plugin| plugin.name == "path-probe"));
    });
}

#[test]
fn app_state_builder_tracks_session_public_helpers_from_resolved_config() {
    let config = resolved_config(&[]);
    let mut state = AppStateBuilder::from_resolved_config(
        RuntimeContext::new(None, TerminalKind::Repl, None),
        config,
    )
    .expect("app state builder should derive host inputs")
    .with_session(AppSession::default().with_history_enabled(false))
    .build();

    assert!(!state.session.history_enabled);

    state.record_repl_failure("ldap user missing", "boom", "boom detail");
    let failure = state
        .last_repl_failure()
        .expect("last failure should be recorded");
    assert_eq!(failure.command_line, "ldap user missing");
    assert_eq!(failure.summary, "boom");
}

#[cfg(unix)]
#[test]
fn explicit_host_config_root_owns_repl_rebuilds_and_plugin_enablement() {
    if crate::tests::isolated_test() {
        return;
    }
    let root = make_temp_dir("osp-cli-host-root");
    let ambient = make_temp_dir("osp-cli-ambient-root");
    let plugins = make_temp_dir("osp-cli-host-plugins");
    let cache = make_temp_dir("osp-cli-host-cache");
    write_named_plugin(plugins.path(), "probe");
    let config_path = root.join("config.toml");
    let ambient_path = ambient.join("config.toml");
    let ambient_config = r#"[default]
user.name = "ambient-user"
extensions.plugins.disabled = ["ambient-disabled"]
"#;
    std::fs::write(&ambient_path, ambient_config).unwrap();
    unsafe { std::env::set_var("OSP_CONFIG_FILE", &ambient_path) };

    let mut state = AppStateBuilder::from_resolved_config(
        RuntimeContext::new(None, TerminalKind::Repl, None),
        resolved_config(&[("user.name", "initial-user")]),
    )
    .unwrap()
    .with_launch(
        LaunchContext::default()
            .with_config_root(Some(root.path().to_path_buf()))
            .with_cache_root(Some(cache.path().to_path_buf()))
            .with_plugin_dir(plugins.path().to_path_buf())
            .with_runtime_load(crate::config::RuntimeLoadOptions::new().with_env(false)),
    )
    .build();
    std::fs::write(
        &config_path,
        r#"[profile.default]
user.name = "reloaded-user"
extensions.plugins.disabled = ["kept-disabled"]
"#,
    )
    .unwrap();
    state = super::super::rebuild_repl_state(&state).expect("host config should reload");
    assert_eq!(
        state.runtime.config().get_string("user.name"),
        Some("reloaded-user")
    );

    let (history, _history_root) = super::make_test_history(&mut state);
    for (action, expected_disabled) in [
        ("disable", vec!["kept-disabled", "probe"]),
        ("enable", vec!["kept-disabled"]),
    ] {
        let executed = crate::repl::dispatch::execute_repl_plugin_line(
            &mut state,
            &history,
            &format!("plugins {action} probe"),
            None,
        )
        .expect("plugin enablement should persist");
        assert!(matches!(
            executed.result,
            crate::repl::ReplResult::Restart { .. }
        ));
        let persisted: toml::Value =
            toml::from_str(&std::fs::read_to_string(&config_path).unwrap()).unwrap();
        assert_eq!(
            persisted["profile"]["default"]["extensions"]["plugins"]["disabled"]
                .as_array()
                .unwrap()
                .iter()
                .map(|value| value.as_str().unwrap())
                .collect::<Vec<_>>(),
            expected_disabled
        );
        assert_eq!(
            std::fs::read_to_string(&ambient_path).unwrap(),
            ambient_config
        );
        state = super::super::rebuild_repl_state(&state).expect("plugin config should reload");
        assert_eq!(
            state
                .runtime
                .config()
                .get_string_list("extensions.plugins.disabled"),
            Some(expected_disabled.into_iter().map(str::to_owned).collect())
        );
        assert_eq!(
            state
                .clients
                .plugins()
                .command_catalog()
                .iter()
                .any(|entry| entry.name == "probe"),
            action == "enable"
        );
    }
}

#[test]
fn app_state_builder_surfaces_invalid_theme_selection_errors() {
    let config = resolved_config(&[("theme.name", "definitely-not-a-theme")]);
    let err = AppStateBuilder::from_resolved_config(
        RuntimeContext::new(None, TerminalKind::Cli, None),
        config,
    )
    .err()
    .expect("invalid theme should fail during host-input derivation");

    let text = err.to_string();
    assert!(text.contains("definitely-not-a-theme"));
    assert!(text.contains("theme"));
}

#[cfg(unix)]
fn with_path_prefix<T>(prefix: &Path, callback: impl FnOnce() -> T) -> T {
    let previous = std::env::var_os("PATH");
    let joined = std::env::join_paths(
        std::iter::once(prefix.to_path_buf())
            .chain(previous.iter().flat_map(std::env::split_paths)),
    )
    .expect("PATH should join");
    unsafe {
        std::env::set_var("PATH", joined);
    }

    let result = callback();

    match previous {
        Some(value) => unsafe { std::env::set_var("PATH", value) },
        None => unsafe { std::env::remove_var("PATH") },
    }

    result
}
