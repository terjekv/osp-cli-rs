//! Assemble the builtin default layer from literal key declarations in `builtin`
//! and computed user, path, and theme defaults. Shared runtime fallbacks also
//! live here; builtin schema metadata and documentation belong to `builtin`.

use super::ConfigLayer;
use super::runtime::RuntimeEnvironment;

/// Default logical profile name used when no profile override is active.
pub(crate) const DEFAULT_PROFILE_NAME: &str = "default";
/// Default maximum number of REPL history entries to keep.
pub(crate) const DEFAULT_REPL_HISTORY_MAX_ENTRIES: i64 = 1000;
/// Default maximum number of rows shown in the REPL history search menu.
pub(crate) const DEFAULT_REPL_HISTORY_MENU_ROWS: i64 = 5;
/// Default maximum terminal width used for rendered output.
pub(crate) const DEFAULT_UI_WIDTH_MAX: i64 = -1;

pub(super) fn build_builtin_defaults(
    env: &RuntimeEnvironment,
    default_theme_name: &str,
    default_repl_prompt: &str,
) -> ConfigLayer {
    let mut layer = ConfigLayer::default();
    seed_literal_defaults(&mut layer);
    seed_computed_defaults(&mut layer, env, default_theme_name, default_repl_prompt);
    layer
}

fn seed_literal_defaults(layer: &mut ConfigLayer) {
    for builtin in super::builtin::builtin_keys() {
        if let Some(value) = &builtin.default {
            layer.set(builtin.key, value.clone());
        }
    }
}

fn seed_computed_defaults(
    layer: &mut ConfigLayer,
    env: &RuntimeEnvironment,
    default_theme_name: &str,
    default_repl_prompt: &str,
) {
    layer.set("theme.name", default_theme_name);
    layer.set("user.name", env.user_name());
    layer.set("domain", env.domain_name());
    layer.set("repl.prompt", default_repl_prompt);
    layer.set("repl.history.path", env.repl_history_path());
    layer.set("log.file.path", env.log_file_path());

    let theme_path = env.theme_paths();
    if !theme_path.is_empty() {
        layer.set("theme.path", theme_path);
    }
}

#[cfg(test)]
#[cfg_attr(coverage_nightly, coverage(off))]
mod tests {
    use super::*;
    use crate::config::{ConfigResolver, ResolveOptions};

    fn resolve_defaults(
        env: RuntimeEnvironment,
        default_theme_name: &str,
        default_repl_prompt: &str,
    ) -> crate::config::ResolvedConfig {
        let mut resolver = ConfigResolver::default();
        resolver.set_defaults(build_builtin_defaults(
            &env,
            default_theme_name,
            default_repl_prompt,
        ));
        resolver
            .resolve(ResolveOptions::default().with_terminal("cli"))
            .expect("builtin defaults should resolve")
    }

    #[test]
    fn builtin_defaults_seed_literal_and_computed_environment_values_unit() {
        let resolved = resolve_defaults(
            RuntimeEnvironment::from_pairs([
                ("XDG_CONFIG_HOME", "/tmp/osp-config"),
                ("XDG_STATE_HOME", "/tmp/osp-state"),
                ("USER", "alice"),
                ("HOSTNAME", "shell.example.com"),
            ]),
            "nord",
            "osp> ",
        );

        assert_eq!(resolved.active_profile(), DEFAULT_PROFILE_NAME);
        assert_eq!(resolved.get_bool("repl.history.enabled"), Some(true));
        assert_eq!(resolved.get_string("theme.name"), Some("nord"));
        assert_eq!(resolved.get_string("user.name"), Some("alice"));
        assert_eq!(resolved.get_string("domain"), Some("example.com"));
        assert_eq!(resolved.get_string("repl.prompt"), Some("osp> "));
        assert_eq!(resolved.get_string("repl.exit_message"), Some(""));
        assert_eq!(
            resolved.get_string_list("theme.path"),
            Some(vec!["/tmp/osp-config/osp/themes".to_string()])
        );
        assert_eq!(
            resolved.get_string("repl.history.path"),
            Some("/tmp/osp-state/osp/history/alice@default.history")
        );
        assert_eq!(
            resolved.get_string("log.file.path"),
            Some("/tmp/osp-state/osp/osp.log")
        );
    }

    #[test]
    fn builtin_defaults_fall_back_without_theme_path_when_config_root_is_missing_unit() {
        let resolved = resolve_defaults(RuntimeEnvironment::defaults_only(), "dracula", "osp> ");

        assert_eq!(resolved.get_string("theme.name"), Some("dracula"));
        assert_eq!(resolved.get_string("user.name"), Some("anonymous"));
        assert_eq!(resolved.get_string("domain"), Some("local"));
        assert_eq!(resolved.get_string_list("theme.path"), None);
        assert!(resolved.get_string("repl.history.path").is_some());
        assert!(resolved.get_string("log.file.path").is_some());
    }
}
