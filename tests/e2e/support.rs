#[cfg(unix)]
pub(crate) use crate::output_support::parse_json_stdout;
#[cfg(unix)]
use crate::temp_support::{TestTempDir, make_temp_dir};
#[cfg(unix)]
use portable_pty::{CommandBuilder, PtySize, native_pty_system};
#[cfg(unix)]
use std::io::{Read, Write};
#[cfg(unix)]
use std::path::{Path, PathBuf};
#[cfg(unix)]
use std::sync::{Arc, Mutex};
#[cfg(unix)]
use std::time::{Duration, Instant};

#[cfg(unix)]
pub(crate) use crate::process_support::{osp_command, write_config, write_executable_script};

#[cfg(unix)]
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub(crate) enum ReplPtyColorMode {
    #[default]
    Plain,
    Always,
}

#[cfg(unix)]
#[derive(Clone, Debug)]
pub(crate) struct ReplPtyConfig<'a> {
    color_mode: ReplPtyColorMode,
    cursor_position_reports: bool,
    simple_prompt: bool,
    config: Option<&'a str>,
    plugins_dir: Option<&'a Path>,
    intro_override: Option<&'a str>,
}

#[cfg(unix)]
impl Default for ReplPtyConfig<'_> {
    fn default() -> Self {
        Self {
            color_mode: ReplPtyColorMode::Plain,
            cursor_position_reports: true,
            simple_prompt: true,
            config: None,
            plugins_dir: None,
            intro_override: Some("none"),
        }
    }
}

#[cfg(unix)]
impl<'a> ReplPtyConfig<'a> {
    pub(crate) fn with_color_mode(mut self, color_mode: ReplPtyColorMode) -> Self {
        self.color_mode = color_mode;
        self
    }

    pub(crate) fn without_cursor_position_reports(mut self) -> Self {
        self.cursor_position_reports = false;
        self
    }

    pub(crate) fn with_simple_prompt(mut self, simple_prompt: bool) -> Self {
        self.simple_prompt = simple_prompt;
        self
    }

    pub(crate) fn with_config(mut self, config: &'a str) -> Self {
        self.config = Some(config);
        self
    }

    pub(crate) fn with_plugins_dir(mut self, plugins_dir: &'a Path) -> Self {
        self.plugins_dir = Some(plugins_dir);
        self
    }

    pub(crate) fn with_intro_override(mut self, intro_override: Option<&'a str>) -> Self {
        self.intro_override = intro_override;
        self
    }
}

#[cfg(unix)]
pub(crate) struct ReplPtySession {
    child: Box<dyn portable_pty::Child + Send>,
    writer: Arc<Mutex<Box<dyn Write + Send>>>,
    terminal: Box<dyn portable_pty::MasterPty + Send>,
    interactive: bool,
    // PTY reads can split UTF-8 characters; retain bytes until taking a view.
    output: Arc<Mutex<Vec<u8>>>,
    reader: Option<std::thread::JoinHandle<()>>,
    deadline: Instant,
    _home: TestTempDir,
    _plugins: Option<TestTempDir>,
}

#[cfg(unix)]
pub(crate) struct PtyCommandOutput {
    pub(crate) stdout: String,
    pub(crate) status: portable_pty::ExitStatus,
}

#[cfg(unix)]
fn osp_pty_command_builder(
    home: &Path,
    plugins_path: &Path,
    color_mode: ReplPtyColorMode,
) -> CommandBuilder {
    let mut cmd = CommandBuilder::new(PathBuf::from(env!("CARGO_BIN_EXE_osp")));
    cmd.env_clear();
    cmd.env("PATH", "/usr/bin:/bin");
    cmd.env("LANG", "C.UTF-8");
    for (key, value) in crate::test_env::isolated_env(home) {
        cmd.env(key, value);
    }
    cmd.env("TERM", "xterm-256color");
    cmd.env("COLUMNS", "80");
    cmd.env("LINES", "24");
    cmd.env("OSP_PLUGIN_PATH", plugins_path);
    cmd.env("OSP_BUNDLED_PLUGIN_DIR", plugins_path);
    match color_mode {
        ReplPtyColorMode::Always => {
            cmd.env_remove("NO_COLOR");
            cmd.env("OSP__UI__COLOR__MODE", "always");
        }
        ReplPtyColorMode::Plain => {
            cmd.env("NO_COLOR", "1");
        }
    }
    cmd
}

