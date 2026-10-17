//! Capture terminal commands with an owned reader and a scenario deadline.
#![cfg(unix)]
use portable_pty::{CommandBuilder, ExitStatus, PtyPair};
use std::io::Read;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

struct Capture {
    child: Box<dyn portable_pty::Child + Send>,
    reader: Option<std::thread::JoinHandle<()>>,
}

impl Drop for Capture {
    fn drop(&mut self) {
        if let Some(pid) = self.child.process_id() {
            unsafe {
                libc::kill(-(pid as i32), libc::SIGKILL);
            }
        }
        let _ = self.child.kill();
        let _ = self.child.wait();
        if let Some(reader) = self.reader.take() {
            let _ = reader.join();
        }
    }
}

pub(crate) fn capture(
    pair: PtyPair,
    command: CommandBuilder,
    timeout: Duration,
) -> (String, ExitStatus) {
    let child = pair
        .slave
        .spawn_command(command)
        .expect("spawn terminal command");
    let mut capture = Capture {
        child,
        reader: None,
    };
    drop(pair.slave);
    let mut reader = pair.master.try_clone_reader().expect("terminal reader");
    let bytes = Arc::new(Mutex::new(Vec::new()));
    let captured = bytes.clone();
    capture.reader = Some(std::thread::spawn(move || {
        let mut chunk = [0; 4096];
        while let Ok(size) = reader.read(&mut chunk) {
            if size == 0 {
                break;
            }
            captured.lock().unwrap().extend_from_slice(&chunk[..size]);
        }
    }));
    let deadline = Instant::now() + timeout;
    let status = loop {
        if let Some(status) = capture.child.try_wait().expect("terminal status") {
            break status;
        }
        assert!(
            Instant::now() < deadline,
            "terminal command deadline exceeded; output:\n{}",
            String::from_utf8_lossy(&bytes.lock().unwrap())
        );
        std::thread::sleep(Duration::from_millis(10));
    };
    drop(capture);
    let output = String::from_utf8(bytes.lock().unwrap().clone()).expect("terminal UTF-8");
    (output, status)
}
