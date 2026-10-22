use anyhow::Result;
use clap::Command;
use osp_cli::app::BufferedUiSink;
use osp_cli::core::plugin::ResponseV1;
use osp_cli::diagnostics::{self, CredentialSource, DiagnosticEvent as Event, DiagnosticSink};
use osp_cli::{
    App, NativeCommand, NativeCommandContext, NativeCommandOutcome, NativeCommandRegistry,
};
use serde_json::json;

struct ServiceProbe;
impl NativeCommand for ServiceProbe {
    fn command(&self) -> Command {
        Command::new("service-probe")
    }
    fn execute(
        &self,
        _: &[String],
        context: &NativeCommandContext<'_>,
    ) -> Result<NativeCommandOutcome> {
        let sink = context.diagnostics().unwrap();
        sink.scope(|| {
            assert!(DiagnosticSink::current().is_some());
            diagnostics::emit(Event::Context {
                service: "Inventory", url: "https://user:url-secret@inventory.example/hosts?token=query-secret#fragment-secret".into(),
                identity: Some("operator\nforged".into()), profile: "default".into(), credential: CredentialSource::SavedSession,
            });
            diagnostics::emit(Event::CatalogGap { service: "Inventory", operations: vec!["GET /new-hosts".into()] });
            diagnostics::emit(Event::Cache { name: "hosts", hit: true, age_seconds: Some(4000), ttl_seconds: Some(3600) });
            diagnostics::emit(Event::Exchange { method: "GET", url: "https://inventory.example/hosts?token=query-secret".into(), status: Some(401), failure: None, elapsed_ms: 12, request_id: Some("req-1\nforged".into()), bytes: Some(64), content_type: Some("application/json") });
            diagnostics::emit(Event::Recovery { retry_available: true });
            diagnostics::emit(Event::RequestFacts { deadline_seconds: Some(30), projection: Some("detail"), cursor_page: true });
            diagnostics::emit(Event::Exchange { method: "GET", url: "https://inventory.example/hosts".into(), status: Some(200), failure: None, elapsed_ms: 8, request_id: Some("req-2".into()), bytes: Some(128), content_type: Some("application/json") });
            diagnostics::emit(Event::RequestIds { header: "req-2".into(), body: "body-2".into() });
        });
        Ok(NativeCommandOutcome::Response(Box::new(ResponseV1::ok(
            json!({"host":"db01"}),
        ))))
    }
}

#[test]
fn adapter_evidence_obeys_verbosity_and_keeps_json_stdout_clean() {
    let app =
        App::new().with_native_commands(NativeCommandRegistry::new().with_command(ServiceProbe));
    for (level, flag) in ["-qq", "-v", "-vv", "-vvv"].into_iter().enumerate() {
        let mut sink = BufferedUiSink::default();
        assert_eq!(
            app.run_with_sink(
                ["osp", "--defaults-only", "--json", flag, "service-probe"],
                &mut sink
            )
            .unwrap(),
            0
        );
        assert_eq!(
            serde_json::from_str::<serde_json::Value>(&sink.stdout).unwrap(),
            json!([{"host":"db01"}])
        );
        let info = "Inventory at https://inventory.example/hosts as operator\\nforged (profile default, saved session).";
        assert_eq!(sink.stderr.contains(info), level >= 1, "{}", sink.stderr);
        assert_eq!(
            sink.stderr
                .contains("HTTP 401 in 12 ms (request req-1\\nforged)."),
            level >= 2,
            "{}",
            sink.stderr
        );
        assert_eq!(
            sink.stderr.contains("Retrying after session renewal."),
            level >= 2
        );
        assert_eq!(
            sink.stderr.contains("cache: hosts stale; used."),
            level >= 2
        );
        assert_eq!(sink.stderr.contains("response-bytes=128"), level >= 3);
        assert_eq!(sink.stderr.contains("query.view=detail"), level >= 3);
        assert_eq!(
            sink.stderr
                .contains("Header request-id=req-2; body request-id=body-2"),
            level >= 3
        );
        assert_eq!(sink.stderr.contains("(2 requests)"), level >= 1);
        for secret in ["url-secret", "query-secret", "fragment-secret"] {
            assert!(!sink.stderr.contains(secret));
        }
    }
}
