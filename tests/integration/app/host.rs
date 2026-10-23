use crate::temp_support::make_temp_dir;
use anyhow::Result;
use clap::Command;
use osp_cli::App;
use osp_cli::app::{BufferedUiSink, StdIoUiSink, UiSink};
use osp_cli::config::ConfigLayer;
use osp_cli::core::DisplayRule;
use osp_cli::core::MessageLevel;
use osp_cli::core::plugin::{PLUGIN_PROTOCOL_V1, ResponseMessageV1, ResponseMetaV1, ResponseV1};
use osp_cli::{
    NativeCommand, NativeCommandContext, NativeCommandOutcome, NativeCommandRegistry,
    NativeProgressEvent,
};
use serde_json::json;

use super::support::{parse_json_output, write_executable_script};

fn with_config_path<T>(config_toml: &str, callback: impl FnOnce() -> T) -> T {
    let temp = make_temp_dir("osp-cli-app-host-config");
    let config_path = temp.path().join("config.toml");
    std::fs::write(&config_path, config_toml).expect("config should be written");

    let previous = std::env::var_os("OSP_CONFIG_FILE");
    unsafe {
        std::env::set_var("OSP_CONFIG_FILE", &config_path);
    }

    let result = callback();

    match previous {
        Some(value) => unsafe { std::env::set_var("OSP_CONFIG_FILE", value) },
        None => unsafe { std::env::remove_var("OSP_CONFIG_FILE") },
    }

    result
}

struct NativeProbeCommand;

struct SourceRecordCommand {
    include_source: bool,
}

struct DoctorSourceRecordCommand;

impl NativeCommand for DoctorSourceRecordCommand {
    fn command(&self) -> Command {
        Command::new("doctor").subcommand(Command::new("source-record"))
    }

    fn execute(
        &self,
        args: &[String],
        context: &NativeCommandContext<'_>,
    ) -> Result<NativeCommandOutcome> {
        SourceRecordCommand {
            include_source: true,
        }
        .execute(args, context)
    }
}

impl NativeCommand for SourceRecordCommand {
    fn command(&self) -> Command {
        Command::new("source-record")
    }

    fn execute(
        &self,
        _args: &[String],
        _context: &NativeCommandContext<'_>,
    ) -> Result<NativeCommandOutcome> {
        let mut response = ResponseV1::ok(json!({"Group renamed to": {"new_name": "display"}}));
        response.source = self.include_source.then(|| {
            json!({
                "new_name": "server",
                "enabled": true,
                "server_field": "kept"
            })
        });
        response.meta = ResponseMetaV1 {
            preserve_json_document: true,
            format_hint: Some("json".into()),
            columns: Some(vec!["Group renamed to".into()]),
            detail_columns: vec!["Group renamed to".into()],
            display_rules: vec![DisplayRule::SuffixWhenTrue {
                field: "new_name".into(),
                when: "enabled".into(),
                suffix: " DISPLAY".into(),
            }],
            ..ResponseMetaV1::default()
        };
        Ok(NativeCommandOutcome::Response(Box::new(response)))
    }
}

#[test]
fn source_selection_drives_rendering_json_pipelines_and_quick_search() {
    let app = App::new().with_native_commands(
        NativeCommandRegistry::new()
            .with_command(SourceRecordCommand {
                include_source: true,
            })
            .with_command(DoctorSourceRecordCommand),
    );
    for (args, expected) in [
        (
            vec!["source-record", "--json"],
            json!({"Group renamed to": {"new_name": "display"}}),
        ),
        (
            vec!["source-record", "--source-record", "--json"],
            json!({"new_name": "server", "enabled": true, "server_field": "kept"}),
        ),
        (
            vec![
                "--source-record",
                "source-record",
                "--json",
                "|",
                "P new_name",
            ],
            json!([{"new_name": "server"}]),
        ),
        (
            vec![
                "doctor",
                "source-record",
                "--source-record",
                "--json",
                "|",
                "P new_name",
            ],
            json!([{"new_name": "server"}]),
        ),
        (
            vec![
                "source-record",
                "--source-record",
                "--json",
                "|",
                "server",
                "|",
                "P new_name",
            ],
            json!([{"new_name": "server"}]),
        ),
    ] {
        let mut sink = BufferedUiSink::default();
        assert_eq!(
            app.run_with_sink(
                ["osp", "--defaults-only"].into_iter().chain(args),
                &mut sink
            )
            .unwrap(),
            0
        );
        assert_eq!(
            serde_json::from_str::<serde_json::Value>(&sink.stdout).unwrap(),
            expected
        );
    }

    let mut sink = BufferedUiSink::default();
    assert_eq!(
        app.run_with_sink(
            ["osp", "--defaults-only", "source-record", "--source-record"],
            &mut sink
        )
        .unwrap(),
        0
    );
    assert!(sink.stdout.contains("server"), "{}", sink.stdout);
    assert!(sink.stdout.contains("kept"), "{}", sink.stdout);
    assert!(!sink.stdout.contains("DISPLAY"), "{}", sink.stdout);
    assert!(sink.stderr.is_empty(), "{}", sink.stderr);
    // A JSON producer hint would bypass the generic human renderer.
    assert!(serde_json::from_str::<serde_json::Value>(&sink.stdout).is_err());
}

