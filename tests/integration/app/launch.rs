use crate::temp_support::make_temp_dir;
use osp_cli::app::{AppSession, AppStateBuilder, LaunchContext, RuntimeContext, TerminalKind};
use osp_cli::config::{ConfigLayer, ConfigResolver, ResolveOptions};

use super::support::{with_path_prefix, write_executable_script};

fn resolved_config(entries: &[(&str, &str)]) -> osp_cli::config::ResolvedConfig {
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
    .with_session(
        AppSession::default()
            .with_prompt_prefix("demo")
            .with_history_enabled(false),
    )
    .build();

    assert_eq!(state.prompt_prefix(), "demo");

    state.record_repl_failure("ldap user missing", "boom", "boom detail");
    let failure = state
        .last_repl_failure()
        .expect("last failure should be recorded");
    assert_eq!(failure.command_line, "ldap user missing");
    assert_eq!(failure.summary, "boom");
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
