use anyhow::Result;
use assert_cmd::Command as Process;
use clap::{Arg, Command};
use osp_cli::context::{ContextContract, Fill};
use osp_cli::core::plugin::ResponseV1;
use osp_cli::{
    App, NativeCommand, NativeCommandContext, NativeCommandOutcome, NativeCommandRegistry,
};
use serde_json::json;
use std::time::Duration;

struct Inventory;
impl NativeCommand for Inventory {
    fn command(&self) -> Command {
        Command::new("inventory").subcommand(
            Command::new("read")
                .arg(Arg::new("name").required(true))
                .add(
                    ContextContract::default()
                        .consumes("host", "name", Fill::Read)
                        .produces("host", "/name"),
                ),
        )
    }
    fn persisted_context_kinds(&self) -> &'static [&'static str] {
        &["host"]
    }
    fn context_origin(&self, _: &osp_cli::config::ResolvedConfig) -> String {
        "https://inventory.example".into()
    }
    fn execute(
        &self,
        args: &[String],
        _: &NativeCommandContext<'_>,
    ) -> Result<NativeCommandOutcome> {
        let matches = self.command().try_get_matches_from(
            std::iter::once("inventory".into()).chain(args.iter().cloned()),
        )?;
        let name = matches
            .subcommand_matches("read")
            .unwrap()
            .get_one::<String>("name")
            .unwrap();
        Ok(NativeCommandOutcome::Response(Box::new(ResponseV1::ok(
            json!({"name":name}),
        ))))
    }
}

#[test]
fn opted_in_context_survives_processes_and_clear_updates_the_private_store() {
    const CHILD: &str = "OSP_CONTEXT_PERSISTENCE_CHILD";
    if std::env::var_os(CHILD).is_some() {
        let app =
            App::new().with_native_commands(NativeCommandRegistry::new().with_command(Inventory));
        assert_eq!(app.run_from(["osp"]).unwrap(), 0);
        return;
    }
    let home = crate::temp_support::make_temp_dir("context-persistence");
    let run = |input: &str| {
        let mut command = Process::new(std::env::current_exe().unwrap());
        command.args(["--exact", "context_persistence::opted_in_context_survives_processes_and_clear_updates_the_private_store", "--nocapture"])
            .env(CHILD, "1").env("HOME", home.path())
            .env("XDG_STATE_HOME", home.path().join("state"))
            .env("XDG_CONFIG_HOME", home.path().join("config"))
            .env("XDG_CACHE_HOME", home.path().join("cache"))
            .env("OSP__CONTEXT__SEED", "[]").env("OSP__REPL__INTRO", "none")
            .timeout(Duration::from_secs(20)).write_stdin(input);
        let output = command.output().unwrap();
        assert!(
            output.status.success(),
            "{}\n{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
        String::from_utf8(output.stdout).unwrap()
    };
    run("inventory read durable.example\nexit\n");
    let path = home.path().join("state/osp/context.json");
    let stored: serde_json::Value = serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
    assert_eq!(stored[0]["value"], "durable.example");
    assert_eq!(stored[0]["origin"], "https://inventory.example");
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        assert_eq!(
            std::fs::metadata(&path).unwrap().permissions().mode() & 0o777,
            0o600
        );
    }
    let output = run("inventory read --json\ncontext clear host\nexit\n");
    assert!(output.contains("\"name\": \"durable.example\""), "{output}");
    let stored: serde_json::Value = serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
    assert_eq!(stored, json!([]));
}
