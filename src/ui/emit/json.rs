use serde_json::Value;

use crate::ui::settings::ResolvedRenderSettings;

use super::shared::indent_lines;

pub(crate) fn emit_value(value: &Value, settings: &ResolvedRenderSettings) -> String {
    let rendered = serde_json::to_string_pretty(value).expect("JSON values serialize");
    format!("{}\n", indent_lines(&rendered, settings.margin))
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
