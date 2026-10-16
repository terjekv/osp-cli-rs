//! Reusable section chrome helpers for messages, help, and guide rendering.
//!
//! This module exists so the rest of the UI can ask for titled sections and
//! framed blocks without duplicating border logic in every renderer. It keeps
//! section-frame policy, title styling, and ASCII/Unicode fallback behavior in
//! one place.
//!
//! Contract:
//!
//! - chrome helpers may depend on theme/style resolution
//! - they should not decide *when* sections are shown, only how a requested
//!   section frame is rendered

use crate::ui::style::{StyleOverrides, StyleToken, apply_style_with_theme_overrides};
use crate::ui::text::display_width;
use crate::ui::theme::ThemeDefinition;

/// Frame style used when rendering section chrome.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum SectionFrameStyle {
    /// Render no surrounding frame.
    None,
    /// Render a top rule only.
    #[default]
    Top,
    /// Render a bottom rule only.
    Bottom,
    /// Render both top and bottom rules.
    TopBottom,
    /// Render a square boxed frame.
    Square,
    /// Render a rounded boxed frame.
    Round,
}

/// Placement policy for ruled section separators across sibling sections.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum RuledSectionPolicy {
    /// Render each section independently according to its frame style.
    #[default]
    PerSection,
    /// Share titled top rules between sibling sections and close the list once.
    Shared,
}

impl SectionFrameStyle {
    /// All variants in canonical configuration order.
    pub const ALL: [Self; 6] = [
        Self::None,
        Self::Top,
        Self::Bottom,
        Self::TopBottom,
        Self::Square,
        Self::Round,
    ];

    /// Parses the section-frame spellings accepted by configuration.
    ///
    /// # Examples
    ///
    /// ```
    /// use osp_cli::ui::section_chrome::SectionFrameStyle;
    ///
    /// assert_eq!(SectionFrameStyle::parse("rules"), Some(SectionFrameStyle::TopBottom));
    /// assert_eq!(SectionFrameStyle::parse("boxed"), Some(SectionFrameStyle::Square));
    /// assert_eq!(SectionFrameStyle::parse("wat"), None);
    /// ```
    pub fn parse(value: &str) -> Option<Self> {
        match value.trim().to_ascii_lowercase().as_str() {
            "none" | "plain" => Some(Self::None),
            "top" | "rule-top" => Some(Self::Top),
            "bottom" | "rule-bottom" => Some(Self::Bottom),
            "top-bottom" | "both" | "rules" => Some(Self::TopBottom),
            "square" | "box" | "boxed" => Some(Self::Square),
            "round" | "rounded" => Some(Self::Round),
            _ => None,
        }
    }

    /// Returns the canonical configuration spelling.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::None => "none",
            Self::Top => "top",
            Self::Bottom => "bottom",
            Self::TopBottom => "top-bottom",
            Self::Square => "square",
            Self::Round => "round",
        }
    }
}

impl RuledSectionPolicy {
    /// All variants in canonical configuration order.
    pub const ALL: [Self; 2] = [Self::PerSection, Self::Shared];

    /// Parses the ruled-section policy spellings accepted by configuration.
    pub fn parse(value: &str) -> Option<Self> {
        match value.trim().to_ascii_lowercase().as_str() {
            "per-section" | "independent" | "separate" => Some(Self::PerSection),
            "shared" | "stacked" | "list" => Some(Self::Shared),
            _ => None,
        }
    }

    /// Returns the canonical configuration spelling.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::PerSection => "per-section",
            Self::Shared => "shared",
        }
    }
}

/// Style tokens applied to section borders and titles.
#[derive(Debug, Clone, Copy)]
pub struct SectionStyleTokens {
    /// Style token used for borders and rules.
    pub border: StyleToken,
    /// Style token used for titles.
    pub title: StyleToken,
}

