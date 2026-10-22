use crate::core::style::parse_style_spec;

use crate::ui::theme::ThemeDefinition;

/// Semantic style tokens used across the UI pipeline.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StyleToken {
    Trace,
    Info,
    Warning,
    Error,
    Success,
    Border,
    #[cfg(test)]
    PanelBorder,
    PanelTitle,
    Key,
    TableHeader,
    Text,
    Muted,
    TextMuted,
    PromptText,
    PromptCommand,
    Value,
    ValueNumber,
    BoolTrue,
    BoolFalse,
    Null,
    Ipv4,
    Ipv6,
    Code,
    MessageError,
    MessageWarning,
    MessageSuccess,
    MessageInfo,
    Punctuation,
}

/// Per-token style overrides layered over the resolved theme palette.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct StyleOverrides {
    pub text: Option<String>,
    pub key: Option<String>,
    pub muted: Option<String>,
    pub table_header: Option<String>,
    pub mreg_key: Option<String>,
    pub value: Option<String>,
    pub number: Option<String>,
    pub bool_true: Option<String>,
    pub bool_false: Option<String>,
    pub null_value: Option<String>,
    pub ipv4: Option<String>,
    pub ipv6: Option<String>,
    pub panel_border: Option<String>,
    pub panel_title: Option<String>,
    pub code: Option<String>,
    pub json_key: Option<String>,
    pub message_error: Option<String>,
    pub message_warning: Option<String>,
    pub message_success: Option<String>,
    pub message_info: Option<String>,
    pub message_trace: Option<String>,
}

/// Small styling facade used by emitters and message/chrome helpers.
#[derive(Debug, Clone, Copy)]
pub struct ThemeStyler<'a> {
    pub(crate) enabled: bool,
    pub(crate) theme: &'a ThemeDefinition,
    pub(crate) overrides: &'a StyleOverrides,
    pub(crate) value_token: Option<StyleToken>,
}

impl<'a> ThemeStyler<'a> {
    pub fn new(enabled: bool, theme: &'a ThemeDefinition, overrides: &'a StyleOverrides) -> Self {
        Self {
            enabled,
            theme,
            overrides,
            value_token: None,
        }
    }

    pub fn paint(&self, text: &str, token: StyleToken) -> String {
        apply_style_spec(
            text,
            style_spec(self.theme, self.overrides, token),
            self.enabled,
        )
    }

    pub fn paint_value(&self, text: &str) -> String {
        self.paint(
            text,
            self.value_token.unwrap_or_else(|| value_style_token(text)),
        )
    }

    /// Style inline code without treating ordinary prose as a data value.
    pub(crate) fn paint_inline(&self, text: &str, base: StyleToken) -> String {
        use pulldown_cmark::{Event, Parser};
        let mut out = String::new();
        let mut start = 0;
        for (event, range) in Parser::new(text).into_offset_iter() {
            if let Event::Code(code) = event {
                out.push_str(&self.paint(
                    &super::text::visible_inline_text(&text[start..range.start]),
                    base,
                ));
                out.push_str(&self.paint(&code, StyleToken::Code));
                start = range.end;
            }
        }
        out.push_str(&self.paint(&super::text::visible_inline_text(&text[start..]), base));
        out
    }
}

pub fn apply_style_with_theme(
    text: &str,
    token: StyleToken,
    color: bool,
    theme: &ThemeDefinition,
) -> String {
    apply_style_with_theme_overrides(text, token, color, theme, &StyleOverrides::default())
}

pub fn apply_style_with_theme_overrides(
    text: &str,
    token: StyleToken,
    color: bool,
    theme: &ThemeDefinition,
    overrides: &StyleOverrides,
) -> String {
    if !color || text.is_empty() {
        return text.to_string();
    }

    apply_style_spec(text, style_spec(theme, overrides, token), color)
}

