//! Help render bootstrap, visible command rows and verbosity policy.
//!
//! Help output has to work before the full host/runtime is fully running. This
//! module owns the small amount of config/bootstrap logic needed to derive
//! sane render settings for `--help` and related guide surfaces without
//! forcing the rest of the app to special-case that path.

#[cfg(test)]
use std::ffi::OsString;

use crate::config::{ConfigLayer, RuntimeLoadOptions};
use crate::core::document::HelpLevel;
use crate::ui::RenderSettings;
use crate::ui::{HelpLayout, help_layout_from_config};

use crate::ui::UiPresentation;

use super::{
    RuntimeConfigRequest, RuntimeContext, TerminalKind, assembly, normalize_profile_override,
    resolve_runtime_config,
};

pub(crate) fn help_all_output(state: &crate::app::AppState) -> crate::app::CommandOutput {
    let catalog = crate::app::authorized_command_catalog_for(
        &state.runtime.auth,
        &state.clients,
        state.clients.native_commands().completion_catalog(),
    );
    let surface = crate::repl::surface::build_repl_surface(
        crate::repl::ReplViewContext::from_parts(&state.runtime, &state.session),
        &catalog,
    );
    let mut rows = Vec::new();
    for spec in &surface.specs {
        collect_help_paths(spec, "", &mut rows);
    }
    crate::app::CommandOutput::output(
        crate::core::output_model::OutputResult::from_rows(rows),
        None,
    )
}

fn collect_help_paths(
    spec: &crate::completion::CompletionNode,
    prefix: &str,
    rows: &mut Vec<crate::core::row::Row>,
) {
    if spec.value_key || spec.value_leaf || spec.hidden {
        return;
    }
    let path = format!("{prefix}{}", spec.name);
    rows.push(
        crate::row! { "command" => path.clone(), "description" => spec.tooltip.as_deref().unwrap_or("") },
    );
    for child in spec.children.values() {
        collect_help_paths(child, &format!("{path} "), rows);
    }
}

#[derive(Debug, Clone, Default)]
pub(crate) struct HelpRenderOverrides {
    pub(crate) profile: Option<String>,
    pub(crate) theme: Option<String>,
    pub(crate) presentation: Option<UiPresentation>,
    pub(crate) invocation: crate::cli::invocation::InvocationOptions,
    pub(crate) gammel_og_bitter: bool,
    pub(crate) no_env: bool,
    pub(crate) no_config_file: bool,
    pub(crate) defaults_only: bool,
}

impl HelpRenderOverrides {
    fn runtime_load_options(&self) -> RuntimeLoadOptions {
        crate::cli::runtime_load_options_from_flags(
            self.no_env,
            self.no_config_file,
            self.defaults_only,
        )
    }
}

pub(crate) struct HelpRenderContext {
    pub(crate) config: Option<crate::config::ResolvedConfig>,
    pub(crate) settings: RenderSettings,
    pub(crate) layout: HelpLayout,
    pub(crate) help_level: HelpLevel,
}

pub(crate) fn help_level(
    config: &crate::config::ResolvedConfig,
    verbose: u8,
    quiet: u8,
) -> HelpLevel {
    let base = config
        .get_string("ui.help.level")
        .and_then(HelpLevel::parse)
        .unwrap_or(HelpLevel::Normal);
    adjust_help_level(base, verbose, quiet)
}

fn adjust_help_level(base: HelpLevel, verbose: u8, quiet: u8) -> HelpLevel {
    let rank = match base {
        HelpLevel::None => 0,
        HelpLevel::Tiny => 1,
        HelpLevel::Normal => 2,
        HelpLevel::Verbose => 3,
    };
    match (rank + i16::from(verbose) - i16::from(quiet)).clamp(0, 3) {
        0 => HelpLevel::None,
        1 => HelpLevel::Tiny,
        2 => HelpLevel::Normal,
        _ => HelpLevel::Verbose,
    }
}