#[test]
fn source_selection_without_a_source_preserves_data_and_presentation() {
    let app = App::new().with_native_commands(NativeCommandRegistry::new().with_command(
        SourceRecordCommand {
            include_source: false,
        },
    ));
    for tail in [vec![], vec!["--json"], vec!["--json", "|", "C"]] {
        let mut normal = BufferedUiSink::default();
        let mut source = BufferedUiSink::default();
        assert_eq!(
            app.run_with_sink(
                ["osp", "--defaults-only", "source-record"]
                    .into_iter()
                    .chain(tail.iter().copied()),
                &mut normal
            )
            .unwrap(),
            0
        );
        assert_eq!(
            app.run_with_sink(
                ["osp", "--defaults-only", "source-record", "--source-record"]
                    .into_iter()
                    .chain(tail),
                &mut source
            )
            .unwrap(),
            0
        );
        assert_eq!(source.stdout, normal.stdout);
        assert_eq!(source.stderr, normal.stderr);
    }
}

#[cfg(unix)]
#[test]
fn source_selection_captures_plugin_responses_without_a_format_flag() {
    let dir = make_temp_dir("osp-cli-app-source-plugin");
    write_executable_script(
        &dir.path().join("osp-source-plugin"),
        r#"#!/bin/sh
echo '{"protocol_version":1,"ok":true,"data":{"label":"display"},"source":{"name":"server"},"error":null}'
"#,
    );
    let mut sink = BufferedUiSink::default();
    assert_eq!(
        App::new()
            .run_with_sink(
                [
                    "osp",
                    "--defaults-only",
                    "--plugin-dir",
                    dir.to_str().unwrap(),
                    "source-plugin",
                    "--source-record"
                ],
                &mut sink,
            )
            .unwrap(),
        0
    );
    assert!(sink.stdout.contains("server"), "{}", sink.stdout);
    assert!(!sink.stdout.contains("display"), "{}", sink.stdout);
}

struct GroupShapedServiceCommand;

impl NativeCommand for GroupShapedServiceCommand {
    fn command(&self) -> Command {
        Command::new("group-shaped")
    }

    fn execute(
        &self,
        _args: &[String],
        _context: &NativeCommandContext<'_>,
    ) -> Result<NativeCommandOutcome> {
        Ok(NativeCommandOutcome::Response(Box::new(ResponseV1 {
            source: None,
            protocol_version: PLUGIN_PROTOCOL_V1,
            ok: true,
            data: json!([{"groups": {}, "aggregates": {}, "rows": []}]),
            error: None,
            messages: Vec::new(),
            context: None,
            meta: ResponseMetaV1::default(),
        })))
    }
}

#[test]
fn service_group_field_names_are_not_interpreted_as_dsl_groups() {
    let app = App::new()
        .with_native_commands(NativeCommandRegistry::new().with_command(GroupShapedServiceCommand));
    let mut sink = BufferedUiSink::default();
    assert_eq!(
        app.run_with_sink(
            [
                "osp",
                "--defaults-only",
                "--json",
                "group-shaped",
                "|",
                "A",
                "count()",
                "AS",
                "total"
            ],
            &mut sink
        )
        .unwrap(),
        0
    );
    assert_eq!(
        serde_json::from_str::<serde_json::Value>(&sink.stdout).unwrap(),
        json!([{"total": 1}])
    );
}

impl NativeCommand for NativeProbeCommand {
    fn command(&self) -> Command {
        Command::new("native-probe").about("Inspect resolved host config")
    }

    fn execute(
        &self,
        _args: &[String],
        context: &NativeCommandContext<'_>,
    ) -> Result<NativeCommandOutcome> {
        Ok(NativeCommandOutcome::Response(Box::new(ResponseV1 {
            source: None,
            protocol_version: PLUGIN_PROTOCOL_V1,
            ok: true,
            data: json!([{
                "active_profile": context.config.active_profile(),
                "theme": context.config.get_string("theme.name"),
            }]),
            error: None,
            messages: Vec::new(),
            context: None,
            meta: ResponseMetaV1 {
                title_field: None,
                context_not_found: false,
                detail_columns: Vec::new(),
                display_notes: Vec::new(),
                context: Default::default(),
                format_hint: Some("json".to_string()),
                columns: Some(vec!["active_profile".to_string(), "theme".to_string()]),
                column_labels: Vec::new(),
                column_align: Vec::new(),
                row_path: None,
                unix_timestamp_columns: Vec::new(),
                display_rules: Vec::new(),
                preserve_json_document: false,
                progress_replace: false,
                display_limit: None,
                partial_rows: None,
            },
        })))
    }
}

