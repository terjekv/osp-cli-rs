//! Run process-global environment scenarios in a private test process.
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

struct Cleanup(Child);

impl Drop for Cleanup {
    fn drop(&mut self) {
        #[cfg(unix)]
        unsafe {
            // Include subprocesses that outlive a failed assertion.
            libc::kill(-(self.0.id() as i32), libc::SIGKILL);
        }
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

/// Return true in the parent after running this exact test in isolation.
pub(crate) fn isolated_test() -> bool {
    if cfg!(miri) {
        return false;
    }
    let thread = std::thread::current();
    let name = thread.name().expect("named test thread");
    if std::env::var("OSP_ISOLATED_TEST").as_deref() == Ok(name) {
        return false;
    }
    let home = tempfile::tempdir().expect("isolated home");
    let transcript = home.path().join("transcript");
    let output = std::fs::File::create(&transcript).expect("test transcript");
    let mut command = Command::new(std::env::current_exe().expect("test executable"));
    command
        .args(["--exact", name, "--nocapture", "--test-threads=1"])
        .env("OSP_ISOLATED_TEST", name)
        .env("HOME", home.path())
        .env("XDG_CONFIG_HOME", home.path().join("config"))
        .env("XDG_CACHE_HOME", home.path().join("cache"))
        .env("XDG_DATA_HOME", home.path().join("data"))
        .env("XDG_STATE_HOME", home.path().join("state"))
        .stdin(Stdio::null())
        .stdout(output.try_clone().expect("clone transcript"))
        .stderr(output);
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        command.process_group(0);
    }
    let mut child = Cleanup(command.spawn().expect("isolated test should start"));
    let deadline = Instant::now() + Duration::from_secs(180);
    let status = loop {
        if let Some(status) = child.0.try_wait().expect("isolated test status") {
            break Some(status);
        }
        if Instant::now() >= deadline {
            break None;
        }
        std::thread::sleep(Duration::from_millis(10));
    };
    drop(child);
    let transcript = std::fs::read_to_string(transcript).expect("read test transcript");
    assert!(
        status.is_some_and(|status| status.success()),
        "isolated test {name} failed ({status:?}):\n{transcript}"
    );
    assert!(
        transcript.contains("1 passed;"),
        "isolated test did not execute: {transcript}"
    );
    true
}
