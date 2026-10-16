#![allow(missing_docs)]

//! Canonical UI pipeline.
//!
//! This module owns planning, lowering, and emission for human-facing output.
//! The important split is:
//!
//! - commands, services, and the DSL produce structured output
//! - `ui` decides how that output should be rendered for a human
//! - `ui` does not re-resolve config, re-run commands, or invent new data
//!   semantics
//!
//! Broad-strokes flow:
//!
//! ```text
//! OutputResult / Document
//!        │
//!        ▼ [ plan ]   choose effective format and render settings
//!        ▼ [ lower ]  convert semantic output into one document IR
//!        ▼ [ emit ]   render terminal/markdown/json text
//! ```
//!
//! [`crate::ui::render`] takes canonical output and a
//! [`crate::ui::RenderRequest`]. The layout IR
//! remains private to this module; semantic content stays in `core`.
//!
//! Internal ownership stays intentionally boring:
//!
//! - one planner decides the effective output format
//! - one human-facing document IR carries structure
//! - one lowering pass turns payloads into that IR
//! - one emitter family renders terminal or markdown output
//! - sidecar subsystems such as messages live in their own folders

use std::borrow::Cow;

pub mod clipboard;
pub mod messages;
pub mod section_chrome;
pub mod style;
pub mod theme;
pub mod theme_catalog;

mod chrome;
mod doc;
mod emit;
mod lower;
mod plan;
pub(crate) mod settings;
mod text;

use crate::core::document::DocBlock;
use crate::core::output::OutputFormat;
use crate::core::output_model::{DisplayRule, OutputResult};

pub(crate) use lower::json_value;
pub use messages::{MessageRequest, render as render_messages};
pub(crate) use plan::plan_output;
pub use settings::RenderBackend;
#[allow(unused_imports)]
pub use settings::{
    HelpLayout, PresentationEffect, RenderProfile, RenderRuntime, RenderSettings,
    ResolvedRenderSettings, TableBorderOverride, TableBorderStyle, TableOverflow, UiPresentation,
    help_layout_from_config, resolve_settings,
};
pub(crate) use settings::{build_presentation_defaults_layer, explain_presentation_effect};
pub use style::{StyleOverrides, StyleToken, ThemeStyler};
pub(crate) use text::{
    crop_display_width, display_width, sanitize_human_text, visible_inline_text,
};
pub use theme::DEFAULT_THEME_NAME;

const LOCAL_TIMESTAMP_FORMAT: &str = "%Y-%m-%d %H:%M:%S %:z";

/// Formats an RFC3339 timestamp in local time at seconds precision.
pub fn format_local_timestamp(value: &str) -> Option<String> {
    chrono::DateTime::parse_from_rfc3339(value)
        .ok()
        .map(|time| format_local_utc_timestamp(time.with_timezone(&chrono::Utc)))
}

/// Formats an RFC3339 timestamp as a compact duration from the current time.
pub fn format_relative_timestamp(value: &str) -> Option<String> {
    chrono::DateTime::parse_from_rfc3339(value)
        .ok()
        .map(|time| format_relative_unix_timestamp(time.timestamp()))
}

/// Formats a byte count with the largest useful binary unit.
pub fn format_binary_bytes(bytes: u64) -> String {
    for (unit, divisor) in [
        ("TiB", 1_u64 << 40),
        ("GiB", 1_u64 << 30),
        ("MiB", 1_u64 << 20),
        ("KiB", 1_u64 << 10),
    ] {
        if bytes >= divisor {
            return if bytes.is_multiple_of(divisor) {
                format!("{} {unit}", bytes / divisor)
            } else {
                format!("{:.1} {unit}", bytes as f64 / divisor as f64)
            };
        }
    }
    format!("{bytes} B")
}

fn format_local_utc_timestamp(time: chrono::DateTime<chrono::Utc>) -> String {
    time.with_timezone(&chrono::Local)
        .format(LOCAL_TIMESTAMP_FORMAT)
        .to_string()
}

pub(crate) fn format_local_unix_timestamp(seconds: i64) -> Option<String> {
    chrono::DateTime::from_timestamp(seconds, 0).map(format_local_utc_timestamp)
}

pub(crate) fn format_relative_unix_timestamp(seconds: i64) -> String {
    let delta = seconds.saturating_sub(chrono::Utc::now().timestamp());
    if delta == 0 {
        return "now".to_string();
    }
    let magnitude = delta.unsigned_abs();
    let (amount, unit) = if magnitude < 60 {
        (magnitude, "s")
    } else if magnitude < 60 * 60 {
        (magnitude / 60, "m")
    } else if magnitude < 24 * 60 * 60 {
        (magnitude / (60 * 60), "h")
    } else {
        (magnitude / (24 * 60 * 60), "d")
    };
    if delta < 0 {
        format!("{amount}{unit} ago")
    } else {
        format!("in {amount}{unit}")
    }
}

/// Settings and human presentation for one render operation.
#[derive(Debug, Clone, Copy)]
pub struct RenderRequest<'a> {
    /// Effective output and terminal settings.
    pub settings: &'a RenderSettings,
    /// Normal output or a copy-safe sink.
    pub profile: RenderProfile,
    /// Optional help layout for an unstaged semantic document.
    pub layout: Option<HelpLayout>,
    /// Prefix for the first semantic section title.
    pub title_prefix: Option<&'a str>,
    /// Override the layout's default closing rule.
    pub show_footer_rule: Option<bool>,
    /// Human-only complete lines, supplied by the command or progress sink.
    pub presentation_lines: &'a [String],
}

