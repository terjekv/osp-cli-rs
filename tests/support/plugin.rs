#[cfg(unix)]
use std::path::Path;

#[cfg(unix)]
pub(crate) fn write_executable_script(path: &Path, script: &str) {
    use std::os::unix::fs::PermissionsExt;

    std::fs::write(path, script).expect("plugin script should be written");
    let mut perms = std::fs::metadata(path)
        .expect("plugin metadata should be readable")
        .permissions();
    perms.set_mode(0o755);
    std::fs::set_permissions(path, perms).expect("plugin script should be executable");
}