#[cfg(unix)]
pub(crate) fn run_osp_command_in_pty(
    args: &[&str],
    color_mode: ReplPtyColorMode,
) -> PtyCommandOutput {
    let pty_system = native_pty_system();
    let pair = pty_system
        .openpty(PtySize {
            rows: 24,
            cols: 80,
            pixel_width: 0,
            pixel_height: 0,
        })
        .expect("open pty");

    let home = make_temp_dir("osp-cli-pty-home");
    let plugins = make_temp_dir("osp-cli-pty-plugins");
    let mut cmd = osp_pty_command_builder(&home, &plugins, color_mode);
    cmd.args(args);

    let (stdout, status) = crate::pty_support::capture(pair, cmd, Duration::from_secs(20));
    PtyCommandOutput { stdout, status }
}

#[cfg(unix)]
impl ReplPtySession {
    pub(crate) fn spawn(config: ReplPtyConfig<'_>) -> Self {
        let pty_system = native_pty_system();
        let pair = pty_system
            .openpty(PtySize {
                rows: 24,
                cols: 80,
                pixel_width: 0,
                pixel_height: 0,
            })
            .expect("open pty");

        let home = make_temp_dir("osp-cli-pty-home");
        if let Some(contents) = config.config {
            write_config(&home, contents);
        }

        let (plugins_path, plugins_temp) = match config.plugins_dir {
            Some(path) => (path.to_path_buf(), None),
            None => {
                let temp = make_temp_dir("osp-cli-pty-plugins");
                (temp.to_path_buf(), Some(temp))
            }
        };

        let mut cmd = osp_pty_command_builder(&home, &plugins_path, config.color_mode);
        if let Some(intro) = config.intro_override {
            cmd.env("OSP__REPL__INTRO", intro);
        }
        cmd.env(
            "OSP__REPL__SIMPLE_PROMPT",
            if config.simple_prompt {
                "true"
            } else {
                "false"
            },
        );
        cmd.env("OSP__REPL__HISTORY__ENABLED", "false");
        if config.cursor_position_reports {
            // Most PTY tests exercise interactive rendering paths directly and
            // should not depend on cursor-probe timing.
            cmd.env("OSP__REPL__INPUT_MODE", "interactive");
        }
        let mut reader = pair.master.try_clone_reader().expect("clone reader");
        let writer = Arc::new(Mutex::new(pair.master.take_writer().expect("take writer")));

        let output = Arc::new(Mutex::new(Vec::new()));
        let output_clone = Arc::clone(&output);
        let writer_clone = Arc::clone(&writer);
        let cursor_position_reports = config.cursor_position_reports;
        let child = pair.slave.spawn_command(cmd).expect("spawn osp repl");
        drop(pair.slave);
        let mut session = Self {
            child,
            writer,
            terminal: pair.master,
            interactive: cursor_position_reports,
            output,
            reader: None,
            deadline: Instant::now() + Duration::from_secs(120),
            _home: home,
            _plugins: plugins_temp,
        };
        session.reader = Some(std::thread::spawn(move || {
            let mut buf = [0u8; 4096];
            let cpr_request = [0x1b, 0x5b, 0x36, 0x6e];
            let mut cpr_prefix = 0;
            loop {
                match reader.read(&mut buf) {
                    Ok(0) => break,
                    Ok(n) => {
                        if cursor_position_reports {
                            // Requests may share a read or cross its boundary.
                            for byte in &buf[..n] {
                                cpr_prefix = if *byte == cpr_request[cpr_prefix] {
                                    cpr_prefix + 1
                                } else {
                                    usize::from(*byte == cpr_request[0])
                                };
                                if cpr_prefix == cpr_request.len() {
                                    let mut writer = writer_clone.lock().expect("writer lock");
                                    let _ = writer.write_all(b"\x1b[1;1R");
                                    let _ = writer.flush();
                                    cpr_prefix = 0;
                                }
                            }
                        }
                        output_clone
                            .lock()
                            .expect("output lock")
                            .extend_from_slice(&buf[..n]);
                    }
                    Err(_) => break,
                }
            }
        }));

        session
    }