impl<'a> RenderRequest<'a> {
    /// Uses ordinary output rendering without additional help chrome.
    pub fn new(settings: &'a RenderSettings) -> Self {
        Self {
            settings,
            profile: RenderProfile::Normal,
            layout: None,
            title_prefix: None,
            show_footer_rule: None,
            presentation_lines: &[],
        }
    }
}

/// Renders canonical data, optionally using its unstaged semantic document.
///
/// ```
/// use osp_cli::{row, core::output_model::OutputResult};
/// use osp_cli::core::output::OutputFormat;
/// use osp_cli::ui::{render, RenderRequest, RenderSettings};
///
/// let output = OutputResult::from_rows(vec![row! { "uid" => "alice" }]);
/// let settings = RenderSettings::test_plain(OutputFormat::Markdown);
/// let rendered = render(&output, &RenderRequest::new(&settings));
/// assert!(rendered.contains("alice"));
/// ```
pub fn render(output: &OutputResult, request: &RenderRequest<'_>) -> String {
    if matches!(
        request.settings.format,
        OutputFormat::Csv | OutputFormat::Tsv
    ) {
        return emit::delimited::render(output, request.settings.format);
    }
    let output = prepare_output_for_sink(output, request.settings, request.profile);
    let plan = plan_output(&output, request.settings, request.profile);
    let mut doc = if plan.format != OutputFormat::Json && !request.presentation_lines.is_empty() {
        lower::lower_presentation_lines(request.presentation_lines, plan.settings.width)
    } else if matches!(
        plan.format,
        OutputFormat::Auto | OutputFormat::Guide | OutputFormat::Markdown
    ) && let Some(layout) = request.layout
        && let Some(document) = output
            .document
            .as_ref()
            .and_then(crate::core::output_model::OutputDocument::semantic)
    {
        let mut document = document.clone();
        if let Some(prefix) = request.title_prefix
            && let Some(DocBlock::Section { title, .. }) = document
                .blocks
                .iter_mut()
                .find(|block| matches!(block, DocBlock::Section { .. }))
        {
            *title = format!("{prefix} · {title}");
        }
        if plan.format == OutputFormat::Markdown {
            lower::lower_document(&document, plan.format)
        } else {
            lower::lower_document_help_layout(
                &document,
                &plan,
                layout,
                request
                    .show_footer_rule
                    .unwrap_or_else(|| default_show_footer_rule(layout)),
            )
        }
    } else {
        lower::lower_output(&output, &plan)
    };
    lower::sanitize_doc(&mut doc);
    let text = emit::emit_doc(&doc, plan.format, &plan.settings);
    if plan.settings.backend == RenderBackend::Rich
        && request.settings.runtime.stdout_is_tty
        && request.profile == RenderProfile::Normal
        && !matches!(plan.format, OutputFormat::Json | OutputFormat::Markdown)
    {
        return link_urls(text, &output);
    }
    text
}

fn link_urls(text: String, output: &OutputResult) -> String {
    let mut urls = std::collections::BTreeSet::new();
    for row in output.as_rows().unwrap_or_default() {
        let row = serde_json::Value::Object(row.clone());
        for rule in &output.meta.display_rules {
            if let DisplayRule::Hyperlink { field } = rule
                && let Some(url) =
                    lower::display_path(&row, field).and_then(serde_json::Value::as_str)
                && (url.starts_with("https://") || url.starts_with("http://"))
                && !url.chars().any(char::is_control)
            {
                urls.insert(url.to_owned());
            }
        }
    }
    let mut rendered = String::new();
    let mut remaining = text.as_str();
    while let Some((offset, url)) = urls
        .iter()
        .filter_map(|url| remaining.find(url.as_str()).map(|offset| (offset, url)))
        .min_by_key(|(offset, url)| (*offset, std::cmp::Reverse(url.len())))
    {
        rendered.push_str(&remaining[..offset]);
        rendered.push_str(&format!("\x1b]8;;{url}\x1b\\{url}\x1b]8;;\x1b\\"));
        remaining = &remaining[offset + url.len()..];
    }
    rendered.push_str(remaining);
    rendered
}

fn prepare_output_for_sink<'a>(
    output: &'a OutputResult,
    settings: &RenderSettings,
    profile: RenderProfile,
) -> Cow<'a, OutputResult> {
    if output.meta.display_limit.is_some()
        && (!settings.runtime.stdout_is_tty || profile == RenderProfile::CopySafe)
    {
        let mut unlimited = output.clone();
        unlimited.meta.display_limit = None;
        Cow::Owned(unlimited)
    } else {
        Cow::Borrowed(output)
    }
}

pub(crate) fn copy_output_to_clipboard(
    output: &OutputResult,
    settings: &RenderSettings,
    clipboard: &clipboard::ClipboardService,
) -> Result<(), clipboard::ClipboardError> {
    clipboard.copy_text(&render(
        output,
        &RenderRequest {
            profile: RenderProfile::CopySafe,
            ..RenderRequest::new(settings)
        },
    ))
}

fn default_show_footer_rule(layout: HelpLayout) -> bool {
    matches!(layout, HelpLayout::Full)
}

#[cfg(test)]
mod document_tests;
#[cfg(test)]
mod tests;

pub mod prompt;
