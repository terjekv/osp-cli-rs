//! Theme picker model; persistence stays in the config command writer.

use std::io::{self, IsTerminal};

use miette::{IntoDiagnostic, Result, miette};

use crate::app::{AppRuntime, AppSession, CommandOutput, UiState};
use crate::core::document::{DocBlock, Document};
use crate::core::output::OutputFormat;
use crate::core::output_model::{OutputDocument, OutputResult};
use crate::ui::messages::{MessageBuffer, MessageLayout, MessageLevel, MessageRequest};
use crate::ui::picker::{self, PickerModel};
use crate::ui::theme::ThemeDefinition;
use crate::ui::{RenderRequest, RenderSettings, render};

pub(super) fn run(
    runtime: &AppRuntime,
    session: &mut AppSession,
    ui: &UiState,
) -> Result<CommandOutput> {
    if !io::stdin().is_terminal() || !io::stdout().is_terminal() || !io::stderr().is_terminal() {
        return Err(miette!(
            "theme preview requires a terminal; use `theme preview NAME` for a non-interactive session preview"
        ));
    }
    let themes = runtime
        .themes
        .entries
        .values()
        .map(|entry| entry.theme.with_color_overrides(runtime.config.resolved()))
        .collect::<Vec<_>>();
    let labels = themes
        .iter()
        .map(|theme| theme.name.clone())
        .collect::<Vec<_>>();
    let initial = themes
        .iter()
        .position(|theme| theme.id == ui.render_settings.theme_name)
        .unwrap_or(0);
    let mut model = Themes {
        themes,
        settings: ui.render_settings.clone(),
    };
    let Some(selected) = picker::run(&labels, initial, &mut model).into_diagnostic()? else {
        return Ok(CommandOutput::exit(0));
    };
    let selected = model.themes[selected].id.clone();
    let output = super::super::config::set_picker_value(
        runtime,
        session,
        ui,
        "theme.name".into(),
        selected.clone(),
        true,
    )?;
    session.config_overrides.replace_scoped(
        "theme.name",
        selected.into(),
        crate::config::Scope::profile_terminal(
            runtime.config.resolved().active_profile(),
            runtime.context.terminal_kind().as_config_terminal(),
        ),
    );
    Ok(output)
}

struct Themes {
    themes: Vec<ThemeDefinition>,
    settings: RenderSettings,
}

impl PickerModel for Themes {
    fn settings(&self, selected: usize) -> RenderSettings {
        let mut settings = self.settings.clone();
        settings.theme_name = self.themes[selected].id.clone();
        settings.theme = Some(self.themes[selected].clone());
        settings
    }

    fn detail(&self, selected: usize, width: usize) -> String {
        let mut settings = self.settings(selected);
        settings.width = Some(width);
        settings.margin = 0;
        settings.format = OutputFormat::Guide;
        let document = Document {
            blocks: vec![DocBlock::section(
                self.themes[selected].name.clone(),
                vec![DocBlock::Data {
                    value: serde_json::json!([
                        {"host": "web01", "state": "ready", "tasks": 3},
                        {"host": "db01", "state": "waiting", "tasks": 0},
                    ]),
                }],
            )],
        };
        let output =
            OutputResult::from_rows(Vec::new()).with_document(OutputDocument::Semantic(document));
        let sample = render(&output, &RenderRequest::new(&settings));
        let mut messages = MessageBuffer::default();
        messages.info("Connected to web01");
        messages.warning("db01 is waiting for capacity");
        messages.error("Task could not start");
        messages.success("Configuration saved");
        let mut request = MessageRequest::new(&settings, MessageLevel::Info);
        request.layout = MessageLayout::Plain;
        format!(
            "{sample}\n\n{}",
            crate::ui::render_messages(&messages, &request)
        )
    }
}