    pub(crate) fn output_len(&self) -> usize {
        self.output.lock().expect("output lock").len()
    }

    pub(crate) fn output_since(&self, start: usize) -> String {
        let buf = self.output.lock().expect("output lock");
        if start >= buf.len() {
            String::new()
        } else {
            String::from_utf8_lossy(&buf[start..]).into_owned()
        }
    }

    pub(crate) fn output_snapshot(&self, max_len: usize) -> String {
        let buf = self.output.lock().expect("output lock");
        let buf = String::from_utf8_lossy(&buf);
        if buf.len() <= max_len {
            buf.into_owned()
        } else {
            let mut start = buf.len().saturating_sub(max_len);
            while start < buf.len() && !buf.is_char_boundary(start) {
                start += 1;
            }
            buf[start..].to_string()
        }
    }

    pub(crate) fn plain_output_snapshot(&self, max_len: usize) -> String {
        strip_terminal_noise(&self.output_snapshot(max_len))
    }

    fn wait_deadline(&self, timeout: Duration) -> Instant {
        // One-second waits belong to latency contracts; other waits detect hangs.
        let timeout = if timeout > Duration::from_secs(1) {
            timeout.max(Duration::from_secs(15))
        } else {
            timeout
        };
        (Instant::now() + timeout).min(self.deadline)
    }

    pub(crate) fn write_bytes(&mut self, bytes: &[u8]) {
        let deadline = self.wait_deadline(Duration::from_secs(15));
        // A rendered prompt can still belong to the editor being rebuilt.
        while self.interactive
            && self
                .terminal
                .get_termios()
                .is_some_and(|termios| termios.local_flags.bits() & libc::ECHO != 0)
        {
            assert!(
                Instant::now() < deadline,
                "editor never disabled terminal echo; output:\n{}",
                self.output_snapshot(8000),
            );
            std::thread::sleep(Duration::from_millis(5));
        }
        let mut writer = self.writer.lock().expect("writer lock");
        writer.write_all(bytes).expect("write to pty");
        writer.flush().expect("flush pty");
    }

    pub(crate) fn type_text(&mut self, text: &str) {
        let start = self.output_len();
        self.write_bytes(text.as_bytes());
        let deadline = self.wait_deadline(Duration::from_secs(15));
        loop {
            let output = self.output_since(start);
            // Cooked terminal echo can precede editor startup after a reload.
            // Wait for the latest completed editor paint before sending Enter.
            if let Some((_, frame)) = output.rsplit_once("\x1b[?25l")
                && frame.contains("\x1b[?25h")
                && strip_terminal_noise(frame).contains(text.trim())
            {
                return;
            }
            assert!(
                Instant::now() < deadline,
                "expected editor to display typed text {text:?}; output:\n{}",
                self.output_snapshot(8000),
            );
            std::thread::sleep(Duration::from_millis(25));
        }
    }

    fn output_ready(&self, output: &str) -> bool {
        !self.interactive
            || output.rsplit_once("\x1b[?25h").is_some_and(|(_, tail)| {
                !tail.contains("\x1b[?25l") && strip_terminal_noise(tail).is_empty()
            })
    }

    pub(crate) fn wait_for_output_since(
        &self,
        start: usize,
        needle: &str,
        timeout: Duration,
    ) -> bool {
        let deadline = self.wait_deadline(timeout);
        loop {
            {
                let buf = self.output.lock().expect("output lock");
                if start < buf.len() {
                    let output = String::from_utf8_lossy(&buf[start..]);
                    if output.contains(needle) && self.output_ready(&output) {
                        return true;
                    }
                }
            }
            if Instant::now() >= deadline {
                return false;
            }
            std::thread::sleep(Duration::from_millis(25));
        }
    }

