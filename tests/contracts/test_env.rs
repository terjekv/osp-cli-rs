use std::ffi::OsString;
use std::path::Path;

pub(crate) fn isolated_env(home: &Path) -> Vec<(&'static str, OsString)> {
    let mut env = vec![
        ("HOME", home.as_os_str().to_owned()),
        ("COLUMNS", "80".into()),
        ("LINES", "24".into()),
        ("LANG", "C.UTF-8".into()),
        ("LC_ALL", "C.UTF-8".into()),
        ("XDG_CONFIG_HOME", home.join(".config").into_os_string()),
        ("XDG_CACHE_HOME", home.join(".cache").into_os_string()),
        (
            "XDG_STATE_HOME",
            home.join(".local").join("state").into_os_string(),
        ),
    ];
    // env_clear callers must keep the instrumented child process's report
    // destination, otherwise their asserted CLI behavior is absent from coverage.
    if let Some(profile) = std::env::var_os("LLVM_PROFILE_FILE") {
        env.push(("LLVM_PROFILE_FILE", profile));
    }
    env
}

#[allow(dead_code)]
pub(crate) fn isolated_env_with_config_home(
    home: &Path,
    xdg_config_home: &Path,
) -> Vec<(&'static str, OsString)> {
    let mut env = isolated_env(home);
    env.iter_mut()
        .find(|(key, _)| *key == "XDG_CONFIG_HOME")
        .expect("isolated config home")
        .1 = xdg_config_home.into();
    env
}

/// Construct a bounded CLI contract process.
#[allow(dead_code)]
pub(crate) fn command(program: impl AsRef<std::ffi::OsStr>) -> assert_cmd::Command {
    let mut command = assert_cmd::Command::new(program);
    command.timeout(std::time::Duration::from_secs(30));
    command
}
