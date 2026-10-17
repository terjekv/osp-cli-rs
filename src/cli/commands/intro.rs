//! `intro` builtin command implementation.
//!
//! This command reuses the REPL intro/presentation machinery outside the REPL
//! loop so operators can render the current intro/overview surface on demand.

use miette::Result;

use crate::app::CommandOutput;
use crate::app::{AppClients, AppRuntime, AppSession, authorized_command_catalog_for};
use crate::cli::IntroArgs;
use crate::repl::ReplViewContext;
use crate::repl::presentation::{build_repl_intro_payload, repl_document_output};
use crate::repl::surface::ReplSurface;

fn intro_surface<'a>(
    runtime: &'a AppRuntime,
    session: &'a AppSession,
    clients: &AppClients,
    ui: &'a crate::app::UiState,
) -> (ReplViewContext<'a>, ReplSurface) {
    let view = ReplViewContext {
        ui,
        ..ReplViewContext::from_parts(runtime, session)
    };
    let catalog =
        authorized_command_catalog_for(&runtime.auth, clients, clients.native_commands().catalog());
    let surface = crate::repl::surface::build_repl_surface(view, &catalog);
    (view, surface)
}

pub(crate) fn run_intro_command(
    runtime: &AppRuntime,
    session: &AppSession,
    clients: &AppClients,
    ui: &crate::app::UiState,
    _args: IntroArgs,
) -> Result<CommandOutput> {
    let (view, surface) = intro_surface(runtime, session, clients, ui);
    Ok(CommandOutput::output(
        repl_document_output(&build_repl_intro_payload(view, &surface, None)),
        None,
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::{AppState, LaunchContext, RuntimeContext, TerminalKind};
    use crate::config::{ConfigLayer, ConfigResolver, ResolveOptions};
    use crate::core::output::OutputFormat;
    use crate::native::NativeCommandRegistry;
    use crate::plugin::PluginManager;
    use crate::ui::RenderSettings;
    use crate::ui::build_presentation_defaults_layer;
    use crate::ui::messages::MessageLevel;
    use crate::ui::theme_catalog::ThemeCatalog;

    fn resolved_repl_config() -> crate::config::ResolvedConfig {
        let mut defaults = ConfigLayer::default();
        defaults.set("profile.default", "default");
        defaults.set("repl.history.path", "/tmp/osp-intro-history.jsonl");
        defaults.set("theme.path", Vec::<String>::new());

        let mut layers = crate::config::LoadedLayers {
            defaults,
            ..crate::config::LoadedLayers::default()
        };

        let options = ResolveOptions::default().with_terminal("repl");
        let base = ConfigResolver::from_loaded_layers_with_schema(
            layers.clone(),
            crate::config::ConfigSchema::default(),
        )
        .resolve(options.clone())
        .expect("base REPL config should resolve");
        layers.presentation = build_presentation_defaults_layer(&base);
        ConfigResolver::from_loaded_layers_with_schema(
            layers.clone(),
            crate::config::ConfigSchema::default(),
        )
        .resolve(options)
        .expect("presentation-seeded REPL config should resolve")
    }

    fn intro_state() -> AppState {
        crate::app::AppStateBuilder {
            context: RuntimeContext::new(None, TerminalKind::Repl, None),
            config: resolved_repl_config(),
            launch: LaunchContext::default(),
            native_commands: NativeCommandRegistry::default(),
            ui: crate::app::UiState::new(
                RenderSettings::test_plain(OutputFormat::Json),
                MessageLevel::Success,
                0,
            ),
            plugins: Some(
                PluginManager::new(Vec::new())
                    .with_bundled_roots(false)
                    .with_default_roots(false),
            ),
            themes: Some(ThemeCatalog::default()),
            session: None,
            definition: Default::default(),
        }
        .build()
    }

    #[test]
    fn intro_context_projects_visible_surface_for_repl_intro_unit() {
        let state = intro_state();

        let (_, surface) = intro_surface(
            &state.runtime,
            &state.session,
            &state.clients,
            state.runtime.ui(),
        );

        assert!(surface.root_words.iter().any(|word| word == "help"));
        assert!(
            surface
                .intro_commands
                .iter()
                .all(|word| word != "exit" && word != "quit")
        );
        assert!(
            surface
                .overview_entries
                .iter()
                .any(|entry| entry.name == "help")
        );
    }

    #[test]
    fn intro_command_returns_structured_guide_output_unit() {
        let state = intro_state();
        let result = run_intro_command(
            &state.runtime,
            &state.session,
            &state.clients,
            state.runtime.ui(),
            IntroArgs::default(),
        )
        .expect("intro command should succeed");

        assert_eq!(result.exit_code, 0);
        let crate::app::CommandBody::Output(structured) = result.body else {
            panic!("intro command should emit structured output");
        };
        assert!(structured.meta.document_rows);
    }
}
