//! Bounded, lifetime-owned delivery above the live editor.
//!
//! Workers own a session handle, never a terminal writer. Only the editor may
//! deliver background lines; foreground commands retain them until input resumes.

use std::sync::mpsc::SyncSender;
use std::sync::{Arc, Condvar, Mutex};
use std::time::Duration;

use crate::app::{StdIoUiSink, UiSink};
use reedline::ExternalPrinter;

#[derive(Default)]
struct Notices {
    printer: Option<SyncSender<String>>,
    pending: Vec<String>,
    basic: bool,
    overflow: usize,
    owner: Option<ReplSession>,
}

impl Notices {
    fn post(&mut self, line: String) {
        let line = inline(&line);
        if self.basic {
            StdIoUiSink.write_stderr(&format!("\n{line}\n"));
        } else if self.overflow > 0 {
            self.overflow = self.overflow.saturating_add(1);
            *self.pending.last_mut().unwrap() = format!("+{} more notices", self.overflow);
        } else if self.pending.len() < 8 {
            self.pending.push(line);
        } else {
            self.overflow = 2;
            self.pending[7] = format!("+{} more notices", self.overflow);
        }
    }

    fn flush(&mut self, prompt_rows: usize) {
        if let Some(printer) = &self.printer
            && !self.pending.is_empty()
        {
            // Reedline prints each message followed by CRLF in raw mode.
            // Clear the multiline prompt header only before the first notice.
            let mut prefix = String::new();
            if prompt_rows > 0 {
                prefix = format!("\x1b[{prompt_rows}A");
                for row in 0..prompt_rows {
                    prefix.push_str("\r\x1b[2K");
                    if row + 1 < prompt_rows {
                        prefix.push_str("\x1b[1B");
                    }
                }
                if prompt_rows > 1 {
                    prefix.push_str(&format!("\x1b[{}A", prompt_rows - 1));
                }
            }
            while let Some(line) = self.pending.first() {
                if printer.try_send(format!("{prefix}{line}")).is_err() {
                    return;
                }
                self.pending.remove(0);
                prefix.clear();
            }
            self.overflow = 0;
        }
    }

    fn attach(&mut self, printer: &ExternalPrinter<String>) {
        self.basic = false;
        self.printer = Some(printer.sender());
    }
}

static NOTICES: Mutex<Notices> = Mutex::new(Notices {
    printer: None,
    pending: Vec::new(),
    basic: false,
    overflow: 0,
    owner: None,
});

fn inline(line: &str) -> String {
    crate::ui::sanitize_human_text(line)
        .replace('\n', " ")
        .chars()
        .take(512)
        .collect()
}

/// Cancellable lifetime and bounded notice sink for one rich editor run.
///
/// Host rebuild, fallback and exit cancel this handle. Old workers cannot
/// publish into a later editor. Status caches may update during commands;
/// terminal delivery waits until the editor owns input again.
#[derive(Clone)]
pub struct ReplSession(Arc<Lifetime>);

struct Lifetime {
    cancelled: Mutex<bool>,
    wake: Condvar,
}

impl ReplSession {
    fn is_current(&self, notices: &Notices) -> bool {
        notices
            .owner
            .as_ref()
            .is_some_and(|owner| Arc::ptr_eq(&owner.0, &self.0))
    }

    /// Whether this editor lifetime has ended.
    pub fn is_cancelled(&self) -> bool {
        *self.0.cancelled.lock().unwrap_or_else(|e| e.into_inner())
    }

    /// Waits up to `duration`, returning true when the host cancels this run.
    pub fn wait(&self, duration: Duration) -> bool {
        let cancelled = self.0.cancelled.lock().unwrap_or_else(|e| e.into_inner());
        let (cancelled, _) = self
            .0
            .wake
            .wait_timeout_while(cancelled, duration, |cancelled| !*cancelled)
            .unwrap_or_else(|e| e.into_inner());
        *cancelled
    }

    /// Posts a sanitized line of at most 512 characters without blocking.
    ///
    /// Cancelled and superseded sessions cannot publish. Overflow is summarized;
    /// a full printer retains pending lines until the next input read.
    pub fn post_notice(&self, line: impl AsRef<str>) {
        let mut notices = NOTICES.lock().unwrap_or_else(|e| e.into_inner());
        if !self.is_cancelled() && self.is_current(&notices) {
            notices.post(line.as_ref().to_string());
        }
    }

    /// Requests best-effort audio for the current rich session.
    pub fn request_bell(&self, config: &crate::config::ResolvedConfig) {
        let notices = NOTICES.lock().unwrap_or_else(|e| e.into_inner());
        if !self.is_cancelled() && self.is_current(&notices) && sound_enabled(config) {
            crate::repl::bell::request();
            crate::repl::bell::finish_background();
        }
    }
}

