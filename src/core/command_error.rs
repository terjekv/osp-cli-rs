//! Structured recovery advice shared by native commands and host renderers.

/// A command failure with optional recovery advice and a service request ID.
/// Display contains only the message; renderers retain advice as data.
#[derive(Debug, Clone)]
pub struct CommandError {
    /// Human-readable failure summary.
    pub message: String,
    /// Suggested recovery action, without a `hint:` prefix.
    pub action: Option<String>,
    /// Service request identifier for support.
    pub request_id: Option<String>,
}

impl CommandError {
    /// Creates a failure message without recovery advice or a request identifier.
    pub fn new(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
            action: None,
            request_id: None,
        }
    }

    /// Attaches recovery advice without a `hint:` prefix.
    pub fn with_action(mut self, action: impl Into<String>) -> Self {
        self.action = Some(action.into());
        self
    }

    /// Renders the message and recovery fields for embedding in text documents.
    /// Native command failures should retain this error as structured data.
    pub fn render(&self) -> String {
        let mut text = self.message.clone();
        if let Some(id) = &self.request_id {
            text.push_str(&format!("\nrequest-id: {id}"));
        }
        if let Some(action) = &self.action {
            text.push_str(&format!("\nhint: {action}"));
        }
        text
    }
}

impl std::fmt::Display for CommandError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.message)
    }
}

impl std::error::Error for CommandError {}

impl miette::Diagnostic for CommandError {
    fn help<'a>(&'a self) -> Option<Box<dyn std::fmt::Display + 'a>> {
        self.action
            .as_ref()
            .map(|action| Box::new(action) as Box<dyn std::fmt::Display>)
    }
}
