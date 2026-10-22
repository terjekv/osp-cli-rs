use crate::core::output_model::{Group, OutputDocument, OutputItems, OutputResult};
use crate::core::row::Row;
use serde_json::Value;

pub(crate) fn emit_value(value: &Value) -> String {
    let rendered = serde_json::to_string_pretty(value).unwrap_or_else(|_| "null".into());
    format!("{rendered}\n")
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
    use crate::core::output_model::{OutputDocument, OutputResult};
    use crate::ui::{RenderRequest, RenderSettings, RenderTarget};

    fn render_json(
        value: serde_json::Value,
        settings: &RenderSettings,
        target: RenderTarget,
    ) -> String {
        crate::ui::render(
            &OutputResult::from_rows(Vec::new()).with_document(OutputDocument::Json(value)),
            &RenderRequest {
                target,
                ..RenderRequest::new(settings)
            },
        )
    }

    #[test]
    fn json_emitter_keeps_keys_punctuation_and_scalars_plain_unit() {
        let mut settings = RenderSettings::test_plain(OutputFormat::Json);
        settings.mode = RenderMode::Rich;
        settings.color = ColorMode::Always;
        settings.unicode = UnicodeMode::Always;
        settings.runtime.stdout_is_tty = true;
        settings.theme_name = "dracula".to_string();

        let rendered = render_json(
            json!({
                "count": 42,
                "ok": true,
                "missing": null,
            }),
            &settings,
            RenderTarget::OneShotScreen,
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

        let rendered = render_json(json!({"count": 42}), &settings, RenderTarget::Clipboard);

        assert!(rendered.contains("\"count\": 42"));
        assert!(!rendered.contains("\x1b["));
    }
}