pub(crate) struct SessionOwner(pub(crate) ReplSession);

impl SessionOwner {
    pub(crate) fn new() -> Self {
        let session = ReplSession(Arc::new(Lifetime {
            cancelled: Mutex::new(false),
            wake: Condvar::new(),
        }));
        crate::repl::bell::discard();
        let mut notices = NOTICES.lock().unwrap_or_else(|e| e.into_inner());
        if let Some(old) = notices.owner.replace(session.clone()) {
            cancel(&old);
        }
        Self(session)
    }
}

fn cancel(session: &ReplSession) {
    *session
        .0
        .cancelled
        .lock()
        .unwrap_or_else(|e| e.into_inner()) = true;
    session.0.wake.notify_all();
}

impl Drop for SessionOwner {
    fn drop(&mut self) {
        cancel(&self.0);
        crate::repl::bell::discard();
        let mut notices = NOTICES.lock().unwrap_or_else(|e| e.into_inner());
        if self.0.is_current(&notices) {
            notices.owner = None;
            notices.printer = None;
            notices.pending.clear();
            notices.overflow = 0;
        }
    }
}

/// Prints a bounded startup line; recurring work should use [`ReplSession`].
pub fn post_notice(line: impl Into<String>) {
    NOTICES
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .post(line.into());
}

pub(crate) fn attach(printer: &ExternalPrinter<String>) {
    NOTICES
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .attach(printer);
}

pub(crate) fn flush(prompt_left: &str) {
    let columns = crossterm::terminal::size()
        .map(|(columns, _)| usize::from(columns).max(1))
        .unwrap_or(80);
    let prompt_rows = prompt_left.rsplit_once('\n').map_or(0, |(header, _)| {
        header
            .split('\n')
            .map(|line| crate::ui::display_width(line).div_ceil(columns).max(1))
            .sum()
    });
    NOTICES
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .flush(prompt_rows);
}

pub(crate) fn attach_basic() {
    crate::repl::bell::discard();
    let mut notices = NOTICES.lock().unwrap_or_else(|e| e.into_inner());
    if let Some(owner) = notices.owner.take() {
        cancel(&owner);
    }
    notices.printer = None;
    for line in notices.pending.drain(..) {
        StdIoUiSink.write_stderr(&format!("{line}\n"));
    }
    notices.overflow = 0;
    notices.basic = true;
}

fn sound_enabled(config: &crate::config::ResolvedConfig) -> bool {
    use std::io::IsTerminal;
    config
        .get_string("repl.notifications.sound")
        .is_some_and(|sound| sound.eq_ignore_ascii_case("bell"))
        && config.get_string("ui.format") != Some("json")
        && std::io::stdin().is_terminal()
        && std::io::stdout().is_terminal()
        && std::io::stderr().is_terminal()
}

pub(crate) fn request_foreground_bell(
    config: &crate::config::ResolvedConfig,
    format: crate::core::output::OutputFormat,
) {
    let notices = NOTICES.lock().unwrap_or_else(|e| e.into_inner());
    if notices.owner.is_some()
        && !notices.basic
        && format != crate::core::output::OutputFormat::Json
        && sound_enabled(config)
    {
        crate::repl::bell::request();
    }
}

#[cfg(test)]
#[cfg_attr(coverage_nightly, coverage(off))]
mod tests {
    use super::{ExternalPrinter, Notices};

    #[test]
    fn notices_posted_before_the_editor_print_once_it_starts_unit() {
        let mut notices = Notices::default();
        notices.post("early".into());
        let printer = ExternalPrinter::<String>::default();
        notices.attach(&printer);
        notices.post("late".into());
        assert_eq!(printer.get_line(), None);
        notices.flush(1);
        assert_eq!(printer.get_line().as_deref(), Some("\x1b[1A\r\x1b[2Kearly"));
        assert_eq!(printer.get_line().as_deref(), Some("late"));
        assert_eq!(printer.get_line(), None);
        for number in 0..11 {
            notices.post(format!("notice {number}"));
        }
        notices.flush(2);
        assert_eq!(
            printer.get_line().as_deref(),
            Some("\x1b[2A\r\x1b[2K\x1b[1B\r\x1b[2K\x1b[1Anotice 0")
        );
        for number in 1..7 {
            assert_eq!(printer.get_line(), Some(format!("notice {number}")));
        }
        assert_eq!(printer.get_line().as_deref(), Some("+4 more notices"));
        assert_eq!(printer.get_line(), None);
    }
}
