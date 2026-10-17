//! Session recording consumes rendered text and the existing allowlisted HTTP
//! evidence. Prompt input, request headers and payloads never enter this module.

use std::cell::RefCell;
use std::fs::{File, OpenOptions};
use std::io::Write;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use miette::{IntoDiagnostic, Result, miette};
use serde_json::{Value, json};

use crate::app::sink::UiSink;
use crate::config::{ResolvedConfig, is_sensitive_key};

#[derive(Clone, Default)]
pub(crate) struct Recording(Arc<Mutex<State>>);

#[derive(Default)]
struct State {
    file: Option<File>,
    path: Option<PathBuf>,
    error: Option<String>,
}

#[derive(Debug, Clone)]
pub(crate) struct Capture(Arc<Mutex<Captured>>);

#[derive(Debug)]
struct Captured {
    events: Vec<Value>,
    secrets: Vec<String>,
}

thread_local! {
    // The outer submitted line owns nested source/pagination execution too.
    static ACTIVE: RefCell<Option<Option<Capture>>> = const { RefCell::new(None) };
}

pub(crate) struct Scope;

impl Drop for Scope {
    fn drop(&mut self) {
        ACTIVE.with(|active| active.replace(None));
    }
}

pub(crate) fn current() -> Option<Capture> {
    ACTIVE.with(|active| active.borrow().clone().flatten())
}

impl Recording {
    pub(crate) fn start(&self, path: PathBuf) -> Result<String> {
        let mut state = self
            .0
            .lock()
            .map_err(|_| miette!("recording lock poisoned"))?;
        if state.file.is_some() {
            return Err(miette!(
                "Recording is already active. Use recording stop first."
            ));
        }
        let mut options = OpenOptions::new();
        options.write(true).create_new(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.mode(0o600);
        }
        let file = options.open(&path).into_diagnostic()?;
        let message = format!("Recording to {}.\n", path.display());
        state.file = Some(file);
        state.path = Some(path);
        state.error = None;
        Ok(message)
    }

    pub(crate) fn stop(&self) -> Result<String> {
        let mut state = self
            .0
            .lock()
            .map_err(|_| miette!("recording lock poisoned"))?;
        let Some(mut file) = state.file.take() else {
            return Ok("Recording is not active.\n".into());
        };
        state.path = None;
        file.flush().into_diagnostic()?;
        Ok("Recording stopped.\n".into())
    }

    pub(crate) fn status(&self) -> Result<String> {
        let state = self
            .0
            .lock()
            .map_err(|_| miette!("recording lock poisoned"))?;
        Ok(state.path.as_ref().map_or_else(
            || "Recording is not active.\n".into(),
            |path| format!("Recording to {}.\n", path.display()),
        ))
    }

    pub(crate) fn enter(
        &self,
        line: &str,
        config: &ResolvedConfig,
    ) -> Option<(Scope, Option<Capture>)> {
        if ACTIVE.with(|active| active.borrow().is_some()) {
            return None;
        }
        let capture = self
            .0
            .lock()
            .ok()
            .filter(|state| state.file.is_some())
            .and_then(|_| {
                let tokens = shell_words::split(line).ok()?;
                if line.starts_with(' ')
                    || line.trim().is_empty()
                    || super::history_store::is_sensitive_line(line)
                    || tokens.iter().any(|token| token == "--show-secrets")
                    || tokens.first().is_some_and(|token| token == "recording")
                {
                    return None;
                }
                let secrets: Vec<String> = config
                    .values()
                    .iter()
                    .filter(|(key, entry)| is_sensitive_key(key) || entry.value.is_secret())
                    .filter_map(|(_, entry)| match entry.value.reveal() {
                        crate::config::ConfigValue::String(value) if !value.is_empty() => {
                            Some(value.clone())
                        }
                        _ => None,
                    })
                    .collect();
                let mut line = line.to_owned();
                for secret in &secrets {
                    line = line.replace(secret, "[redacted]");
                }
                Some(Capture(Arc::new(Mutex::new(Captured {
                    events: vec![json!({"event": "command", "line": line})],
                    secrets,
                }))))
            });
        ACTIVE.with(|active| active.replace(Some(capture.clone())));
        Some((Scope, capture))
    }

    pub(crate) fn finish(&self, capture: Option<Capture>) -> Option<String> {
        let mut state = self.0.lock().ok()?;
        if let Some(capture) = capture {
            let captured = capture.0.lock().ok()?;
            if let Some(file) = &mut state.file {
                let result = captured.events.iter().try_for_each(|event| {
                    let mut event = event.clone();
                    if event["event"] == "output" {
                        replace_secrets(&mut event, &captured.secrets);
                    }
                    serde_json::to_writer(&mut *file, &event)?;
                    file.write_all(b"\n")?;
                    Ok::<_, std::io::Error>(())
                });
                if let Err(error) = result {
                    state.file = None;
                    state.path = None;
                    state.error = Some(format!("Recording stopped after a write failure: {error}"));
                }
            }
        }
        state.error.take()
    }
}

impl Capture {
    pub(crate) fn observe(&self, value: &Value) {
        fn collect(value: &Value, sensitive: bool, secrets: &mut Vec<String>) {
            match value {
                Value::Object(fields) => {
                    for (key, value) in fields {
                        collect(value, sensitive || is_sensitive_key(key), secrets);
                    }
                }
                Value::Array(values) => {
                    for value in values {
                        collect(value, sensitive, secrets);
                    }
                }
                Value::String(value) if sensitive && !value.is_empty() => {
                    secrets.push(value.clone())
                }
                Value::Number(_) | Value::Bool(_) if sensitive => {
                    secrets.push(value.to_string());
                }
                _ => {}
            }
        }
        if let Ok(mut captured) = self.0.lock() {
            collect(value, false, &mut captured.secrets);
        }
    }

