//! Semantic terminal feedback, separate from result documents.

use std::io::{self, Write};
use std::sync::Mutex;
use std::time::{Duration, Instant};

#[derive(Default)]
struct Bell {
    pending: bool,
    last: Option<Instant>,
}
static BELL: Mutex<Bell> = Mutex::new(Bell {
    pending: false,
    last: None,
});

pub(crate) fn request() {
    BELL.lock().unwrap_or_else(|e| e.into_inner()).pending = true;
}

pub(crate) fn discard() {
    BELL.lock().unwrap_or_else(|e| e.into_inner()).pending = false;
}

pub(crate) fn finish() {
    let _ = io::stdout().flush();
    finish_background();
}

// Background feedback must not acquire the editor's stdout lock.
pub(crate) fn finish_background() {
    let mut bell = BELL.lock().unwrap_or_else(|e| e.into_inner());
    if !std::mem::take(&mut bell.pending) {
        return;
    }
    if bell
        .last
        .is_some_and(|last| last.elapsed() < Duration::from_secs(10))
    {
        return;
    }
    let mut terminal = io::stderr().lock();
    if terminal
        .write_all(b"\x07")
        .and_then(|()| terminal.flush())
        .is_ok()
    {
        bell.last = Some(Instant::now());
    }
}