fn native_probe_registry() -> NativeCommandRegistry {
    NativeCommandRegistry::new().with_command(NativeProbeCommand)
}

struct SiteStatusCommand;

struct CuratedOrchRowsCommand;

impl NativeCommand for CuratedOrchRowsCommand {
    fn command(&self) -> Command {
        Command::new("orch-view").about("Render a canonical orchestrator page")
    }

    fn execute(
        &self,
        _args: &[String],
        context: &NativeCommandContext<'_>,
    ) -> Result<NativeCommandOutcome> {
        for state in ["running", "completed"] {
            context.emit_progress(
                NativeProgressEvent::new(json!({
                    "state": {"name": state},
                    "name": "db01.uio.no",
                    "provider": {"name": "vmware"},
                    "created_at": 1700000000,
                    "description": "Resource preparation with a description long enough to wrap on a narrow terminal",
                }))
                .with_meta(ResponseMetaV1 {
                    context_not_found: false,
                    context: Default::default(),
                    format_hint: Some("mreg".into()),
                    columns: Some(vec!["state.name".into(), "name".into(), "description".into()]),
                    progress_replace: true,
                    preserve_json_document: true,
                    ..ResponseMetaV1::default()
                }),
            )?;
        }
        context.flush_progress()?;
        Ok(NativeCommandOutcome::Response(Box::new(ResponseV1 {
            source: None,
            protocol_version: PLUGIN_PROTOCOL_V1,
            ok: true,
            data: json!({
                "items": [{
                    "name": "db01.uio.no",
                    "is_stale": true,
                    "lookup_error": "provider unavailable",
                    "created_at": 1700000000,
                    "provider": {"name": "vmware"},
                    "state": {"name": "powered_on"},
                    "compute": {"display": "4 CPU / 8 GiB"},
                    "location": {"display": "vcsa-prod"}
                }],
                "page": {"next_cursor": "cursor-2"},
                "targets": [{"target_id": "vmware:prod", "status": "ok"}]
            }),
            error: None,
            messages: vec![
                ResponseMessageV1 {
                    level: MessageLevel::Warning,
                    text: "Results are incomplete".to_string(),
                },
                ResponseMessageV1 {
                    level: MessageLevel::Info,
                    text: "provider evidence".to_string(),
                },
                ResponseMessageV1 {
                    level: MessageLevel::Trace,
                    text: "runtime target vmware:prod".to_string(),
                },
            ],
            context: None,
            meta: ResponseMetaV1 {
                title_field: None,
                context_not_found: false,
                detail_columns: Vec::new(),
                display_notes: Vec::new(),
                context: Default::default(),
                format_hint: Some("table".to_string()),
                columns: Some(vec![
                    "name".to_string(),
                    "provider.name".to_string(),
                    "state.name".to_string(),
                    "compute.display".to_string(),
                    "location.display".to_string(),
                    "created_at".to_string(),
                ]),
                column_labels: vec![
                    "NAME".to_string(),
                    "PROVIDER".to_string(),
                    "STATE".to_string(),
                    "COMPUTE".to_string(),
                    "LOCATION".to_string(),
                    "CREATED".to_string(),
                ],
                column_align: Vec::new(),
                row_path: Some("items".to_string()),
                unix_timestamp_columns: vec!["created_at".to_string()],
                display_rules: vec![
                    DisplayRule::SuffixWhenTrue {
                        field: "name".to_string(),
                        when: "is_stale".to_string(),
                        suffix: " (stale)".to_string(),
                    },
                    DisplayRule::BlankWhenPresent {
                        field: "state.name".to_string(),
                        when: "lookup_error".to_string(),
                    },
                ],
                preserve_json_document: true,
                progress_replace: false,
                display_limit: None,
                partial_rows: None,
            },
        })))
    }
}

impl NativeCommand for SiteStatusCommand {
    fn command(&self) -> Command {
        Command::new("site-status").about("Show wrapper defaults from resolved config")
    }

    fn execute(
        &self,
        _args: &[String],
        context: &NativeCommandContext<'_>,
    ) -> Result<NativeCommandOutcome> {
        Ok(NativeCommandOutcome::Response(Box::new(ResponseV1 {
            source: None,
            protocol_version: PLUGIN_PROTOCOL_V1,
            ok: true,
            data: json!([{
                "enabled": context.config.get_bool("extensions.site.enabled").unwrap_or(false),
                "banner": context.config.get_string("extensions.site.banner"),
            }]),
            error: None,
            messages: Vec::new(),
            context: None,
            meta: ResponseMetaV1 {
                title_field: None,
                context_not_found: false,
                detail_columns: Vec::new(),
                display_notes: Vec::new(),
                context: Default::default(),
                format_hint: Some("json".to_string()),
                columns: Some(vec!["enabled".to_string(), "banner".to_string()]),
                column_labels: Vec::new(),
                column_align: Vec::new(),
                row_path: None,
                unix_timestamp_columns: Vec::new(),
                display_rules: Vec::new(),
                preserve_json_document: false,
                progress_replace: false,
                display_limit: None,
                partial_rows: None,
            },
        })))
    }
}