#[cfg(test)]
pub(crate) fn render_settings_for_help(
    args: &[OsString],
    product_defaults: &ConfigLayer,
    product_overrides: &ConfigLayer,
) -> HelpRenderContext {
    let scanned = crate::cli::invocation::scan_cli_argv(args);
    render_settings_for_help_with_scan(&scanned, product_defaults, product_overrides)
}

pub(crate) fn render_settings_for_help_with_scan(
    scanned: &crate::cli::invocation::ScannedCliArgs,
    product_defaults: &ConfigLayer,
    product_overrides: &ConfigLayer,
) -> HelpRenderContext {
    let overrides = parse_help_render_overrides_with_scan(scanned);
    let profile_override = normalize_profile_override(overrides.profile.clone());
    let mut help_override_layer = product_overrides.clone();
    help_override_layer.extend_from_layer(&build_help_override_layer(&overrides));
    let config = resolve_runtime_config(
        RuntimeConfigRequest::new(profile_override, Some("cli"))
            .with_runtime_load(overrides.runtime_load_options())
            .with_product_defaults(product_defaults.clone())
            .with_session_layer(
                (!help_override_layer.entries().is_empty()).then_some(help_override_layer),
            ),
    )
    .ok();
    let terminal_env = std::env::var("TERM").ok();
    let runtime_context = RuntimeContext::new(None, TerminalKind::Cli, terminal_env.clone());
    let (ui, _) = assembly::build_host(assembly::HostBuildRequest::Presentation {
        context: &runtime_context,
        config: config.as_ref(),
        seed: assembly::RenderSettingsSeed::DefaultAuto,
        theme: None,
        tolerant: true,
    })
    .unwrap_or_else(|_| unreachable!("tolerant presentation cannot fail"))
    .presentation();
    let mut settings = ui.render_settings;
    let mut layout = HelpLayout::Full;
    let effective_help_level;
    if let Some(config) = config.as_ref() {
        layout = help_layout_from_config(config);
        effective_help_level = help_level(
            config,
            overrides.invocation.verbose,
            overrides.invocation.quiet,
        );
    } else {
        effective_help_level = adjust_help_level(
            HelpLevel::Normal,
            overrides.invocation.verbose,
            overrides.invocation.quiet,
        );
    }
    if let Some(format) = overrides.invocation.format {
        settings.format = format;
        settings.format_explicit = true;
    }

    if let Some(mode) = overrides.invocation.mode {
        settings.mode = mode;
    }
    if let Some(color) = overrides.invocation.color {
        settings.color = color;
    }
    if let Some(unicode) = overrides.invocation.unicode {
        settings.unicode = unicode;
    }

    HelpRenderContext {
        config,
        settings,
        layout,
        help_level: effective_help_level,
    }
}

fn build_help_override_layer(overrides: &HelpRenderOverrides) -> ConfigLayer {
    let mut layer = ConfigLayer::default();
    crate::cli::append_appearance_overrides(
        &mut layer,
        overrides.theme.as_deref(),
        if overrides.gammel_og_bitter {
            Some(UiPresentation::Austere)
        } else {
            overrides.presentation
        },
    );

    if let Some(mode) = overrides.invocation.mode {
        layer.set("ui.mode", mode.as_str());
    }

    if let Some(color) = overrides.invocation.color {
        layer.set("ui.color.mode", color.as_str());
    }

    if let Some(unicode) = overrides.invocation.unicode {
        layer.set("ui.unicode.mode", unicode.as_str());
    }

    layer
}

#[cfg(test)]
pub(crate) fn parse_help_render_overrides(args: &[OsString]) -> HelpRenderOverrides {
    let scanned = crate::cli::invocation::scan_cli_argv(args);
    parse_help_render_overrides_with_scan(&scanned)
}