/// Returns the style specification for a semantic token under the selected theme.
pub fn style_spec<'a>(
    theme: &'a ThemeDefinition,
    overrides: &'a StyleOverrides,
    token: StyleToken,
) -> &'a str {
    if let Some(spec) = override_spec(overrides, token) {
        return spec;
    }

    match token {
        #[cfg(test)]
        StyleToken::PanelBorder => theme.palette.border.as_str(),
        StyleToken::Trace | StyleToken::Border => theme.palette.border.as_str(),
        StyleToken::Muted | StyleToken::TextMuted | StyleToken::Null | StyleToken::Punctuation => {
            theme.palette.muted.as_str()
        }
        StyleToken::Info | StyleToken::MessageInfo => theme.palette.info.as_str(),
        StyleToken::Warning | StyleToken::MessageWarning => theme.palette.warning.as_str(),
        StyleToken::Error | StyleToken::MessageError => theme.palette.error.as_str(),
        StyleToken::Success | StyleToken::PromptCommand | StyleToken::MessageSuccess => {
            theme.palette.success.as_str()
        }
        StyleToken::PanelTitle => theme.palette.title.as_str(),
        StyleToken::Code => theme.palette.accent.as_str(),
        StyleToken::Key | StyleToken::TableHeader => theme.palette.accent.as_str(),
        StyleToken::Text | StyleToken::PromptText | StyleToken::Value => {
            theme.palette.text.as_str()
        }
        // Literal data shares one colour; red and green stay for status.
        StyleToken::ValueNumber
        | StyleToken::BoolTrue
        | StyleToken::BoolFalse
        | StyleToken::Ipv4
        | StyleToken::Ipv6 => theme.value_number_spec(),
    }
}

fn override_spec(overrides: &StyleOverrides, token: StyleToken) -> Option<&str> {
    match token {
        StyleToken::PromptCommand => None,
        StyleToken::Trace => overrides
            .message_trace
            .as_deref()
            .or(overrides.panel_border.as_deref()),
        StyleToken::Info | StyleToken::MessageInfo => overrides.message_info.as_deref(),
        StyleToken::Warning | StyleToken::MessageWarning => overrides.message_warning.as_deref(),
        StyleToken::Error | StyleToken::MessageError => overrides
            .message_error
            .as_deref()
            .or(overrides.panel_border.as_deref()),
        StyleToken::Success | StyleToken::MessageSuccess => overrides.message_success.as_deref(),
        #[cfg(test)]
        StyleToken::PanelBorder => overrides.panel_border.as_deref(),
        StyleToken::Border => overrides.panel_border.as_deref(),
        StyleToken::PanelTitle => overrides.panel_title.as_deref(),
        StyleToken::Key => overrides.key.as_deref(),
        StyleToken::TableHeader => overrides
            .table_header
            .as_deref()
            .or(overrides.key.as_deref()),
        StyleToken::Text => overrides.text.as_deref(),
        StyleToken::Muted | StyleToken::TextMuted | StyleToken::Punctuation => {
            overrides.muted.as_deref()
        }
        StyleToken::Code => overrides.code.as_deref().or(overrides.text.as_deref()),
        StyleToken::Value | StyleToken::PromptText => {
            overrides.value.as_deref().or(overrides.text.as_deref())
        }
        StyleToken::ValueNumber => overrides.number.as_deref(),
        StyleToken::BoolTrue => overrides
            .bool_true
            .as_deref()
            .or(overrides.number.as_deref()),
        StyleToken::BoolFalse => overrides
            .bool_false
            .as_deref()
            .or(overrides.number.as_deref()),
        StyleToken::Null => overrides
            .null_value
            .as_deref()
            .or(overrides.muted.as_deref()),
        StyleToken::Ipv4 => overrides.ipv4.as_deref().or(overrides.number.as_deref()),
        StyleToken::Ipv6 => overrides.ipv6.as_deref().or(overrides.number.as_deref()),
    }
}

pub fn value_style_token(value: &str) -> StyleToken {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        return StyleToken::Value;
    }

    // Copyable command lines look like the command typed at the prompt.
    if trimmed.starts_with("osp ") {
        return StyleToken::PromptCommand;
    }

    if let Ok(address) = trimmed.parse::<std::net::IpAddr>() {
        return match address {
            std::net::IpAddr::V4(_) => StyleToken::Ipv4,
            std::net::IpAddr::V6(_) => StyleToken::Ipv6,
        };
    }

    match trimmed.to_ascii_lowercase().as_str() {
        "true" => StyleToken::BoolTrue,
        "false" => StyleToken::BoolFalse,
        "null" | "none" | "nil" | "n/a" => StyleToken::Null,
        _ if trimmed.parse::<f64>().is_ok() => StyleToken::ValueNumber,
        _ => StyleToken::Value,
    }
}