impl SectionStyleTokens {
    /// Uses the same style token for both the border and the title.
    ///
    /// # Examples
    ///
    /// ```
    /// use osp_cli::ui::section_chrome::SectionStyleTokens;
    /// use osp_cli::ui::style::StyleToken;
    ///
    /// let tokens = SectionStyleTokens::same(StyleToken::PanelTitle);
    ///
    /// assert_eq!(tokens.border, StyleToken::PanelTitle);
    /// assert_eq!(tokens.title, StyleToken::PanelTitle);
    /// ```
    pub const fn same(token: StyleToken) -> Self {
        Self {
            border: token,
            title: token,
        }
    }
}

/// Context required to render section chrome with semantic styling.
#[derive(Clone, Copy)]
pub struct SectionRenderContext<'a> {
    /// Whether ANSI color output is enabled.
    pub color: bool,
    /// Active theme used for style resolution.
    pub theme: &'a ThemeDefinition,
    /// Explicit style overrides layered over the theme.
    pub style_overrides: &'a StyleOverrides,
}

impl SectionRenderContext<'_> {
    fn style(self, text: &str, token: StyleToken) -> String {
        if self.color {
            apply_style_with_theme_overrides(text, token, true, self.theme, self.style_overrides)
        } else {
            text.to_string()
        }
    }
}

/// Presentation of a single titled section divider.
#[derive(Clone, Copy)]
pub struct SectionDividerRequest<'a> {
    /// Use Unicode rule characters.
    pub unicode: bool,
    /// Target divider width.
    pub width: Option<usize>,
    /// Columns consumed before the title starts; at least two.
    pub title_columns: usize,
    /// Theme and style overrides.
    pub render: SectionRenderContext<'a>,
    /// Border and title styling.
    pub tokens: SectionStyleTokens,
}

/// Renders a section divider using the requested width and styling.
pub fn render_section_divider(title: &str, request: &SectionDividerRequest<'_>) -> String {
    let SectionDividerRequest {
        unicode,
        width,
        title_columns,
        render,
        tokens,
    } = *request;
    let border_token = tokens.border;
    let title_token = tokens.title;
    let fill_char = if unicode { '─' } else { '-' };
    let target_width = width.unwrap_or(12).max(12);
    let title = title.trim();

    let raw = if title.is_empty() {
        fill_char.to_string().repeat(target_width)
    } else {
        let title_columns = title_columns.max(2);
        let prefix = format!(
            "{} {title} ",
            fill_char
                .to_string()
                .repeat(title_columns.saturating_sub(1))
        );
        let prefix_width = display_width(&prefix);
        if prefix_width >= target_width {
            prefix
        } else {
            format!(
                "{prefix}{}",
                fill_char.to_string().repeat(target_width - prefix_width)
            )
        }
    };

    if !render.color {
        return raw;
    }

    if title.is_empty() || title_token == border_token {
        return render.style(&raw, border_token);
    }

    let title_columns = title_columns.max(2);
    let prefix = format!(
        "{} ",
        fill_char
            .to_string()
            .repeat(title_columns.saturating_sub(1))
    );
    let title_text = title;
    let prefix_width = display_width(&prefix);
    let title_width = display_width(title_text);
    let base_width = prefix_width + title_width + 1;
    let fill_len = target_width.saturating_sub(base_width);
    let suffix = if fill_len == 0 {
        " ".to_string()
    } else {
        format!(" {}", fill_char.to_string().repeat(fill_len))
    };

    let styled_prefix = render.style(&prefix, border_token);
    let styled_title = render.style(title_text, title_token);
    let styled_suffix = render.style(&suffix, border_token);
    format!("{styled_prefix}{styled_title}{styled_suffix}")
}

