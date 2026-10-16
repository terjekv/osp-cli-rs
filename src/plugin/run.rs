//! Subprocess execution: inherited terminals for runs, bounded optional describe.

use super::{Plugin, PluginDispatchContext};
use crate::core::plugin::PluginDescribe;
use miette::{IntoDiagnostic, Result, miette};
use std::io::Read;
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

const MAX_STDOUT: u64 = 64 * 1024 * 1024;

pub(crate) struct RunOutput {
    pub(crate) status: i32,
    pub(crate) stdout: Option<Vec<u8>>,
    pub(crate) wrote_stderr: bool,
}

pub(crate) fn run(
    plugin: &Plugin,
    args: &[String],
    context: &PluginDispatchContext,
    capture: bool,
) -> Result<RunOutput> {
    let mut command = Command::new(&plugin.path);
    command
        .args(args)
        .stdin(Stdio::inherit())
        .stderr(Stdio::piped())
        .stdout(if capture {
            Stdio::piped()
        } else {
            Stdio::inherit()
        })
        .envs(context.runtime_hints.env_pairs())
        .env("OSP_COMMAND", &plugin.name);
    if let Some(user) = &context.user {
        command.env("OSP_USER", user);
    }
    command.envs(context.shared_env.iter().cloned());
    if let Some(env) = context.plugin_env.get(&plugin.name) {
        command.envs(env.iter().cloned());
    }
    #[cfg(unix)]
    let _signal = IgnoreInterrupt::new(&mut command)?;
    let mut child = spawn(&mut command, plugin)?;
    let stderr = child.stderr.take().map(|mut stream| {
        std::thread::spawn(move || {
            let mut wrote = false;
            let mut buffer = [0; 8192];
            loop {
                match stream.read(&mut buffer) {
                    Ok(0) | Err(_) => break,
                    Ok(_) => wrote = true,
                }
            }
            wrote
        })
    });
    let stdout = if let Some(stdout) = child.stdout.take() {
        match read_stdout(stdout) {
            Ok(bytes) => Some(bytes),
            Err(error) => {
                let _ = child.kill();
                let _ = child.wait();
                return Err(error);
            }
        }
    } else {
        None
    };
    let status = child.wait().into_diagnostic()?;
    #[cfg(unix)]
    let code = {
        use std::os::unix::process::ExitStatusExt;
        status
            .code()
            .unwrap_or_else(|| 128 + status.signal().unwrap_or(1))
    };
    #[cfg(not(unix))]
    let code = status.code().unwrap_or(1);
    Ok(RunOutput {
        wrote_stderr: stderr.is_some_and(|reader| reader.join().unwrap_or(false)),
        status: code,
        stdout,
    })
}

pub(super) fn describe(plugin: &Plugin, timeout: Duration) -> Result<PluginDescribe> {
    let mut command = Command::new(&plugin.path);
    command
        .arg("--osp-describe")
        .env("OSP_COMMAND", &plugin.name)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null());
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        command.process_group(0);
    }
    let mut child = spawn(&mut command, plugin)?;
    #[expect(
        clippy::expect_used,
        reason = "stdout was explicitly configured as piped"
    )]
    let stdout = child.stdout.take().expect("describe stdout is piped");
    let reader = std::thread::spawn(move || read_stdout(stdout));
    let started = Instant::now();
    let status = loop {
        match child.try_wait() {
            Ok(Some(status)) => break Ok(status),
            Ok(None) if started.elapsed() < timeout => std::thread::sleep(Duration::from_millis(5)),
            Ok(None) => {
                break Err(miette!(
                    "describe timed out after {} ms",
                    timeout.as_millis()
                ));
            }
            Err(error) => break Err(miette!("failed to wait for describe: {error}")),
        }
    };
    // Also close inherited pipes in any describe descendants.
    #[cfg(unix)]
    unsafe {
        libc::kill(-(child.id() as i32), libc::SIGKILL);
    }
    if status.is_err() {
        let _ = child.kill();
    }
    let _ = child.wait();
    let bytes = reader
        .join()
        .map_err(|_| miette!("describe reader failed"))??;
    let status = status?;
    if !status.success() {
        return Err(miette!(
            "describe exited with status {}",
            status.code().unwrap_or(1)
        ));
    }
    let describe: PluginDescribe =
        serde_json::from_slice(&bytes).map_err(|_| miette!("invalid describe JSON metadata"))?;
    crate::core::plugin::validate_command(&describe.clone().command(&plugin.name))
        .map_err(|_| miette!("invalid describe command metadata"))?;
    Ok(describe)
}

fn read_stdout(stdout: impl Read) -> Result<Vec<u8>> {
    let mut bytes = Vec::new();
    stdout
        .take(MAX_STDOUT + 1)
        .read_to_end(&mut bytes)
        .into_diagnostic()?;
    if bytes.len() as u64 > MAX_STDOUT {
        return Err(miette!("plugin stdout exceeds 64 MiB"));
    }
    Ok(bytes)
}

fn spawn(command: &mut Command, plugin: &Plugin) -> Result<Child> {
    for attempt in 0..5 {
        match command.spawn() {
            Ok(child) => return Ok(child),
            Err(error) if error.raw_os_error() == Some(libc::ETXTBSY) && attempt < 4 => {
                std::thread::sleep(Duration::from_millis(10))
            }
            Err(error) => {
                return Err(miette!(
                    "failed to run plugin {} ({}): {error}",
                    plugin.name,
                    plugin.path.display()
                ));
            }
        }
    }
    unreachable!()
}

#[cfg(unix)]
struct IgnoreInterrupt {
    previous: libc::sigaction,
    _lock: std::sync::MutexGuard<'static, ()>,
}
#[cfg(unix)]
impl IgnoreInterrupt {
    fn new(command: &mut Command) -> Result<Self> {
        use std::os::unix::process::CommandExt;
        static SIGNAL_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());
        let lock = SIGNAL_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        // SIGINT must reach the foreground child while the waiting host survives.
        unsafe {
            let mut ignore: libc::sigaction = std::mem::zeroed();
            let mut previous = std::mem::zeroed();
            ignore.sa_sigaction = libc::SIG_IGN;
            libc::sigemptyset(&mut ignore.sa_mask);
            if libc::sigaction(libc::SIGINT, &ignore, &mut previous) != 0 {
                return Err(miette!(
                    "failed to ignore SIGINT: {}",
                    std::io::Error::last_os_error()
                ));
            }
            command.pre_exec(|| {
                let mut default: libc::sigaction = std::mem::zeroed();
                default.sa_sigaction = libc::SIG_DFL;
                libc::sigemptyset(&mut default.sa_mask);
                if libc::sigaction(libc::SIGINT, &default, std::ptr::null_mut()) != 0 {
                    return Err(std::io::Error::last_os_error());
                }
                Ok(())
            });
            Ok(Self {
                previous,
                _lock: lock,
            })
        }
    }
}
#[cfg(unix)]
impl Drop for IgnoreInterrupt {
    fn drop(&mut self) {
        unsafe {
            libc::sigaction(libc::SIGINT, &self.previous, std::ptr::null_mut());
        }
    }
}
