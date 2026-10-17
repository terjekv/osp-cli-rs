//! Rendered timing badges for debug-oriented output.
//!
//! Timing capture happens elsewhere. This module only owns the small
//! formatting/styling policy that turns parse/execute/render durations into a
//! compact badge for prompts, debug output, and other operator-facing surfaces.

use std::time::Duration;

use crate::ui::ResolvedRenderSettings;
use crate::ui::style::{StyleToken, apply_style_with_theme_overrides};

#[derive(Debug, Clone, Copy, Default)]
pub(crate) struct TimingSummary {
    pub(crate) total: Duration,
    pub(crate) parse: Option<Duration>,
    pub(crate) execute: Option<Duration>,
    pub(crate) render: Option<Duration>,
}

pub(crate) fn format_timing_badge(
    summary: TimingSummary,
    debug_level: u8,
    resolved: &ResolvedRenderSettings,
) -> String {
    if debug_level == 0 {
        return String::new();
    }

    let mut text = format_duration(summary.total, debug_level);
    if debug_level >= 3 {
        let mut parts = Vec::new();
        if let Some(parse) = summary.parse {
            parts.push(format!("p{}", format_duration(parse, 2)));
        }
        if let Some(execute) = summary.execute {
            parts.push(format!("e{}", format_duration(execute, 2)));
        }
        if let Some(render) = summary.render {
            parts.push(format!("r{}", format_duration(render, 2)));
        }
        if !parts.is_empty() {
            text.push(' ');
            text.push_str(&parts.join(" "));
        }
    }

    apply_style_with_theme_overrides(
        &text,
        timing_style(summary.total),
        resolved.color,
        &resolved.theme,
        &resolved.style_overrides,
    )
}

fn timing_style(total: Duration) -> StyleToken {
    let total_ms = total.as_millis();
    if total_ms <= 250 {
        StyleToken::MessageSuccess
    } else if total_ms <= 1_000 {
        StyleToken::MessageWarning
    } else {
        StyleToken::MessageError
    }
}

fn format_duration(duration: Duration, debug_level: u8) -> String {
    let secs = duration.as_secs_f64();
    if debug_level <= 1 {
        if duration.as_millis() == 0 && !duration.is_zero() {
            return "<1ms".to_string();
        }
        if secs >= 1.0 {
            return format!("{:.2}s", secs);
        }
        return format!("{}ms", duration.as_millis());
    }

    if secs >= 1.0 {
        return format!("{secs:.2}s");
    }
    let ms = duration.as_secs_f64() * 1_000.0;
    format!("{ms:.1}ms")
}

#[cfg(test)]
mod tests {
    use std::hint::black_box;

    use super::{TimingSummary, format_duration, format_timing_badge, timing_style};
    use crate::ui::RenderSettings;
    use crate::ui::style::StyleToken;
    use std::time::Duration;

    #[test]
    fn level_three_timing_includes_phase_breakdown() {
        let resolved = RenderSettings::test_plain(crate::core::output::OutputFormat::Table)
            .resolve_render_settings();
        let text = format_timing_badge(
            TimingSummary {
                total: Duration::from_millis(187),
                parse: Some(Duration::from_millis(2)),
                execute: Some(Duration::from_millis(180)),
                render: Some(Duration::from_millis(5)),
            },
            3,
            &resolved,
        );

        assert!(text.contains("187.0ms"));
        assert!(text.contains("p2.0ms"));
        assert!(text.contains("e180.0ms"));
        assert!(text.contains("r5.0ms"));
    }

    #[test]
    fn debug_level_zero_and_empty_alignment_yield_no_output() {
        let resolved = RenderSettings::test_plain(crate::core::output::OutputFormat::Table)
            .resolve_render_settings();

        assert!(black_box(format_timing_badge(TimingSummary::default(), 0, &resolved)).is_empty());
    }

    #[test]
    fn duration_and_style_helpers_cover_threshold_edges() {
        assert_eq!(
            black_box(format_duration(Duration::from_nanos(1), 1)),
            "<1ms"
        );
        assert_eq!(
            black_box(format_duration(Duration::from_millis(9), 1)),
            "9ms"
        );
        assert_eq!(
            black_box(format_duration(Duration::from_millis(9), 2)),
            "9.0ms"
        );
        assert_eq!(
            black_box(format_duration(Duration::from_millis(1_250), 1)),
            "1.25s"
        );
        assert_eq!(
            black_box(format_duration(Duration::from_millis(1_250), 2)),
            "1.25s"
        );

        assert_eq!(
            black_box(timing_style(Duration::from_millis(250))),
            StyleToken::MessageSuccess
        );
        assert_eq!(
            black_box(timing_style(Duration::from_millis(251))),
            StyleToken::MessageWarning
        );
        assert_eq!(
            black_box(timing_style(Duration::from_millis(1_001))),
            StyleToken::MessageError
        );
    }
}