/// Renders one titled section body with the requested frame and style tokens.
///
/// The returned text is newline-free at the end so callers can compose several
/// sections without trimming renderer output.
///
/// # Examples
///
/// ```
/// use osp_cli::ui::section_chrome::{
///     SectionFrameStyle, SectionRenderContext, SectionStyleTokens,
///     render_section_block_with_overrides,
/// };
/// use osp_cli::ui::style::{StyleOverrides, StyleToken};
/// use osp_cli::ui::theme::resolve_theme;
///
/// let theme = resolve_theme("plain");
/// let rendered = render_section_block_with_overrides(
///     "Errors",
///     "- bad",
///     SectionFrameStyle::TopBottom,
///     false,
///     Some(18),
///     SectionRenderContext {
///         color: false,
///         theme: &theme,
///         style_overrides: &StyleOverrides::default(),
///     },
///     SectionStyleTokens::same(StyleToken::MessageError),
/// );
///
/// assert!(rendered.contains("Errors"));
/// assert!(rendered.contains("- bad"));
/// ```
pub fn render_section_block_with_overrides(
    title: &str,
    body: &str,
    frame_style: SectionFrameStyle,
    unicode: bool,
    width: Option<usize>,
    render: SectionRenderContext<'_>,
    tokens: SectionStyleTokens,
) -> String {
    match frame_style {
        SectionFrameStyle::None => render_plain_section(title, body, render, tokens.title),
        SectionFrameStyle::Top => {
            render_ruled_section(title, body, true, false, unicode, width, render, tokens)
        }
        SectionFrameStyle::Bottom => {
            render_ruled_section(title, body, false, true, unicode, width, render, tokens)
        }
        SectionFrameStyle::TopBottom => {
            render_ruled_section(title, body, true, true, unicode, width, render, tokens)
        }
        SectionFrameStyle::Square => render_boxed_section(
            title,
            body,
            unicode,
            render,
            tokens,
            BoxFrameChars::square(unicode),
        ),
        SectionFrameStyle::Round => render_boxed_section(
            title,
            body,
            unicode,
            render,
            tokens,
            BoxFrameChars::round(unicode),
        ),
    }
}

fn render_plain_section(
    title: &str,
    body: &str,
    render: SectionRenderContext<'_>,
    title_token: StyleToken,
) -> String {
    let mut out = String::new();
    let title = title.trim();
    let body = body.trim_end_matches('\n');

    if !title.is_empty() {
        let raw_title = format!("{title}:");
        out.push_str(&style_segment(&raw_title, render, title_token));
        if !body.is_empty() {
            out.push('\n');
        }
    }
    if !body.is_empty() {
        out.push_str(body);
    }
    out
}

#[allow(clippy::too_many_arguments)]
fn render_ruled_section(
    title: &str,
    body: &str,
    top_rule: bool,
    bottom_rule: bool,
    unicode: bool,
    width: Option<usize>,
    render: SectionRenderContext<'_>,
    tokens: SectionStyleTokens,
) -> String {
    let mut out = String::new();
    let body = body.trim_end_matches('\n');
    let title = title.trim();

    if top_rule {
        out.push_str(&render_section_divider(
            title,
            &crate::ui::section_chrome::SectionDividerRequest {
                unicode,
                width,
                title_columns: 2,
                render,
                tokens,
            },
        ));
    } else if !title.is_empty() {
        let raw_title = format!("{title}:");
        out.push_str(&style_segment(&raw_title, render, tokens.title));
    }

    if !body.is_empty() {
        if !out.is_empty() {
            out.push('\n');
        }
        out.push_str(body);
    }

    if bottom_rule {
        if !out.is_empty() {
            out.push('\n');
        }
        out.push_str(&render_section_divider(
            "",
            &crate::ui::section_chrome::SectionDividerRequest {
                unicode,
                width,
                title_columns: 2,
                render,
                tokens: SectionStyleTokens::same(tokens.border),
            },
        ));
    }

    out
}

#[derive(Debug, Clone, Copy)]
struct BoxFrameChars {
    top_left: char,
    top_right: char,
    bottom_left: char,
    bottom_right: char,
    horizontal: char,
    vertical: char,
}