fn site_status_registry() -> NativeCommandRegistry {
    NativeCommandRegistry::new().with_command(SiteStatusCommand)
}

fn curated_orch_rows_registry() -> NativeCommandRegistry {
    NativeCommandRegistry::new().with_command(CuratedOrchRowsCommand)
}

struct TerminalCapture {
    output: BufferedUiSink,
    width: usize,
    height: usize,
    resize_height: Option<usize>,
}

impl UiSink for TerminalCapture {
    fn write_stdout(&mut self, text: &str) {
        self.output.stdout.push_str(text);
    }
    fn write_stderr(&mut self, text: &str) {
        self.output.stderr.push_str(text);
        if let Some(height) = self.resize_height.take() {
            self.height = height;
        }
    }
    fn stderr_is_terminal(&self) -> bool {
        true
    }
    fn stderr_width(&self) -> Option<usize> {
        Some(self.width)
    }
    fn stderr_height(&self) -> Option<usize> {
        Some(self.height)
    }
}

#[test]
fn app_host_keeps_orch_documents_pipeable_while_rendering_curated_rows() {
    let app = App::new().with_native_commands(curated_orch_rows_registry());

    if std::env::var_os("OSP_LIVE_OUTPUT_CONTRACT_CHILD").is_some() {
        if !StdIoUiSink.stderr_is_terminal() {
            assert_eq!(StdIoUiSink.stderr_width(), Some(80));
            assert_eq!(StdIoUiSink.stderr_height(), Some(24));
        }
        app.run_with_sink(
            ["osp", "--defaults-only", "--plain", "orch-view"],
            &mut StdIoUiSink,
        )
        .expect("the real terminal should receive progress and the final result");
        return;
    }

    let mut human = BufferedUiSink::default();
    let exit = app
        .run_with_sink(
            ["osp", "--defaults-only", "--plain", "orch-view"],
            &mut human,
        )
        .expect("curated human output should render");
    assert_eq!(exit, 0);
    assert!(human.stdout.contains("NAME"));
    assert!(human.stdout.contains("PROVIDER"));
    assert!(!human.stdout.contains("provider.name"));
    assert!(human.stdout.contains("4 CPU / 8"));
    assert!(human.stdout.contains("GiB"));
    assert!(human.stdout.contains("db01.uio.no"));
    assert!(human.stdout.contains("db01.uio.no (stale)"));
    assert!(!human.stdout.contains("powered_on"));
    assert!(!human.stdout.contains("next_cursor"));
    assert!(human.stderr.contains("Results are incomplete"));
    assert!(!human.stderr.contains("provider evidence"));
    assert!(!human.stderr.contains("runtime target vmware:prod"));

    let mut values = BufferedUiSink::default();
    assert_eq!(
        app.run_with_sink(
            ["osp", "--defaults-only", "--plain", "--value", "orch-view"],
            &mut values,
        )
        .expect("human value output should retain columns omitted by a narrow table"),
        0,
    );
    assert!(
        values
            .stdout
            .lines()
            .any(|line| line.trim() == "1700000000"),
        "{}",
        values.stdout,
    );

    let mut json_sink = BufferedUiSink::default();
    let json_args = ["osp", "--defaults-only", "--json", "orch-view"];
    let exit = app
        .run_with_sink(json_args, &mut json_sink)
        .expect("canonical JSON should render");
    assert_eq!(exit, 0);
    let document = parse_json_output(
        "app_host_keeps_orch_documents_pipeable_while_rendering_curated_rows/json",
        &json_args,
        &json_sink.stdout,
        &json_sink.stderr,
    );
    assert_eq!(document["page"]["next_cursor"], "cursor-2");
    assert_eq!(document["items"][0]["provider"]["name"], "vmware");
    assert_eq!(document["items"][0]["name"], "db01.uio.no");
    assert_eq!(document["items"][0]["state"]["name"], "powered_on");
    assert_eq!(document["items"][0]["created_at"], 1700000000);

    let mut raw = BufferedUiSink::default();
    app.run_with_sink(
        [
            "osp",
            "--defaults-only",
            "--json",
            "orch-view",
            "|",
            "F",
            "created_at",
            ">=",
            "2023-11-14T22:13:20+00:00",
            "|",
            "P",
            "name",
            "state.name",
            "created_at",
        ],
        &mut raw,
    )
    .expect("DSL filtering and projection should receive raw display fields");
    assert_eq!(
        serde_json::from_str::<serde_json::Value>(&raw.stdout).unwrap(),
        json!([{
            "name": "db01.uio.no",
            "state": {"name": "powered_on"},
            "created_at": 1700000000,
        }]),
    );

    let mut piped = BufferedUiSink::default();
    let pipe_args = [
        "osp",
        "--defaults-only",
        "--json",
        "orch-view",
        "|",
        "P",
        "provider.name",
    ];
    let exit = app
        .run_with_sink(pipe_args, &mut piped)
        .expect("DSL should receive canonical collection rows");
    assert_eq!(exit, 0);
    let projected = parse_json_output(
        "app_host_keeps_orch_documents_pipeable_while_rendering_curated_rows/pipeline",
        &pipe_args,
        &piped.stdout,
        &piped.stderr,
    );
    assert_eq!(projected, json!([{"provider":{"name":"vmware"}}]));

    for format in ["--format=auto", "--md", "--mreg"] {
        let mut rendered = BufferedUiSink::default();
        assert_eq!(
            app.run_with_sink(
                ["osp", "--defaults-only", "--plain", format, "orch-view"],
                &mut rendered,
            )
            .expect("explicit human formats should preserve the curated collection"),
            0,
        );
        let expected_name = if format == "--md" {
            r"db01.uio.no \(stale\)"
        } else {
            "db01.uio.no (stale)"
        };
        assert!(
            rendered.stdout.contains(expected_name),
            "{format}: {}",
            rendered.stdout
        );
        assert!(
            rendered.stdout.contains("4 CPU / 8 GiB"),
            "{format}: {}",
            rendered.stdout
        );
        assert!(
            rendered.stdout.contains("vcsa-prod"),
            "{format}: {}",
            rendered.stdout
        );
        assert!(rendered.stderr.contains("Results are incomplete"));
    }

    // Grouping must retain canonical member rows, even when human projections
    // blank an unavailable state or add a stale marker to its display name.
    for format in ["--json", "--table", "--md", "--mreg", "--value"] {
        let mut grouped = BufferedUiSink::default();
        assert_eq!(
            app.run_with_sink(
                [
                    "osp",
                    "--defaults-only",
                    "--plain",
                    format,
                    "orch-view",
                    "|",
                    "P",
                    "name",
                    "provider.name",
                    "state.name",
                    "|",
                    "G",
                    "provider.name",
                    "AS",
                    "provider",
                    "|",
                    "A",
                    "count()",
                    "AS",
                    "total",
                ],
                &mut grouped,
            )
            .expect("provider grouping should pass through the host and renderer"),
            0,
        );
        if format == "--json" {
            let payload: serde_json::Value = serde_json::from_str(&grouped.stdout).unwrap();
            assert_eq!(
                payload,
                json!([{
                    "groups": {"provider": "vmware"},
                    "aggregates": {"total": 1},
                    "rows": [{"name": "db01.uio.no", "provider": {"name": "vmware"}, "state": {"name": "powered_on"}}],
                }])
            );
        } else {
            assert!(
                grouped.stdout.contains("vmware"),
                "{format}: {}",
                grouped.stdout
            );
            assert!(
                grouped.stdout.contains("db01.uio.no"),
                "{format}: {}",
                grouped.stdout
            );
            let expected_state = if format == "--table" {
                "1 field"
            } else if format == "--md" {
                r"powered\_on"
            } else {
                "powered_on"
            };
            assert!(
                grouped.stdout.contains(expected_state),
                "{format}: {}",
                grouped.stdout
            );
            if format == "--value" {
                assert!(
                    grouped.stdout.lines().any(|line| line.trim() == "1"),
                    "{}",
                    grouped.stdout
                );
            } else {
                assert!(
                    grouped.stdout.contains("total"),
                    "{format}: {}",
                    grouped.stdout
                );
                assert!(
                    grouped.stdout.split_whitespace().any(|token| token == "1"),
                    "{format}: {}",
                    grouped.stdout
                );
            }
        }
    }

    let mut verbose = BufferedUiSink::default();
    app.run_with_sink(["osp", "--defaults-only", "-v", "orch-view"], &mut verbose)
        .expect("normal operator evidence should render at -v");
    assert!(verbose.stderr.contains("provider evidence"));
    assert!(!verbose.stderr.contains("runtime target vmware:prod"));

    let mut trace = BufferedUiSink::default();
    app.run_with_sink(["osp", "--defaults-only", "-vv", "orch-view"], &mut trace)
        .expect("diagnostic evidence should render at -vv");
    assert!(trace.stderr.contains("runtime target vmware:prod"));

    // Live output and the final document share filtering, but occupy separate
    // channels so consuming the final JSON never consumes transient progress.
    assert!(json_sink.stderr.contains("running"));
    assert!(json_sink.stderr.contains("db01.uio.no"));
    assert!(json_sink.stderr.contains("completed"));
    assert!(!json_sink.stderr.contains('\x1b'));
    for (width, height, resize_height) in [
        (80, 24, None),
        (120, 24, None),
        (80, 2, None),
        (80, 24, Some(2)),
    ] {
        let mut terminal = TerminalCapture {
            output: BufferedUiSink::default(),
            width,
            height,
            resize_height,
        };
        app.run_with_sink(
            ["osp", "--defaults-only", "--plain", "orch-view"],
            &mut terminal,
        )
        .expect("live presentations should fit the terminal or append safely");
        let appends_progress = height <= 2 || resize_height.is_some();
        assert_eq!(terminal.output.stderr.contains("running"), appends_progress);
        assert_eq!(
            terminal.output.stderr.contains("completed"),
            appends_progress
        );
        assert_eq!(
            terminal.output.stderr.contains("db01.uio.no"),
            appends_progress
        );
        assert!(!terminal.output.stderr.contains("\x1b[2K"));
        assert!(terminal.output.stdout.contains("db01.uio.no"));
    }
    let mut filtered = BufferedUiSink::default();
    app.run_with_sink(
        [
            "osp",
            "--defaults-only",
            "--json",
            "orch-view",
            "|",
            "completed",
        ],
        &mut filtered,
    )
    .expect("live events should obey the invocation filter");
    assert!(filtered.stderr.contains("completed"));
    assert!(!filtered.stderr.contains("running"));
    assert!(!filtered.stderr.contains('\x1b'));
    assert_eq!(
        serde_json::from_str::<serde_json::Value>(&filtered.stdout).unwrap(),
        json!([])
    );

    // Exercise actual terminal dimensions and paging, not just a sink that
    // claims to be a terminal. Even a failed pager must deliver the result.
    #[cfg(unix)]
    {
        use portable_pty::{CommandBuilder, PtySize, native_pty_system};
        let redirected = assert_cmd::Command::new(std::env::current_exe().unwrap())
            .timeout(std::time::Duration::from_secs(20))
            .args([
                "--exact",
                "app::host::app_host_keeps_orch_documents_pipeable_while_rendering_curated_rows",
                "--nocapture",
            ])
            .env("OSP_LIVE_OUTPUT_CONTRACT_CHILD", "1")
            .env("COLUMNS", "80")
            .env("LINES", "24")
            .output()
            .expect("capture redirected host output");
        assert!(redirected.status.success());
        let stdout = String::from_utf8(redirected.stdout).unwrap();
        let stderr = String::from_utf8(redirected.stderr).unwrap();
        assert!(stdout.contains("4 CPU / 8 GiB"));
        assert!(stderr.contains("running"));
        assert!(stderr.contains("db01.uio.no"));
        assert!(stderr.contains("completed"));
        assert!(!stderr.contains('\x1b'));

        for (height, pager) in [(24, "cat"), (2, "cat"), (2, "exit 1")] {
            let pair = native_pty_system()
                .openpty(PtySize {
                    rows: height,
                    cols: 80,
                    pixel_width: 0,
                    pixel_height: 0,
                })
                .expect("open terminal for the host output contract");
            let mut command = CommandBuilder::new(std::env::current_exe().unwrap());
            command.args([
                "--exact",
                "app::host::app_host_keeps_orch_documents_pipeable_while_rendering_curated_rows",
                "--nocapture",
            ]);
            command.env("OSP_LIVE_OUTPUT_CONTRACT_CHILD", "1");
            command.env("PAGER", pager);
            let (captured, status) =
                crate::pty_support::capture(pair, command, std::time::Duration::from_secs(20));
            assert!(status.success(), "{captured}");
            assert_eq!(captured.contains("state.name:  running"), height <= 2);
            assert_eq!(captured.contains("completed"), height <= 2);
            assert!(captured.contains("db01.uio.no"));
            assert!(captured.contains("4 CPU / 8 GiB"), "{captured}");
        }
    }
}

