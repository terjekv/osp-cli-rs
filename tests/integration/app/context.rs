use anyhow::Result;
use clap::Command;
use osp_cli::App;
use osp_cli::app::BufferedUiSink;
use osp_cli::core::plugin::{PLUGIN_PROTOCOL_V1, ResponseMetaV1, ResponseV1};
use osp_cli::{NativeCommand, NativeCommandContext, NativeCommandOutcome, NativeCommandRegistry};
use serde_json::json;

#[test]
fn native_session_context_shares_selection_and_redacts_private_state() {
    let session = osp_cli::NativeSessionContext::default();
    let next_command = session.clone();
    session.set_value("selected_target", "private-resource-ref");
    session.set_context_value("host", "public-label");
    assert_eq!(
        next_command.value("selected_target").as_deref(),
        Some("private-resource-ref")
    );
    let entries = next_command.prompt_entries();
    assert_eq!(entries.len(), 1);
    assert_eq!(entries[0].label, "host");
    assert_eq!(entries[0].value, "public-label");
    let diagnostic = format!("{next_command:?}");
    assert!(diagnostic.contains("[redacted]"));
    assert!(!diagnostic.contains("private-resource-ref"));
    next_command.clear_context(Some("host"));
    assert!(session.prompt_entries().is_empty());
    assert_eq!(
        session.value("selected_target").as_deref(),
        Some("private-resource-ref")
    );
}

struct NativeSubjectCommand {
    observed: std::sync::Arc<std::sync::Mutex<Option<osp_cli::NativeSessionContext>>>,
}

impl NativeCommand for NativeSubjectCommand {
    fn command(&self) -> Command {
        Command::new("remember-host").arg(clap::Arg::new("host").required(true).index(1))
    }

    fn execute(
        &self,
        args: &[String],
        context: &NativeCommandContext<'_>,
    ) -> Result<NativeCommandOutcome> {
        let host = args.first().cloned().unwrap_or_default();
        context
            .session_context
            .set_context_value("host", host.clone());
        *self
            .observed
            .lock()
            .map_err(|_| anyhow::anyhow!("context lock poisoned"))? =
            Some(context.session_context.clone());
        Ok(NativeCommandOutcome::Response(Box::new(ResponseV1 {
            protocol_version: PLUGIN_PROTOCOL_V1,
            ok: true,
            data: json!([{"host": host}]),
            error: None,
            messages: Vec::new(),
            context: None,
            meta: ResponseMetaV1::default(),
        })))
    }
}

#[test]
fn native_command_sets_session_context_with_source_and_time() {
    let observed = std::sync::Arc::new(std::sync::Mutex::new(None));
    let command = NativeSubjectCommand {
        observed: observed.clone(),
    };
    let app = App::new().with_native_commands(NativeCommandRegistry::new().with_command(command));
    let mut sink = BufferedUiSink::default();
    assert_eq!(
        app.run_with_sink(
            [
                "osp",
                "--defaults-only",
                "--json",
                "remember-host",
                "foo.uio.no"
            ],
            &mut sink
        )
        .unwrap(),
        0
    );

    let context = observed.lock().unwrap().clone().unwrap();
    let entries = context.context_entries();
    assert_eq!(entries.len(), 1);
    assert_eq!(entries[0].kind, "host");
    assert_eq!(entries[0].value, "foo.uio.no");
    assert_eq!(entries[0].command, "remember-host");
    assert!(chrono::DateTime::parse_from_rfc3339(&entries[0].when).is_ok());
    assert_eq!(context.prompt_entries()[0].label, "host");
}