    pub(crate) fn http(&self, method: &str, url: &str, status: Option<u16>, elapsed_ms: u128) {
        if let Ok(mut captured) = self.0.lock() {
            let mut url = crate::diagnostics::safe_url(url);
            for secret in &captured.secrets {
                url = url.replace(secret, "[redacted]");
            }
            captured.events.push(json!({
                "event": "http", "method": method,
                "url": url, "status": status,
                "duration_ms": elapsed_ms,
            }));
        }
    }

    pub(crate) fn output(&self, stream: &str, text: &str) {
        if text.is_empty() {
            return;
        }
        if let Ok(mut captured) = self.0.lock() {
            let mut text = crate::ui::sanitize_human_text(text);
            // JSON output can contain nested credentials even when the terminal
            // deliberately displays them. Reuse the config key classification.
            if let Ok(mut value) = serde_json::from_str::<Value>(&text) {
                redact_value(&mut value);
                text = format!(
                    "{}\n",
                    serde_json::to_string_pretty(&value).unwrap_or_default()
                );
            } else {
                text = text
                    .split_inclusive('\n')
                    .map(|line| {
                        let key = line
                            .trim_start()
                            .split([':', '=', '│', '\t'])
                            .next()
                            .unwrap_or("");
                        if is_sensitive_key(key) {
                            format!("{key}: [redacted]\n")
                        } else {
                            line.to_owned()
                        }
                    })
                    .collect();
            }
            for secret in &captured.secrets {
                text = text.replace(secret, "[redacted]");
            }
            captured
                .events
                .push(json!({"event": "output", "stream": stream, "text": text}));
        }
    }
}

fn replace_secrets(value: &mut Value, secrets: &[String]) {
    match value {
        Value::String(text) => {
            for secret in secrets {
                *text = text.replace(secret, "[redacted]");
            }
        }
        Value::Object(fields) => {
            for value in fields.values_mut() {
                replace_secrets(value, secrets);
            }
        }
        Value::Array(values) => {
            for value in values {
                replace_secrets(value, secrets);
            }
        }
        _ => {}
    }
}

fn redact_value(value: &mut Value) {
    match value {
        Value::Object(fields) => {
            for (key, value) in fields {
                if is_sensitive_key(key) {
                    *value = json!("[redacted]");
                } else {
                    redact_value(value);
                }
            }
        }
        Value::Array(values) => values.iter_mut().for_each(redact_value),
        _ => {}
    }
}

pub(crate) struct RecordingSink<'a> {
    pub(crate) inner: &'a mut dyn UiSink,
    pub(crate) capture: Option<Capture>,
}

impl UiSink for RecordingSink<'_> {
    fn write_stdout(&mut self, text: &str) {
        if let Some(capture) = &self.capture {
            capture.output("stdout", text);
        }
        self.inner.write_stdout(text);
    }
    fn write_stderr(&mut self, text: &str) {
        if let Some(capture) = &self.capture {
            capture.output("stderr", text);
        }
        self.inner.write_stderr(text);
    }
    fn write_stdout_paged(&mut self, text: &str, pager: &str) {
        if let Some(capture) = &self.capture {
            capture.output("stdout", text);
        }
        self.inner.write_stdout_paged(text, pager);
    }
    fn stderr_is_terminal(&self) -> bool {
        self.inner.stderr_is_terminal()
    }
    fn stderr_width(&self) -> Option<usize> {
        self.inner.stderr_width()
    }
    fn stderr_height(&self) -> Option<usize> {
        self.inner.stderr_height()
    }
    fn write_progress(&mut self, text: &str, replace: bool) {
        if let Some(capture) = &self.capture {
            capture.output("stderr", text);
        }
        self.inner.write_progress(text, replace);
    }
    fn clear_progress(&mut self) {
        self.inner.clear_progress();
    }
    fn start_progress_spinner(
        &mut self,
        text: &str,
        unicode: bool,
    ) -> Option<crate::app::sink::StderrSpinner> {
        if let Some(capture) = &self.capture {
            capture.output("stderr", text);
        }
        self.inner.start_progress_spinner(text, unicode)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, clap::Parser)]
#[command(
    name = "recording",
    about = "Record commands, output and HTTP timing as private JSON Lines"
)]
pub(crate) struct RecordingCommand {
    #[command(subcommand)]
    pub(crate) command: RecordingAction,
}

#[derive(Debug, Clone, PartialEq, Eq, clap::Subcommand)]
pub(crate) enum RecordingAction {
    /// Create a new recording file (0600); existing files are refused.
    Start {
        #[arg(value_name = "FILE", value_hint = clap::ValueHint::FilePath)]
        file: PathBuf,
    },
    /// Stop recording.
    Stop,
    /// Show the current recording file.
    Status,
}

impl RecordingCommand {
    pub(crate) fn run(self, recording: &Recording) -> Result<String> {
        match self.command {
            RecordingAction::Start { file } => recording
                .start(crate::normalize::expand_home(&file.to_string_lossy()).unwrap_or(file)),
            RecordingAction::Stop => recording.stop(),
            RecordingAction::Status => recording.status(),
        }
    }
}