#[cfg(unix)]
fn write_route_probe_plugin(dir: &std::path::Path, plugin_id: &str, command_name: &str) {
    let plugin_path = dir.join(format!("osp-{plugin_id}"));
    let script = format!(
        r#"#!/bin/sh
PATH=/usr/bin:/bin:$PATH
if [ "$1" = "--osp-describe" ]; then
  cat <<'JSON'
{{"about":"{command_name} route probe","args":[],"flags":{{}},"subcommands":[],"output":"json"}}
JSON
  exit 0
fi

cat <<JSON
{{"protocol_version":1,"ok":true,"data":[{{"profile":"${{OSP_PROFILE:-}}","selected_command":"${{OSP_COMMAND:-}}","arg0":"${{1:-}}","arg1":"${{2:-}}"}}],"error":null,"meta":{{"format_hint":"json"}}}}
JSON
"#,
        command_name = command_name,
    );
    write_executable_script(&plugin_path, &script);
}

#[test]
fn app_host_surfaces_native_commands_in_help_and_dispatch() {
    let app = App::new().with_native_commands(native_probe_registry());
    let help_args = ["osp", "--defaults-only", "--help"];

    let mut help_sink = BufferedUiSink::default();
    let exit = app
        .run_with_sink(help_args, &mut help_sink)
        .expect("help should render");
    assert_eq!(exit, 0);
    assert!(help_sink.stdout.contains("native-probe"));
    assert!(help_sink.stdout.contains("Inspect resolved host config"));

    let dispatch_args = ["osp", "--defaults-only", "--json", "native-probe"];
    let mut dispatch_sink = BufferedUiSink::default();
    let exit = app
        .run_with_sink(dispatch_args, &mut dispatch_sink)
        .expect("native command should dispatch");
    assert_eq!(exit, 0);

    let payload = parse_json_output(
        "app_host_surfaces_native_commands_in_help_and_dispatch/native",
        &dispatch_args,
        &dispatch_sink.stdout,
        &dispatch_sink.stderr,
    );
    let rows = payload
        .as_array()
        .expect("native command output should be row array");
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0]["active_profile"], "default");
}

