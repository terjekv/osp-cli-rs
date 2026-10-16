pub(crate) fn parse_json_output(
    label: &str,
    args: &[&str],
    stdout: &str,
    stderr: &str,
) -> serde_json::Value {
    serde_json::from_str(stdout).unwrap_or_else(|err| {
        panic!(
            "{label} stdout should be json: {err}\nargs: {:?}\nstdout:\n{}\nstderr:\n{}",
            args, stdout, stderr
        )
    })
}

#[cfg(unix)]
#[path = "../../support/plugin.rs"]
mod plugin;
#[cfg(unix)]
pub(super) use plugin::write_executable_script;
