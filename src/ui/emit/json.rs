use crate::core::output_model::{Group, OutputDocument, OutputItems, OutputResult};
use crate::core::row::Row;
use serde_json::Value;

use crate::ui::settings::ResolvedRenderSettings;

use super::shared::indent_lines;

pub(crate) fn emit_value(value: &Value, settings: &ResolvedRenderSettings) -> String {
    let rendered = serde_json::to_string_pretty(value).unwrap_or_else(|_| "null".into());
    format!("{}\n", indent_lines(&rendered, settings.margin))
}

pub(crate) fn json_value(output: &OutputResult) -> Value {
    let data = if let Some(document) = &output.document
        && (output.meta.document_rows || matches!(document, OutputDocument::Json(_)))
    {
        document.json_value()
    } else {
        match &output.items {
            OutputItems::Rows(rows) => {
                Value::Array(rows.iter().cloned().map(Value::Object).collect())
            }
            OutputItems::Groups(groups) => {
                Value::Array(groups.iter().map(json_group_value).collect())
            }
        }
    };
    if output.meta.context.is_empty() {
        data
    } else {
        serde_json::json!({"data": data, "meta": {"context": output.meta.context}})
    }
}

fn json_group_value(group: &Group) -> Value {
    let mut item = Row::new();
    item.insert("groups".to_string(), Value::Object(group.groups.clone()));
    item.insert(
        "aggregates".to_string(),
        Value::Object(group.aggregates.clone()),
    );
    item.insert(
        "rows".to_string(),
        Value::Array(
            group
                .rows
                .iter()
                .cloned()
                .map(Value::Object)
                .collect::<Vec<_>>(),
        ),
    );
    Value::Object(item)
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use crate::core::output::{ColorMode, OutputFormat, RenderMode, UnicodeMode};
    use crate::ui::settings::{RenderProfile, RenderSettings, resolve_settings};

    use super::emit_value;

    #[test]
    fn json_emitter_keeps_keys_punctuation_and_scalars_plain_unit() {
        let mut settings = RenderSettings::test_plain(OutputFormat::Json);
        settings.mode = RenderMode::Rich;
        settings.color = ColorMode::Always;
        settings.unicode = UnicodeMode::Always;
        settings.runtime.stdout_is_tty = true;
        settings.theme_name = "dracula".to_string();

        let resolved = resolve_settings(&settings, RenderProfile::Normal);
        let rendered = emit_value(
            &json!({
                "count": 42,
                "ok": true,
                "missing": null,
            }),
            &resolved,
        );

        assert_eq!(
            serde_json::from_str::<serde_json::Value>(&rendered).unwrap(),
            json!({"count": 42, "ok": true, "missing": null})
        );
        assert!(!rendered.contains("\x1b["));
    }

    #[test]
    fn copy_safe_json_emitter_stays_plain_unit() {
        let mut settings = RenderSettings::test_plain(OutputFormat::Json);
        settings.mode = RenderMode::Rich;
        settings.color = ColorMode::Always;
        settings.runtime.stdout_is_tty = true;
        settings.theme_name = "dracula".to_string();

        let resolved = resolve_settings(&settings, RenderProfile::CopySafe);
        let rendered = emit_value(&json!({"count": 42}), &resolved);

        assert!(rendered.contains("\"count\": 42"));
        assert!(!rendered.contains("\x1b["));
    }
}