#[test]
fn app_value_api_layers_product_defaults_and_process_style_failures() {
    let mut product_defaults = ConfigLayer::default();
    product_defaults.set("extensions.site.enabled", true);
    product_defaults.insert(
        "extensions.site.banner",
        "cli-wrapper",
        osp_cli::config::Scope::terminal("cli"),
    );

    let app = App::new()
        .with_native_commands(site_status_registry())
        .with_product_defaults(product_defaults);

    let help_args = ["osp", "--defaults-only", "--help"];
    let mut help_sink = BufferedUiSink::default();
    let help_exit = app.run_process_with_sink(help_args, &mut help_sink);
    assert_eq!(help_exit, 0);

    let status_args = ["osp", "--json", "--defaults-only", "site-status"];
    let mut status_sink = BufferedUiSink::default();
    let status_exit = app.run_process_with_sink(status_args, &mut status_sink);
    assert_eq!(status_exit, 0);
    let payload = parse_json_output(
        "app_value_api_layers_product_defaults_and_process_style_failures/status",
        &status_args,
        &status_sink.stdout,
        &status_sink.stderr,
    );
    let rows = payload
        .as_array()
        .expect("site status output should be a row array");
    assert_eq!(rows[0]["enabled"], true);
    assert_eq!(rows[0]["banner"], "cli-wrapper");

    let mut invalid_sink = BufferedUiSink::default();
    let invalid_args = [
        "osp",
        "--defaults-only",
        "--quiet",
        "--definitely-not-a-flag",
    ];
    let invalid_exit = app.run_process_with_sink(invalid_args, &mut invalid_sink);
    assert_ne!(invalid_exit, 0);
    assert!(invalid_sink.stdout.is_empty());
    assert!(!invalid_sink.stderr.is_empty());
    assert!(
        invalid_sink.stderr.contains("definitely-not-a-flag")
            || invalid_sink.stderr.contains("unexpected argument")
    );
}

