//! Staged config edits using the schema and the ordinary config-set writer.

use std::collections::BTreeSet;
use std::io::{self, IsTerminal};

use miette::{IntoDiagnostic, Result, miette};

use super::*;
use crate::config::SchemaValueType;
use crate::ui::picker::{self, PickerModel};
use crate::ui::style::{StyleToken, ThemeStyler};
use crate::ui::{RenderProfile, RenderSettings, sanitize_human_text};

pub(super) fn run(
    runtime: &AppRuntime,
    session: &mut AppSession,
    ui: &UiState,
    initial_key: Option<String>,
) -> Result<CommandOutput> {
    if !io::stdin().is_terminal() || !io::stdout().is_terminal() || !io::stderr().is_terminal() {
        return Err(miette!(
            "config edit requires a terminal; use `config get KEY` and `config set KEY VALUE`"
        ));
    }
    let schema = ConfigSchema::default();
    let keys = schema
        .entries()
        .map(|(key, _)| key)
        .chain(
            runtime
                .config
                .resolved()
                .values()
                .keys()
                .map(String::as_str),
        )
        .filter(|key| schema.validate_writable_key(key).is_ok())
        .collect::<BTreeSet<_>>();
    let entries = keys
        .into_iter()
        .map(|key| {
            let bootstrap = if is_bootstrap_only_key(key) {
                explain_runtime_config(runtime_request(runtime, session), key)?.final_entry
            } else {
                None
            };
            let resolved = runtime
                .config
                .resolved()
                .get_value_entry(key)
                .or(bootstrap.as_ref());
            let schema_entry = schema
                .entries()
                .find(|(candidate, _)| *candidate == key)
                .map(|(_, entry)| entry);
            let value = resolved
                .map(|entry| config_value_to_json_for_key(key, &entry.value))
                .map(|value| match value {
                    serde_json::Value::String(text) => text,
                    value => value.to_string(),
                })
                .unwrap_or_default();
            let choices = schema_entry
                .map(|entry| {
                    if let Some(values) = entry.allowed_values() {
                        values.to_vec()
                    } else if entry.value_type() == SchemaValueType::Bool {
                        vec!["false".into(), "true".into()]
                    } else {
                        Vec::new()
                    }
                })
                .unwrap_or_default();
            Ok(Entry {
                key: key.into(),
                original: value.clone(),
                value,
                source: resolved
                    .map(|entry| format!("{} ({})", entry.source, format_scope(&entry.scope)))
                    .unwrap_or_else(|| "unset".into()),
                doc: schema.doc_for_key(key).unwrap_or("").into(),
                choices,
                secret: is_sensitive_key(key)
                    || resolved.is_some_and(|entry| entry.value.is_secret()),
            })
        })
        .collect::<Result<Vec<_>>>()?;
    let labels = entries
        .iter()
        .map(|entry| entry.key.clone())
        .collect::<Vec<_>>();
    let initial = match initial_key {
        Some(key) => labels
            .iter()
            .position(|label| label == &key)
            .ok_or_else(|| miette!("unknown or read-only config key: {key}"))?,
        None => 0,
    };
    let mut model = Config {
        entries,
        runtime,
        settings: ui.render_settings.clone(),
        error: String::new(),
    };
    if picker::run(&labels, initial, &mut model)
        .into_diagnostic()?
        .is_none()
    {
        return Ok(CommandOutput::exit(0));
    }
    let mut rows = Vec::new();
    let mut messages = MessageBuffer::default();
    for entry in model
        .entries
        .iter()
        .filter(|entry| entry.value != entry.original)
    {
        let output = set_picker_value(
            runtime,
            session,
            ui,
            entry.key.clone(),
            entry.value.clone(),
            false,
        )?;
        if let crate::app::CommandBody::Output(output) = output.body {
            rows.extend(output.into_rows().unwrap_or_default());
        }
        for message in output.messages.entries() {
            messages.push_message(message.clone());
        }
    }
    if rows.is_empty() {
        return Ok(CommandOutput::exit(0));
    }
    Ok(CommandOutput {
        messages,
        ..CommandOutput::output(rows_to_output_result(rows), None)
    })
}

struct Entry {
    key: String,
    original: String,
    value: String,
    source: String,
    doc: String,
    choices: Vec<String>,
    secret: bool,
}

struct Config<'a> {
    entries: Vec<Entry>,
    runtime: &'a AppRuntime,
    settings: RenderSettings,
    error: String,
}

impl Config<'_> {
    fn validate(&self, selected: usize, value: &str) -> Result<()> {
        let key = &self.entries[selected].key;
        let value = ConfigSchema::default()
            .parse_input_value(key, value)
            .into_diagnostic()?;
        validate_bootstrap_value(key, &value).into_diagnostic()?;
        resolve_edit_target(
            self.runtime,
            key,
            &ConfigScopeArgs::default(),
            &ConfigStoreArgs::default(),
        )?;
        self.runtime
            .config
            .resolved()
            .validate_candidate(key, &value)
            .into_diagnostic()
    }
}

impl PickerModel for Config<'_> {
    fn settings(&self, _selected: usize) -> RenderSettings {
        self.settings.clone()
    }

    fn detail(&self, selected: usize, width: usize) -> String {
        let entry = &self.entries[selected];
        let settings = crate::ui::settings::resolve_settings(&self.settings, RenderProfile::Normal);
        let styler = ThemeStyler::new(settings.color, &settings.theme, &settings.style_overrides);
        let value = if entry.secret {
            "<redacted>"
        } else {
            &entry.value
        };
        let mut lines = Vec::new();
        for (text, token) in [
            (entry.key.clone(), StyleToken::PanelTitle),
            (format!("Value: {value}"), StyleToken::Text),
            (format!("Source: {}", entry.source), StyleToken::Muted),
            (String::new(), StyleToken::Text),
            (entry.doc.clone(), StyleToken::Text),
            (self.error.clone(), StyleToken::Error),
        ] {
            for line in crate::ui::wrap_display_width(&sanitize_human_text(&text), width) {
                lines.push(styler.paint(&line, token));
            }
        }
        lines.join("\n")
    }

    fn edit_value(&self, selected: usize) -> Option<String> {
        Some(if self.entries[selected].secret {
            String::new()
        } else {
            self.entries[selected].value.clone()
        })
    }

    fn secret(&self, selected: usize) -> bool {
        self.entries[selected].secret
    }

    fn set_value(&mut self, selected: usize, value: &str) -> std::result::Result<(), String> {
        self.validate(selected, value)
            .map_err(|error| error.to_string())?;
        self.entries[selected].value = value.into();
        self.error.clear();
        Ok(())
    }

    fn cycle_value(&mut self, selected: usize, backwards: bool) -> bool {
        let entry = &self.entries[selected];
        if entry.choices.is_empty() {
            return false;
        }
        let index = entry
            .choices
            .iter()
            .position(|value| value == &entry.value)
            .unwrap_or(0);
        let next = (index
            + if backwards {
                entry.choices.len() - 1
            } else {
                1
            })
            % entry.choices.len();
        let value = entry.choices[next].clone();
        if let Err(error) = self.set_value(selected, &value) {
            self.error = error;
        }
        true
    }
}
