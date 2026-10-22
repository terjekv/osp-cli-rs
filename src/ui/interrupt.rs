//! Cooperative Ctrl-C boundary for terminal commands and blocking service reads.
//!
//! A detached read may still finish remotely. Callers own submission certainty
//! and must keep session publication and terminal effects outside the worker.

use std::io;
use std::sync::{
    Arc, OnceLock,
    atomic::{AtomicBool, Ordering},
    mpsc,
};
use std::time::Duration;

static INTERRUPTED: OnceLock<Result<Arc<AtomicBool>, String>> = OnceLock::new();

/// Install terminal interruption handling and start a fresh command boundary.
/// Call only for interactive input; redirected commands retain normal signals.
pub fn prepare() -> io::Result<()> {
    let flag = INTERRUPTED
        .get_or_init(|| {
            let flag = Arc::new(AtomicBool::new(false));
            signal_hook::flag::register(signal_hook::consts::SIGINT, Arc::clone(&flag))
                .map(|_| flag)
                .map_err(|error| error.to_string())
        })
        .as_ref()
        .map_err(|message| io::Error::other(message.clone()))?;
    flag.store(false, Ordering::SeqCst);
    Ok(())
}

pub(crate) fn interrupted() -> bool {
    INTERRUPTED
        .get()
        .and_then(|flag| flag.as_ref().ok())
        .is_some_and(|flag| flag.load(Ordering::SeqCst))
}

/// Wait for a blocking read, or abandon its result on Ctrl-C.
///
/// Without an interactive boundary this executes inline. With a boundary the
/// worker owns all inputs and may outlive this call after interruption. It must
/// only perform transport work, never mutate caller state or read the terminal.
/// Returns `Interrupted` on cancellation; it does not establish remote outcome.
pub fn run<T: Send + 'static>(work: impl FnOnce() -> T + Send + 'static) -> io::Result<T> {
    if INTERRUPTED.get().is_none() {
        return Ok(work());
    }
    if interrupted() {
        return Err(io::Error::new(
            io::ErrorKind::Interrupted,
            "command interrupted",
        ));
    }
    let (send, receive) = mpsc::sync_channel(1);
    std::thread::Builder::new()
        .name("service-read".into())
        .spawn(move || {
            let _ = send.send(work());
        })?;
    loop {
        if interrupted() {
            return Err(io::Error::new(
                io::ErrorKind::Interrupted,
                "command interrupted",
            ));
        }
        match receive.recv_timeout(Duration::from_millis(50)) {
            Ok(value) => return Ok(value),
            Err(mpsc::RecvTimeoutError::Timeout) => {}
            Err(mpsc::RecvTimeoutError::Disconnected) => {
                return Err(io::Error::other("service read worker stopped"));
            }
        }
    }
}