#[test]
fn app_host_passes_default_and_selected_profiles_into_native_context() {
    if crate::isolation::isolated_test() {
        return;
    }
    with_config_path(
        r#"[default]
profile.default = "uio"
theme.name = "nord"

[profile.tsd]
theme.name = "dracula"
"#,
        || {
            let app = App::new().with_native_commands(native_probe_registry());

            let default_args = ["osp", "--no-env", "--json", "native-probe"];
            let mut default_sink = BufferedUiSink::default();
            let exit = app
                .run_with_sink(default_args, &mut default_sink)
                .expect("default profile command should run");
            assert_eq!(exit, 0);
            let payload = parse_json_output(
                "app_host_passes_default_and_selected_profiles_into_native_context/default",
                &default_args,
                &default_sink.stdout,
                &default_sink.stderr,
            );
            let rows = payload
                .as_array()
                .expect("default output should be row array");
            assert_eq!(rows[0]["active_profile"], "uio");
            assert_eq!(rows[0]["theme"], "nord");

            let explicit_args = [
                "osp",
                "--no-env",
                "--profile",
                "tsd",
                "--json",
                "native-probe",
            ];
            let mut explicit_sink = BufferedUiSink::default();
            let exit = app
                .run_with_sink(explicit_args, &mut explicit_sink)
                .expect("explicit profile command should run");
            assert_eq!(exit, 0);
            let payload = parse_json_output(
                "app_host_passes_default_and_selected_profiles_into_native_context/explicit",
                &explicit_args,
                &explicit_sink.stdout,
                &explicit_sink.stderr,
            );
            let rows = payload
                .as_array()
                .expect("explicit output should be row array");
            assert_eq!(rows[0]["active_profile"], "tsd");
            assert_eq!(rows[0]["theme"], "dracula");

            let positional_args = ["osp", "--no-env", "--profile=tsd", "--json", "native-probe"];
            let mut positional_sink = BufferedUiSink::default();
            let exit = app
                .run_with_sink(positional_args, &mut positional_sink)
                .expect("attached profile flag command should run");
            assert_eq!(exit, 0);
            let payload = parse_json_output(
                "app_host_passes_default_and_selected_profiles_into_native_context/positional",
                &positional_args,
                &positional_sink.stdout,
                &positional_sink.stderr,
            );
            let rows = payload
                .as_array()
                .expect("positional output should be row array");
            assert_eq!(rows[0]["active_profile"], "tsd");
            assert_eq!(rows[0]["theme"], "dracula");
        },
    );
}

