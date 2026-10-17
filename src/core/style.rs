//! Shared colour/style syntax for configuration, themes and terminal output.

use nu_ansi_term::{Color, Style};

/// Palette roles that can be overridden independently of theme selection.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ColorRole {
    Text,
    Muted,
    Accent,
    Info,
    Warning,
    Success,
    Error,
    Border,
    Title,
}

impl ColorRole {
    pub(crate) const ALL: [Self; 9] = [
        Self::Text,
        Self::Muted,
        Self::Accent,
        Self::Info,
        Self::Warning,
        Self::Success,
        Self::Error,
        Self::Border,
        Self::Title,
    ];

    pub(crate) fn key(self) -> &'static str {
        match self {
            Self::Text => "color.text",
            Self::Muted => "color.muted",
            Self::Accent => "color.accent",
            Self::Info => "color.info",
            Self::Warning => "color.warning",
            Self::Success => "color.success",
            Self::Error => "color.error",
            Self::Border => "color.border",
            Self::Title => "color.title",
        }
    }

    pub(crate) fn name(self) -> &'static str {
        self.key().trim_start_matches("color.")
    }
}

/// Validates that a style specification uses syntax the renderer understands.
pub fn is_valid_style_spec(value: &str) -> bool {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        return true;
    }

    trimmed.split_whitespace().all(|raw| {
        let token = raw.trim().to_ascii_lowercase();
        !token.is_empty() && (is_style_modifier(&token) || parse_color_token(&token).is_some())
    })
}

pub(crate) fn parse_style_spec(spec: &str) -> Option<Style> {
    let mut style = Style::new();
    let mut changed = false;

    for raw in spec.split_whitespace() {
        let token = raw.trim().to_ascii_lowercase();
        if token.is_empty() {
            continue;
        }
        if let Some(updated) = apply_style_token(style, &token) {
            style = updated;
            changed = true;
        }
    }

    changed.then_some(style)
}

fn is_style_modifier(token: &str) -> bool {
    matches!(token, "bold" | "dim" | "dimmed" | "italic" | "underline")
}

fn apply_style_token(style: Style, token: &str) -> Option<Style> {
    match token {
        "bold" => Some(style.bold()),
        "dim" | "dimmed" => Some(style.dimmed()),
        "italic" => Some(style.italic()),
        "underline" => Some(style.underline()),
        _ => parse_color_token(token).map(|color| style.fg(color)),
    }
}

pub(crate) fn parse_color_token(token: &str) -> Option<Color> {
    if let Some(value) = token.strip_prefix("ansi")
        && let Ok(index) = value.parse::<u8>()
    {
        return Some(Color::Fixed(index));
    }

    if let Some(value) = token
        .strip_prefix("rgb(")
        .and_then(|value| value.strip_suffix(')'))
    {
        let mut parts = value.split(',').map(|part| part.trim().parse::<u8>().ok());
        if let (Some(Some(r)), Some(Some(g)), Some(Some(b))) =
            (parts.next(), parts.next(), parts.next())
        {
            return Some(Color::Rgb(r, g, b));
        }
    }

    match token {
        "black" => Some(Color::Black),
        "red" => Some(Color::Red),
        "green" => Some(Color::Green),
        "yellow" => Some(Color::Yellow),
        "blue" => Some(Color::Blue),
        "purple" | "magenta" => Some(Color::Purple),
        "cyan" => Some(Color::Cyan),
        "white" => Some(Color::White),
        "bright-black" | "darkgray" | "dark_gray" | "gray" | "grey" => Some(Color::DarkGray),
        "bright-red" | "lightred" | "light_red" => Some(Color::LightRed),
        "bright-green" | "lightgreen" | "light_green" => Some(Color::LightGreen),
        "bright-yellow" | "lightyellow" | "light_yellow" => Some(Color::LightYellow),
        "bright-blue" | "lightblue" | "light_blue" => Some(Color::LightBlue),
        "bright-purple" | "bright-magenta" | "lightmagenta" | "light_magenta" | "lightpurple"
        | "light_purple" => Some(Color::LightPurple),
        "bright-cyan" | "lightcyan" | "light_cyan" => Some(Color::LightCyan),
        "bright-white" | "lightgray" | "light_gray" | "lightgrey" | "light_grey" => {
            Some(Color::LightGray)
        }
        _ => parse_hex_rgb(token).map(|(r, g, b)| Color::Rgb(r, g, b)),
    }
}

fn parse_hex_rgb(value: &str) -> Option<(u8, u8, u8)> {
    match value.as_bytes() {
        [b'#', r, g, b] => Some((
            expand_hex_nibble(*r)?,
            expand_hex_nibble(*g)?,
            expand_hex_nibble(*b)?,
        )),
        [b'#', r1, r2, g1, g2, b1, b2] => Some((
            parse_hex_pair(*r1, *r2)?,
            parse_hex_pair(*g1, *g2)?,
            parse_hex_pair(*b1, *b2)?,
        )),
        _ => None,
    }
}

fn expand_hex_nibble(value: u8) -> Option<u8> {
    let nibble = parse_hex_digit(value)?;
    Some((nibble << 4) | nibble)
}

fn parse_hex_pair(high: u8, low: u8) -> Option<u8> {
    Some((parse_hex_digit(high)? << 4) | parse_hex_digit(low)?)
}

fn parse_hex_digit(value: u8) -> Option<u8> {
    match value {
        b'0'..=b'9' => Some(value - b'0'),
        b'a'..=b'f' => Some(value - b'a' + 10),
        b'A'..=b'F' => Some(value - b'A' + 10),
        _ => None,
    }
}