fn parse_help_render_overrides_with_scan(
    scanned: &crate::cli::invocation::ScannedCliArgs,
) -> HelpRenderOverrides {
    let mut out = HelpRenderOverrides {
        invocation: scanned.invocation.clone(),
        ..HelpRenderOverrides::default()
    };
    let source = &scanned.argv;
    let mut iter = source
        .iter()
        .skip(1)
        .filter_map(|value| value.to_str())
        .peekable();

    while let Some(token) = iter.next() {
        if let Some(value) = token.strip_prefix("--profile=") {
            if !value.trim().is_empty() {
                out.profile = Some(value.trim().to_string());
            }
            continue;
        }
        if let Some(value) = token.strip_prefix("--theme=") {
            if !value.trim().is_empty() {
                out.theme = Some(value.trim().to_string());
            }
            continue;
        }
        if let Some(value) = token.strip_prefix("--presentation=") {
            out.presentation = UiPresentation::parse(value);
            continue;
        }

        match token {
            "--profile" => {
                if let Some(value) = iter.peek().copied()
                    && !value.starts_with('-')
                {
                    out.profile = Some(value.to_string());
                    iter.next();
                }
            }
            "--theme" => {
                if let Some(value) = iter.peek().copied()
                    && !value.starts_with('-')
                {
                    out.theme = Some(value.to_string());
                    iter.next();
                }
            }
            "--presentation" => {
                if let Some(value) = iter.peek().copied()
                    && !value.starts_with('-')
                    && let Some(parsed) = UiPresentation::parse(value)
                {
                    out.presentation = Some(parsed);
                    iter.next();
                }
            }
            "--no-env" => out.no_env = true,
            "--no-config" | "--no-config-file" => out.no_config_file = true,
            "--defaults-only" => out.defaults_only = true,
            "--gammel-og-bitter" => out.gammel_og_bitter = true,
            _ => {}
        }
    }

    out
}

#[cfg(test)]
mod tests {
    use super::{parse_help_render_overrides, render_settings_for_help};
    use crate::config::ConfigLayer;
    use crate::core::document::HelpLevel;
    use crate::core::output::{ColorMode, OutputFormat, RenderMode, UnicodeMode};
    use crate::ui::HelpLayout;
    use std::ffi::OsString;

    fn with_test_xdg_env<T>(run: impl FnOnce() -> T) -> T {
        run()
    }

    fn help_args(args: &[&str]) -> Vec<OsString> {
        args.iter().map(OsString::from).collect()
    }

    #[cfg_attr(miri, ignore = "help bootstrap config/theme integration test")]
    #[test]
    fn render_settings_for_help_combines_presentation_format_and_level_overrides_unit() {
        if crate::tests::isolated_test() {
            return;
        }
        with_test_xdg_env(|| {
            let mut product_defaults = ConfigLayer::default();
            product_defaults.set("theme.path", Vec::<String>::new());
            let context = render_settings_for_help(
                &help_args(&["osp", "--gammel-og-bitter", "--defaults-only"]),
                &product_defaults,
                &ConfigLayer::default(),
            );

            assert_eq!(context.layout, HelpLayout::Minimal);
            assert_eq!(context.help_level, HelpLevel::Normal);
            assert_eq!(context.settings.mode, RenderMode::Plain);
            assert_eq!(context.settings.color, ColorMode::Never);
            assert_eq!(context.settings.unicode, UnicodeMode::Never);
            assert_eq!(context.settings.format, OutputFormat::Auto);
            assert!(!context.settings.format_explicit);

            let context = render_settings_for_help(
                &help_args(&[
                    "osp",
                    "--presentation",
                    "compact",
                    "--render-mode",
                    "rich",
                    "--color",
                    "always",
                    "--unicode",
                    "always",
                    "--defaults-only",
                ]),
                &product_defaults,
                &ConfigLayer::default(),
            );

            assert_eq!(context.layout, HelpLayout::Compact);
            assert_eq!(context.help_level, HelpLevel::Normal);
            assert_eq!(context.settings.mode, RenderMode::Rich);
            assert_eq!(context.settings.color, ColorMode::Always);
            assert_eq!(context.settings.unicode, UnicodeMode::Always);
            assert_eq!(context.settings.format, OutputFormat::Auto);
            assert!(!context.settings.format_explicit);

            let context = render_settings_for_help(
                &help_args(&["osp", "--json", "--defaults-only"]),
                &product_defaults,
                &ConfigLayer::default(),
            );

            assert_eq!(context.settings.format, OutputFormat::Json);
            assert!(context.settings.format_explicit);
            let context = render_settings_for_help(
                &help_args(&["osp", "--format=auto", "--defaults-only"]),
                &product_defaults,
                &ConfigLayer::default(),
            );

            assert_eq!(context.settings.format, OutputFormat::Auto);
            assert!(context.settings.format_explicit);

            for (args, expected_level) in [
                (&["osp", "-v", "--defaults-only"][..], HelpLevel::Verbose),
                (&["osp", "-q", "--defaults-only"][..], HelpLevel::Tiny),
                (&["osp", "-qq", "--defaults-only"][..], HelpLevel::None),
            ] {
                let context = render_settings_for_help(
                    &help_args(args),
                    &product_defaults,
                    &ConfigLayer::default(),
                );
                assert_eq!(context.help_level, expected_level);
            }
        });
    }