#[test]
fn app_host_projects_native_commands_into_repl_completion_surface() {
    let app = App::new().with_native_commands(native_probe_registry());

    let args = [
        "osp",
        "--json",
        "--defaults-only",
        "repl",
        "debug-complete",
        "--line",
        "native-",
    ];
    let mut sink = BufferedUiSink::default();
    let exit = app
        .run_with_sink(args, &mut sink)
        .expect("debug-complete should run");
    assert_eq!(exit, 0);
    assert!(sink.stderr.is_empty());

    let payload = parse_json_output(
        "app_host_projects_native_commands_into_repl_completion_surface",
        &args,
        &sink.stdout,
        &sink.stderr,
    );
    let matches = payload["matches"]
        .as_array()
        .expect("matches should render as an array");
    assert!(matches.iter().any(|item| item["label"] == "native-probe"));
}

#[cfg(unix)]
#[test]
fn app_host_routes_attached_and_separate_profile_flags_the_same_for_external_commands() {
    if crate::isolation::isolated_test() {
        return;
    }
    with_config_path(
        r#"[default]
profile.default = "uio"

[profile.tsd]
theme.name = "dracula"
"#,
        || {
            let dir = make_temp_dir("osp-cli-app-host-route-plugin");
            write_route_probe_plugin(dir.path(), "route-probe", "route-probe");
            let plugin_dir = dir.to_str().expect("plugin dir should be utf-8");
            let app = App::new();

            let explicit_args = [
                "osp",
                "--json",
                "--no-env",
                "--plugin-dir",
                plugin_dir,
                "--profile",
                "tsd",
                "route-probe",
                "hello",
            ];
            let mut explicit_sink = BufferedUiSink::default();
            let exit = app
                .run_with_sink(explicit_args, &mut explicit_sink)
                .expect("explicit profile command should run");
            assert_eq!(exit, 0);

            let positional_args = [
                "osp",
                "--json",
                "--no-env",
                "--plugin-dir",
                plugin_dir,
                "--profile=tsd",
                "route-probe",
                "hello",
            ];
            let mut positional_sink = BufferedUiSink::default();
            let exit = app
                .run_with_sink(positional_args, &mut positional_sink)
                .expect("attached profile flag command should run");
            assert_eq!(exit, 0);

            let explicit = parse_json_output(
                "app_host_routes_attached_and_separate_profile_flags_the_same_for_external_commands/explicit",
                &explicit_args,
                &explicit_sink.stdout,
                &explicit_sink.stderr,
            );
            let positional = parse_json_output(
                "app_host_routes_attached_and_separate_profile_flags_the_same_for_external_commands/positional",
                &positional_args,
                &positional_sink.stdout,
                &positional_sink.stderr,
            );
            let explicit_row = explicit
                .as_array()
                .expect("explicit payload should be a row array")
                .first()
                .expect("explicit payload should contain one row");
            let positional_row = positional
                .as_array()
                .expect("positional payload should be a row array")
                .first()
                .expect("positional payload should contain one row");
            assert_eq!(explicit_row["profile"], "tsd");
            assert_eq!(positional_row["profile"], "tsd");
            assert_eq!(explicit_row["selected_command"], "route-probe");
            assert_eq!(positional_row["selected_command"], "route-probe");
            assert_eq!(explicit_row["arg0"], positional_row["arg0"]);
            assert_eq!(explicit_row["arg1"], positional_row["arg1"]);
        },
    );
}

#[cfg(unix)]
#[test]
fn app_host_keeps_unknown_leading_token_as_command_instead_of_profile() {
    if crate::isolation::isolated_test() {
        return;
    }
    with_config_path(
        r#"[default]
profile.default = "uio"

[profile.tsd]
theme.name = "dracula"
"#,
        || {
            let dir = make_temp_dir("osp-cli-app-host-unknown-profile-token");
            write_route_probe_plugin(dir.path(), "prod", "prod");
            let plugin_dir = dir.to_str().expect("plugin dir should be utf-8");
            let app = App::new();

            let mut sink = BufferedUiSink::default();
            let args = [
                "osp",
                "--json",
                "--no-env",
                "--plugin-dir",
                plugin_dir,
                "prod",
            ];
            let exit = app
                .run_with_sink(args, &mut sink)
                .expect("unknown leading token command should run");
            assert_eq!(exit, 0);

            let payload = parse_json_output(
                "app_host_keeps_unknown_leading_token_as_command_instead_of_profile",
                &args,
                &sink.stdout,
                &sink.stderr,
            );
            let row = payload
                .as_array()
                .expect("payload should be a row array")
                .first()
                .expect("payload should contain one row");
            assert_eq!(row["selected_command"], "prod");
            assert_eq!(row["profile"], "uio");
        },
    );
}
