//! Buffered user-facing messages and their canonical rendering helpers.

mod render;
use crate::config::ResolvedConfig;
use crate::ui::settings::{RenderProfile, RenderSettings, resolve_settings};
use crate::ui::style::StyleToken;

pub(crate) use crate::core::MessageLevel;

impl MessageLevel {
    pub fn title(self) -> &'static str {
        match self {
            MessageLevel::Error => "Errors",
            MessageLevel::Warning => "Warnings",
            MessageLevel::Success => "Success",
            MessageLevel::Info => "Info",
            MessageLevel::Trace => "Trace",
            MessageLevel::Evidence => "Evidence",
        }
    }

    pub(crate) fn as_rank(self) -> i8 {
        match self {
            MessageLevel::Error => 0,
            MessageLevel::Warning => 1,
            MessageLevel::Success => 2,
            MessageLevel::Info => 3,
            MessageLevel::Trace => 4,
            MessageLevel::Evidence => 5,
        }
    }

    pub(crate) fn from_rank(rank: i8) -> Self {
        match rank {
            i8::MIN..=0 => MessageLevel::Error,
            1 => MessageLevel::Warning,
            2 => MessageLevel::Success,
            3 => MessageLevel::Info,
            4 => MessageLevel::Trace,
            _ => MessageLevel::Evidence,
        }
    }

    pub(crate) fn ordered() -> impl Iterator<Item = Self> {
        Self::ALL.into_iter()
    }

    pub(crate) fn style_token(self) -> StyleToken {
        match self {
            MessageLevel::Error => StyleToken::Error,
            MessageLevel::Warning => StyleToken::Warning,
            MessageLevel::Success => StyleToken::Success,
            MessageLevel::Info => StyleToken::Info,
            MessageLevel::Trace | MessageLevel::Evidence => StyleToken::Trace,
        }
    }
}

/// Layout style used when rendering buffered messages.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MessageLayout {
    Minimal,
    Plain,
    Compact,
    Grouped,
}

impl MessageLayout {
    pub fn parse(value: &str) -> Option<Self> {
        match value.trim().to_ascii_lowercase().as_str() {
            "minimal" | "austere" => Some(Self::Minimal),
            "plain" | "none" => Some(Self::Plain),
            "compact" => Some(Self::Compact),
            "grouped" | "full" => Some(Self::Grouped),
            _ => None,
        }
    }
}

/// A single UI message with its associated severity.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UiMessage {
    pub level: MessageLevel,
    pub text: String,
    /// Human-facing context; machine-readable message text stays unchanged.
    pub title: Option<String>,
}

/// In-memory buffer for messages collected during command execution.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct MessageBuffer {
    entries: Vec<UiMessage>,
}

/// Resolved presentation for rendering buffered messages.
#[derive(Debug, Clone)]
pub struct MessageRequest {
    pub max_level: MessageLevel,
    pub layout: MessageLayout,
    pub settings: crate::ui::ResolvedRenderSettings,
}

impl MessageRequest {
    /// Resolves ordinary message chrome from render settings.
    pub fn new(settings: &RenderSettings, max_level: MessageLevel) -> Self {
        Self {
            max_level,
            layout: MessageLayout::Grouped,
            settings: resolve_settings(settings, RenderProfile::Normal),
        }
    }
}

/// Renders message events in their requested layout and chrome.
pub fn render(messages: &MessageBuffer, request: &MessageRequest) -> String {
    render::render_messages(messages, request)
}

impl MessageBuffer {
    #[cfg(test)]
    pub fn new() -> Self {
        Self::default()
    }

    pub fn push<T: Into<String>>(&mut self, level: MessageLevel, text: T) {
        self.push_message(UiMessage::new(level, text));
    }

    /// Give an event its own heading instead of merging it into a severity group.
    pub fn push_titled(
        &mut self,
        level: MessageLevel,
        title: impl Into<String>,
        text: impl Into<String>,
    ) {
        let mut message = UiMessage::new(level, text);
        message.title = Some(title.into());
        self.push_message(message);
    }

    pub(crate) fn push_message(&mut self, message: UiMessage) {
        self.entries.push(message);
    }

    pub fn error<T: Into<String>>(&mut self, text: T) {
        self.push(MessageLevel::Error, text);
    }

    pub fn warning<T: Into<String>>(&mut self, text: T) {
        self.push(MessageLevel::Warning, text);
    }

    pub fn success<T: Into<String>>(&mut self, text: T) {
        self.push(MessageLevel::Success, text);
    }

    pub fn info<T: Into<String>>(&mut self, text: T) {
        self.push(MessageLevel::Info, text);
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    pub fn entries(&self) -> &[UiMessage] {
        &self.entries
    }

    pub(crate) fn entries_for_level(
        &self,
        level: MessageLevel,
    ) -> impl Iterator<Item = &UiMessage> {
        self.entries
            .iter()
            .filter(move |entry| entry.level == level)
    }
}

pub(crate) fn message_layout_from_config(config: &ResolvedConfig) -> MessageLayout {
    config
        .get_string("ui.messages.layout")
        .and_then(MessageLayout::parse)
        .unwrap_or(MessageLayout::Grouped)
}

pub fn adjust_verbosity(base: MessageLevel, verbose: u8, quiet: u8) -> MessageLevel {
    let rank =
        (i16::from(base.as_rank()) + i16::from(verbose) - i16::from(quiet)).clamp(0, 5) as i8;
    MessageLevel::from_rank(rank)
}

impl UiMessage {
    pub(crate) fn new(level: MessageLevel, text: impl Into<String>) -> Self {
        Self {
            level,
            text: text.into(),
            title: None,
        }
    }
}
