#[test]
fn cli_reports_workspace_version_with_long_flag_contract() {
    let mut cmd = crate::test_env::command(assert_cmd::cargo::cargo_bin!("osp"));
    cmd.arg("--version");

    cmd.assert()
        .success()
        .stderr("")
        .stdout(format!("osp {}\n", env!("CARGO_PKG_VERSION")));
}

#[test]
fn cli_reports_workspace_version_with_short_flag_contract() {
    let mut cmd = crate::test_env::command(assert_cmd::cargo::cargo_bin!("osp"));
    cmd.arg("-V");

    cmd.assert()
        .success()
        .stderr("")
        .stdout(format!("osp {}\n", env!("CARGO_PKG_VERSION")));
}
