//! Capture terminal commands with an owned reader and a scenario deadline.
#![cfg(unix)]
use portable_pty::{CommandBuilder, ExitStatus, PtyPair};
use std::io::{Read, Write};
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
    capture_dialogue(pair, command, timeout, &[])
}

pub(crate) fn capture_dialogue(
    pair: PtyPair,
    command: CommandBuilder,
    timeout: Duration,
    answers: &[(&str, &str, bool)],
) -> (String, ExitStatus) {
    let fd = pair.master.as_raw_fd();
    let mut writer = pair.master.take_writer().expect("terminal writer");
    let mut answered = 0;
    let mut observed = 0;
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
        if let Some((prompt, answer, hidden)) = answers.get(answered) {
            let snapshot = bytes.lock().unwrap();
            let seen = String::from_utf8_lossy(&snapshot[observed..]).contains(prompt);
            let next_observed = snapshot.len();
            drop(snapshot);
            let secret_ready = !hidden
                || fd.is_some_and(|fd| {
                    let mut attrs = std::mem::MaybeUninit::<libc::termios>::uninit();
                    // SAFETY: tcgetattr initializes attrs on success for this owned PTY.
                    unsafe {
                        libc::tcgetattr(fd, attrs.as_mut_ptr()) == 0
                            && attrs.assume_init().c_lflag & libc::ECHO == 0
                    }
                });
            if seen && secret_ready {
                writer
                    .write_all(answer.as_bytes())
                    .expect("dialogue answer");
                writer.flush().unwrap();
                answered += 1;
                observed = next_observed;
            }
        }
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
    assert_eq!(answered, answers.len(), "unanswered terminal dialogue");
    let output = String::from_utf8(bytes.lock().unwrap().clone()).expect("terminal UTF-8");
    (output, status)
}