impl BoxFrameChars {
    fn square(unicode: bool) -> Self {
        if unicode {
            Self {
                top_left: '┌',
                top_right: '┐',
                bottom_left: '└',
                bottom_right: '┘',
                horizontal: '─',
                vertical: '│',
            }
        } else {
            Self {
                top_left: '+',
                top_right: '+',
                bottom_left: '+',
                bottom_right: '+',
                horizontal: '-',
                vertical: '|',
            }
        }
    }

    fn round(unicode: bool) -> Self {
        if unicode {
            Self {
                top_left: '╭',
                top_right: '╮',
                bottom_left: '╰',
                bottom_right: '╯',
                horizontal: '─',
                vertical: '│',
            }
        } else {
            Self::square(false)
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn render_boxed_section(
    title: &str,
    body: &str,
    _unicode: bool,
    render: SectionRenderContext<'_>,
    tokens: SectionStyleTokens,
    chars: BoxFrameChars,
) -> String {
    let lines = section_body_lines(body);
    let title = title.trim();
    let body_width = lines
        .iter()
        .map(|line| display_width(line))
        .max()
        .unwrap_or(0);
    let title_width = if title.is_empty() {
        0
    } else {
        display_width(title) + 2
    };
    let inner_width = body_width.max(title_width).max(8);

    let mut out = String::new();
    out.push_str(&render_box_top(title, inner_width, chars, render, tokens));

    if !lines.is_empty() {
        out.push('\n');
    }

    for (index, line) in lines.iter().enumerate() {
        if index > 0 {
            out.push('\n');
        }
        out.push_str(&render_box_body_line(
            line,
            inner_width,
            chars,
            render,
            tokens.border,
        ));
    }

    if !out.is_empty() {
        out.push('\n');
    }
    out.push_str(&style_segment(
        &format!(
            "{}{}{}",
            chars.bottom_left,
            chars.horizontal.to_string().repeat(inner_width + 2),
            chars.bottom_right
        ),
        render,
        tokens.border,
    ));
    out
}

fn render_box_top(
    title: &str,
    inner_width: usize,
    chars: BoxFrameChars,
    render: SectionRenderContext<'_>,
    tokens: SectionStyleTokens,
) -> String {
    if title.is_empty() {
        return style_segment(
            &format!(
                "{}{}{}",
                chars.top_left,
                chars.horizontal.to_string().repeat(inner_width + 2),
                chars.top_right
            ),
            render,
            tokens.border,
        );
    }

    let title_width = display_width(title);
    let remaining = inner_width.saturating_sub(title_width);
    let left = format!("{} ", chars.top_left);
    let right = format!(
        " {}{}",
        chars.horizontal.to_string().repeat(remaining),
        chars.top_right
    );

    format!(
        "{}{}{}",
        style_segment(&left, render, tokens.border,),
        style_segment(title, render, tokens.title),
        style_segment(&right, render, tokens.border,),
    )
}

fn render_box_body_line(
    line: &str,
    inner_width: usize,
    chars: BoxFrameChars,
    render: SectionRenderContext<'_>,
    border_token: StyleToken,
) -> String {
    let padding = inner_width.saturating_sub(display_width(line));
    let left = format!("{} ", chars.vertical);
    let right = format!("{} {}", " ".repeat(padding), chars.vertical);
    format!(
        "{}{}{}",
        style_segment(&left, render, border_token,),
        line,
        style_segment(&right, render, border_token,),
    )
}

fn style_segment(text: &str, render: SectionRenderContext<'_>, token: StyleToken) -> String {
    render.style(text, token)
}

fn section_body_lines(body: &str) -> Vec<&str> {
    body.trim_end_matches('\n')
        .lines()
        .map(str::trim_end)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::{
        SectionFrameStyle, SectionRenderContext, SectionStyleTokens,
        render_section_block_with_overrides, render_section_divider,
    };
    use unicode_width::UnicodeWidthStr;

    #[cfg_attr(miri, ignore = "terminal environment integration test")]
    #[test]
    fn section_divider_ignores_columns_env_without_explicit_width() {
        if crate::tests::isolated_test() {
            return;
        }

        let original = std::env::var("COLUMNS").ok();
        unsafe {
            std::env::set_var("COLUMNS", "99");
        }

        let divider = render_section_divider(
            "",
            &crate::ui::section_chrome::SectionDividerRequest {
                unicode: false,
                width: None,
                title_columns: 2,
                render: SectionRenderContext {
                    color: false,
                    theme: &crate::ui::theme::resolve_theme(crate::ui::theme::DEFAULT_THEME_NAME),
                    style_overrides: &crate::ui::StyleOverrides::default(),
                },
                tokens: SectionStyleTokens::same(crate::ui::style::StyleToken::PanelBorder),
            },
        );

        match original {
            Some(value) => unsafe { std::env::set_var("COLUMNS", value) },
            None => unsafe { std::env::remove_var("COLUMNS") },
        }

        assert_eq!(divider.len(), 12);
    }

    #[test]
    fn section_divider_can_style_border_and_title_separately() {
        let theme = crate::ui::theme::resolve_theme("dracula");
        let overrides = crate::ui::style::StyleOverrides {
            panel_border: Some("#112233".to_string()),
            panel_title: Some("#445566".to_string()),
            ..Default::default()
        };
        let divider = render_section_divider(
            "Info",
            &crate::ui::section_chrome::SectionDividerRequest {
                unicode: true,
                width: Some(20),
                title_columns: 2,
                render: SectionRenderContext {
                    color: true,
                    theme: &theme,
                    style_overrides: &overrides,
                },
                tokens: SectionStyleTokens {
                    border: crate::ui::style::StyleToken::PanelBorder,
                    title: crate::ui::style::StyleToken::PanelTitle,
                },
            },
        );

        assert!(divider.starts_with("\x1b[38;2;17;34;51m"));
        assert!(divider.contains("\x1b[38;2;68;85;102mInfo\x1b[0m"));
        assert!(divider.ends_with("\x1b[0m"));
    }

    #[test]
    fn section_frame_style_parses_expected_names_unit() {
        assert_eq!(
            SectionFrameStyle::parse("top"),
            Some(SectionFrameStyle::Top)
        );
        assert_eq!(
            SectionFrameStyle::parse("top-bottom"),
            Some(SectionFrameStyle::TopBottom)
        );
        assert_eq!(
            SectionFrameStyle::parse("round"),
            Some(SectionFrameStyle::Round)
        );
        assert_eq!(
            SectionFrameStyle::parse("square"),
            Some(SectionFrameStyle::Square)
        );
        assert_eq!(
            SectionFrameStyle::parse("none"),
            Some(SectionFrameStyle::None)
        );
    }

    #[test]
    fn ruled_section_policy_parses_expected_names_unit() {
        assert_eq!(
            super::RuledSectionPolicy::parse("per-section"),
            Some(super::RuledSectionPolicy::PerSection)
        );
        assert_eq!(
            super::RuledSectionPolicy::parse("stacked"),
            Some(super::RuledSectionPolicy::Shared)
        );
        assert_eq!(
            super::RuledSectionPolicy::parse("list"),
            Some(super::RuledSectionPolicy::Shared)
        );
        assert_eq!(super::RuledSectionPolicy::parse("wat"), None);
    }

    #[test]
    fn top_bottom_section_frame_wraps_body_with_rules_unit() {
        let theme = crate::ui::theme::resolve_theme(crate::ui::theme::DEFAULT_THEME_NAME);
        let render = SectionRenderContext {
            color: false,
            theme: &theme,
            style_overrides: &crate::ui::style::StyleOverrides::default(),
        };
        let tokens = SectionStyleTokens {
            border: crate::ui::style::StyleToken::PanelBorder,
            title: crate::ui::style::StyleToken::PanelTitle,
        };
        let rendered = render_section_block_with_overrides(
            "Commands",
            "  show\n  delete",
            SectionFrameStyle::TopBottom,
            true,
            Some(18),
            render,
            tokens,
        );

        assert!(rendered.contains("Commands"));
        assert!(rendered.contains("show"));
        assert!(
            rendered
                .lines()
                .last()
                .is_some_and(|line| line.contains('─'))
        );
    }

    #[test]
    fn square_section_frame_boxes_body_unit() {
        let theme = crate::ui::theme::resolve_theme(crate::ui::theme::DEFAULT_THEME_NAME);
        let render = SectionRenderContext {
            color: false,
            theme: &theme,
            style_overrides: &crate::ui::style::StyleOverrides::default(),
        };
        let tokens = SectionStyleTokens {
            border: crate::ui::style::StyleToken::PanelBorder,
            title: crate::ui::style::StyleToken::PanelTitle,
        };
        let rendered = render_section_block_with_overrides(
            "Usage",
            "osp config show",
            SectionFrameStyle::Square,
            true,
            None,
            render,
            tokens,
        );

        assert!(rendered.contains("┌"));
        assert!(rendered.contains("│ osp config show"));
        assert!(rendered.contains("┘"));
    }

    #[test]
    fn section_chrome_measures_wide_titles_in_terminal_columns_unit() {
        let theme = crate::ui::theme::resolve_theme(crate::ui::theme::DEFAULT_THEME_NAME);
        let render = SectionRenderContext {
            color: false,
            theme: &theme,
            style_overrides: &crate::ui::style::StyleOverrides::default(),
        };
        let tokens = SectionStyleTokens::same(crate::ui::style::StyleToken::PanelBorder);

        let divider = render_section_divider(
            "界界界界界",
            &crate::ui::section_chrome::SectionDividerRequest {
                unicode: true,
                width: Some(16),
                title_columns: 2,
                render,
                tokens,
            },
        );
        assert_eq!(UnicodeWidthStr::width(divider.as_str()), 16);

        let boxed = render_section_block_with_overrides(
            "界界界界界",
            "x",
            SectionFrameStyle::Round,
            true,
            None,
            render,
            tokens,
        );
        let widths = boxed
            .lines()
            .map(UnicodeWidthStr::width)
            .collect::<Vec<_>>();
        assert!(widths.windows(2).all(|pair| pair[0] == pair[1]), "{boxed}");
    }

    #[test]
    fn section_frame_styles_cover_none_bottom_and_round_unit() {
        let theme = crate::ui::theme::resolve_theme(crate::ui::theme::DEFAULT_THEME_NAME);
        let render = SectionRenderContext {
            color: false,
            theme: &theme,
            style_overrides: &crate::ui::style::StyleOverrides::default(),
        };
        let tokens = SectionStyleTokens {
            border: crate::ui::style::StyleToken::PanelBorder,
            title: crate::ui::style::StyleToken::PanelTitle,
        };
        let plain = render_section_block_with_overrides(
            "Note",
            "body",
            SectionFrameStyle::None,
            false,
            Some(16),
            render,
            tokens,
        );
        let bottom = render_section_block_with_overrides(
            "Note",
            "body",
            SectionFrameStyle::Bottom,
            false,
            Some(16),
            render,
            tokens,
        );
        let round = render_section_block_with_overrides(
            "Note",
            "body",
            SectionFrameStyle::Round,
            true,
            Some(16),
            render,
            tokens,
        );

        assert!(plain.contains("Note:"));
        assert!(bottom.lines().last().is_some_and(|line| line.contains('-')));
        assert!(round.contains("╭"));
        assert!(round.contains("╰"));
    }
}