    #[test]
    fn parse_help_render_overrides_handle_inline_assignments_invalid_values_and_flags_unit() {
        let parsed = parse_help_render_overrides(&[
            OsString::from("osp"),
            OsString::from("--profile=prod"),
            OsString::from("--theme=nord"),
            OsString::from("--presentation=compact"),
            OsString::from("--render-mode=plain"),
            OsString::from("--color=always"),
            OsString::from("--unicode=never"),
        ]);

        assert_eq!(parsed.profile.as_deref(), Some("prod"));
        assert_eq!(parsed.theme.as_deref(), Some("nord"));
        assert_eq!(
            parsed.presentation,
            Some(crate::ui::UiPresentation::Compact)
        );
        assert_eq!(parsed.invocation.mode, Some(RenderMode::Plain));
        assert_eq!(parsed.invocation.color, Some(ColorMode::Always));
        assert_eq!(parsed.invocation.unicode, Some(UnicodeMode::Never));

        let parsed = parse_help_render_overrides(&[
            OsString::from("osp"),
            OsString::from("--presentation"),
            OsString::from("loud"),
            OsString::from("--render-mode=LOUD"),
            OsString::from("--color=sideways"),
            OsString::from("--unicode"),
            OsString::from("sometimes"),
            OsString::from("--profile"),
            OsString::from("dev"),
        ]);

        assert_eq!(parsed.presentation, None);
        assert_eq!(parsed.invocation.mode, None);
        assert_eq!(parsed.invocation.color, None);
        assert_eq!(parsed.invocation.unicode, None);
        assert_eq!(parsed.profile.as_deref(), Some("dev"));

        let parsed = parse_help_render_overrides(&help_args(&[
            "osp",
            "--format=auto",
            "--render-mode",
            "rich",
            "--color",
            "always",
            "--unicode",
            "never",
            "-vq",
            "--verbose",
            "--quiet",
            "--ascii",
            "--no-env",
            "--no-config",
            "--defaults-only",
        ]));
        assert_eq!(parsed.invocation.format, Some(OutputFormat::Auto));
        assert_eq!(parsed.invocation.mode, Some(RenderMode::Rich));
        assert_eq!(parsed.invocation.color, Some(ColorMode::Always));
        assert_eq!(parsed.invocation.verbose, 2);
        assert_eq!(parsed.invocation.quiet, 2);
        assert_eq!(parsed.invocation.unicode, Some(UnicodeMode::Never));
        assert!(parsed.no_env);
        assert!(parsed.no_config_file);
        assert!(parsed.defaults_only);
    }

    #[test]
    fn help_arg_parsers_accept_case_whitespace_and_invalid_values_unit() {
        assert_eq!(RenderMode::parse(" rich "), Some(RenderMode::Rich));
        assert_eq!(RenderMode::parse("LOUD"), None);
        assert_eq!(ColorMode::parse(" WARNING "), None);
        assert_eq!(ColorMode::parse(" Always "), Some(ColorMode::Always));
        assert_eq!(UnicodeMode::parse(" Never "), Some(UnicodeMode::Never));
        assert_eq!(UnicodeMode::parse("maybe"), None);
    }
}
