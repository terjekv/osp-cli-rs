use super::*;

fn env_lock() -> &'static std::sync::Mutex<()> {
    crate::tests::env_lock()
}

fn with_test_xdg_env<T>(callback: impl FnOnce() -> T) -> T {
    let _guard = env_lock()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    let original_home = std::env::var("HOME").ok();
    let original_xdg_config_home = std::env::var("XDG_CONFIG_HOME").ok();
    let original_xdg_cache_home = std::env::var("XDG_CACHE_HOME").ok();
    let original_xdg_data_home = std::env::var("XDG_DATA_HOME").ok();

    unsafe {
        std::env::set_var("HOME", "/tmp/osp-repl-runtime-home");
        std::env::set_var("XDG_CONFIG_HOME", "/tmp/osp-repl-runtime-xdg/config");
        std::env::set_var("XDG_CACHE_HOME", "/tmp/osp-repl-runtime-xdg/cache");
        std::env::set_var("XDG_DATA_HOME", "/tmp/osp-repl-runtime-xdg/data");
    }

    let result = callback();

    match original_home {
        Some(value) => unsafe { std::env::set_var("HOME", value) },
        None => unsafe { std::env::remove_var("HOME") },
    }
    match original_xdg_config_home {
        Some(value) => unsafe { std::env::set_var("XDG_CONFIG_HOME", value) },
        None => unsafe { std::env::remove_var("XDG_CONFIG_HOME") },
    }
    match original_xdg_cache_home {
        Some(value) => unsafe { std::env::set_var("XDG_CACHE_HOME", value) },
        None => unsafe { std::env::remove_var("XDG_CACHE_HOME") },
    }
    match original_xdg_data_home {
        Some(value) => unsafe { std::env::set_var("XDG_DATA_HOME", value) },
        None => unsafe { std::env::remove_var("XDG_DATA_HOME") },
    }

    result
}

include!("repl_runtime/plugin_dispatch.rs");
include!("repl_runtime/rebuild_restart.rs");
include!("repl_runtime/session_shell.rs");

#[test]
fn repl_flag_prefixed_help_records_prompt_timing_badge_unit() {
    let mut state = make_test_state(Vec::new());
    let history = make_test_history(&mut state);

    let rendered = repl_dispatch::execute_repl_plugin_line(
        &mut state.runtime,
        &mut state.session,
        &state.clients,
        &history,
        "-ddd help config",
    )
    .expect("flag-prefixed help should render successfully");

    match rendered {
        crate::repl::ReplResult::Print(text) => {
            assert!(text.contains("config <COMMAND>"));
        }
        other => panic!("unexpected repl result: {other:?}"),
    }

    let badge = state
        .session
        .prompt_timing
        .badge()
        .expect("help flow should update prompt timing");
    assert_eq!(badge.level, 3);

    let prompt_right = crate::repl::render_repl_prompt_right_for_test(
        &state.runtime.ui.render_settings.resolve_render_settings(),
        true,
        &state.session.prompt_timing,
    );
    assert!(
        prompt_right.contains("ms"),
        "unexpected prompt right: {prompt_right:?}"
    );
    assert!(
        prompt_right.contains('p'),
        "unexpected prompt right: {prompt_right:?}"
    );
}