    pub(crate) fn wait_for_editor_since(&self, start: usize, timeout: Duration) -> bool {
        let deadline = self.wait_deadline(timeout);
        loop {
            let output = self.output_since(start);
            if let Some((_, frame)) = output.rsplit_once("\x1b[?25l")
                && frame.contains("\x1b[J")
                && frame.contains("\x1b[?25h")
            {
                return true;
            }
            if Instant::now() >= deadline {
                return false;
            }
            std::thread::sleep(Duration::from_millis(10));
        }
    }

    pub(crate) fn wait_for_plain_output(&self, needle: &str, timeout: Duration) -> bool {
        self.wait_for_plain_output_since(0, needle, timeout)
    }

    pub(crate) fn wait_for_plain_output_since(
        &self,
        start: usize,
        needle: &str,
        timeout: Duration,
    ) -> bool {
        let deadline = self.wait_deadline(timeout);
        loop {
            {
                let buf = self.output.lock().expect("output lock");
                if start < buf.len() {
                    let output = String::from_utf8_lossy(&buf[start..]);
                    if strip_terminal_noise(&output).contains(needle) && self.output_ready(&output)
                    {
                        return true;
                    }
                }
            }
            if Instant::now() >= deadline {
                return false;
            }
            std::thread::sleep(Duration::from_millis(25));
        }
    }

    pub(crate) fn wait_for_exit(&mut self, timeout: Duration) -> bool {
        let deadline = self.wait_deadline(timeout);
        loop {
            match self.child.try_wait() {
                Ok(Some(_)) => return true,
                Ok(None) => {}
                Err(_) => return false,
            }
            if Instant::now() >= deadline {
                return false;
            }
            std::thread::sleep(Duration::from_millis(50));
        }
    }

    pub(crate) fn kill(&mut self) {
        #[cfg(unix)]
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

#[cfg(unix)]
impl Drop for ReplPtySession {
    fn drop(&mut self) {
        self.kill();
    }
}

#[cfg(unix)]
pub(crate) fn strip_terminal_noise(text: &str) -> String {
    let bytes = text.as_bytes();
    let mut out = String::with_capacity(text.len());
    let mut idx = 0usize;

    while idx < bytes.len() {
        match bytes[idx] {
            b'\x1b' => {
                idx += 1;
                if idx >= bytes.len() {
                    break;
                }
                match bytes[idx] {
                    b'[' => {
                        idx += 1;
                        while idx < bytes.len() {
                            let byte = bytes[idx];
                            idx += 1;
                            if (b'@'..=b'~').contains(&byte) {
                                break;
                            }
                        }
                    }
                    b']' => {
                        idx += 1;
                        while idx < bytes.len() {
                            if bytes[idx] == b'\x07' {
                                idx += 1;
                                break;
                            }
                            if bytes[idx] == b'\x1b'
                                && idx + 1 < bytes.len()
                                && bytes[idx + 1] == b'\\'
                            {
                                idx += 2;
                                break;
                            }
                            idx += 1;
                        }
                    }
                    _ => idx += 1,
                }
            }
            b'\r' | b'\n' => {
                out.push('\n');
                idx += 1;
            }
            byte if byte.is_ascii_control() => {
                idx += 1;
            }
            _ => {
                let ch = text[idx..]
                    .chars()
                    .next()
                    .expect("utf-8 cursor should stay on a character boundary");
                out.push(ch);
                idx += ch.len_utf8();
            }
        }
    }

    out.split_whitespace().collect::<Vec<_>>().join(" ")
}

#[cfg(unix)]
pub(crate) fn strip_ansi_preserve_newlines(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut chars = text.chars().peekable();

    while let Some(ch) = chars.next() {
        match ch {
            '\u{1b}' => {
                if matches!(chars.peek(), Some('[')) {
                    let _ = chars.next();
                    for next in chars.by_ref() {
                        if ('@'..='~').contains(&next) {
                            break;
                        }
                    }
                }
            }
            '\r' => {}
            ch if ch.is_control() && ch != '\n' => {}
            other => out.push(other),
        }
    }

    out
}