pub fn apply_style_spec(text: &str, spec: &str, enabled: bool) -> String {
    if !enabled || text.is_empty() {
        return text.to_string();
    }

    let Some(style) = parse_style_spec(spec) else {
        return text.to_string();
    };
    let prefix = style.prefix().to_string();
    if prefix.is_empty() {
        return text.to_string();
    }
    format!("{prefix}{text}{}", style.suffix())
}

#[cfg(test)]
mod tests {
    use crate::core::style::is_valid_style_spec;
    use crate::ui::theme::resolve_theme;

    use super::{
        StyleOverrides, StyleToken, ThemeStyler, apply_style_spec, style_spec, value_style_token,
    };

    #[test]
    fn style_tokens_follow_palette_defaults_and_overrides_unit() {
        let rose = resolve_theme("rose-pine-moon");
        let overrides = StyleOverrides::default();
        assert_eq!(
            style_spec(&rose, &overrides, StyleToken::TextMuted),
            rose.palette.muted
        );
        assert_eq!(
            style_spec(&rose, &overrides, StyleToken::PanelTitle),
            rose.palette.title
        );

        let overridden = StyleOverrides {
            muted: Some("yellow".to_string()),
            panel_title: Some("bold blue".to_string()),
            ..StyleOverrides::default()
        };
        assert_eq!(
            style_spec(&rose, &overridden, StyleToken::TextMuted),
            "yellow"
        );
        assert_eq!(
            style_spec(&rose, &overridden, StyleToken::PanelTitle),
            "bold blue"
        );
    }

    #[test]
    fn value_tokens_cover_booleans_null_numbers_and_text_unit() {
        assert_eq!(value_style_token("true"), StyleToken::BoolTrue);
        assert_eq!(value_style_token("false"), StyleToken::BoolFalse);
        assert_eq!(value_style_token("null"), StyleToken::Null);
        assert_eq!(value_style_token("19.2"), StyleToken::ValueNumber);
        assert_eq!(value_style_token("hello"), StyleToken::Value);
        assert_eq!(
            value_style_token("osp orch task info 7"),
            StyleToken::PromptCommand
        );
    }

    #[test]
    fn style_helpers_cover_plain_and_colored_paths_unit() {
        let rose = resolve_theme("rose-pine-moon");
        let overrides = StyleOverrides::default();
        let styler = ThemeStyler::new(true, &rose, &overrides);
        let painted = styler.paint("Errors", StyleToken::MessageError);
        assert!(painted.contains("\u{1b}["));
        assert_eq!(apply_style_spec("x", "wat", true), "x");
        assert!(is_valid_style_spec("bold #abcdef"));
        assert!(!is_valid_style_spec("wat ???"));
    }

    #[test]
    fn style_overrides_cover_value_painting_and_fallback_tokens_unit() {
        let rose = resolve_theme("rose-pine-moon");
        let overrides = StyleOverrides {
            key: Some("green".to_string()),
            number: Some("#123456".to_string()),
            bool_true: Some("#0f0".to_string()),
            message_error: Some("bold red".to_string()),
            ..StyleOverrides::default()
        };
        let styler = ThemeStyler::new(true, &rose, &overrides);

        assert_eq!(
            style_spec(&rose, &overrides, StyleToken::TableHeader),
            "green"
        );
        assert!(styler.paint_value("42").contains("\u{1b}["));
        assert!(
            styler
                .paint("true", StyleToken::BoolTrue)
                .contains("\u{1b}[")
        );
        assert!(
            super::apply_style_with_theme_overrides(
                "boom",
                StyleToken::MessageError,
                true,
                &rose,
                &overrides,
            )
            .contains("\u{1b}[")
        );
        assert!(super::apply_style_spec("x", "bold #abc", true).contains("\u{1b}["));
    }
}
