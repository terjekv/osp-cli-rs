use crate::core::OutputFormat;
use crate::core::output_model::{
    OutputDocument, OutputMeta, OutputResult, output_items_from_value,
};
use crate::core::{DocBlock, Document, DocumentEntry};
use crate::dsl::{apply_output_pipeline, parse_pipeline};
use crate::ui::{RenderRuntime, RenderSettings, TableBorderStyle, TableOverflow, render};
use serde_json::{Value, json};

fn export_settings(format: OutputFormat) -> RenderSettings {
    let mut settings = RenderSettings::test_plain(format);
    settings.format_explicit = true;
    settings.width = Some(100);
    settings.width_max = 100;
    settings.margin = 2;
    settings.table_overflow = TableOverflow::None;
    settings.table_border = TableBorderStyle::None;
    settings.runtime = RenderRuntime {
        command_scope: Default::default(),
        stdout_is_tty: false,
        terminal: Some("dumb".into()),
        no_color: true,
        width: Some(100),
        locale_utf8: Some(true),
    };
    settings
}

#[test]
fn service_record_exports_preserve_nested_typed_data() {
    let record = json!({
        "uid": "alice",
        "enabled": true,
        "quota_gib": 42.5,
        "roles": ["operator", "reviewer", "dns", "directory", "vm", "audit", "backup"],
        "contacts": {
            "email": "alice@example.com",
            "routes": [
                {"kind": "primary", "address": "192.0.2.8"},
                {"kind": "backup", "address": "2001:db8::8"}
            ]
        },
        "approval": {
            "decision": "approved",
            "evidence": [
                {"reviewer": "bob", "sources": ["ticket", "review"]},
                ["deploy", "audit"],
                true
            ]
        },
        "labels": [["primary", "prod"], ["backup", "stage"]]
    });
    let output =
        OutputResult::from_rows(Vec::new()).with_document(OutputDocument::Json(record.clone()));

    let json_export = render(
        &output,
        &crate::ui::RenderRequest::new(&export_settings(OutputFormat::Json)),
    );
    assert_eq!(serde_json::from_str::<Value>(&json_export).unwrap(), record);

    let mreg = render(
        &output,
        &crate::ui::RenderRequest::new(&export_settings(OutputFormat::Mreg)),
    );
    for expected in [
        "uid:",
        "alice",
        "enabled:",
        "true",
        "42.5",
        "contacts:",
        "email:",
        "alice@example.com",
        "routes (2):",
        "192.0.2.8",
        "2001:db8::8",
        "approved",
        "bob",
        "ticket",
        "review",
        "deploy",
        "audit",
        "prod",
        "stage",
        "operator",
        "reviewer",
        "dns",
        "directory",
        "vm",
        "backup",
    ] {
        assert!(
            mreg.contains(expected),
            "missing {expected:?} in Mreg export:\n{mreg}"
        );
    }

    let markdown = render(
        &output,
        &crate::ui::RenderRequest::new(&export_settings(OutputFormat::Markdown)),
    );
    for expected in [
        "- uid: alice",
        "- enabled: true",
        "- quota_gib: 42.5",
        "- contacts:",
        "- email: alice@example.com",
        "- routes (2):",
        "- address: 192.0.2.8",
        "- address: 2001:db8::8",
        "- evidence (3):",
        "- reviewer: bob",
        "- ticket",
        "- review",
        "- deploy",
        "- audit",
        "- primary",
        "- prod",
        "- stage",
    ] {
        assert!(
            markdown.contains(expected),
            "missing {expected:?} in Markdown export:\n{markdown}"
        );
    }
    assert_eq!(output.document.unwrap().json_value(), record);
}

#[test]
fn grouped_report_exports_restore_aggregates_and_member_rows() {
    let members = [
        json!({"region": "eu|west", "uid": "alice|operator", "quota": 1.5, "roles": ["dns", "vm"]}),
        json!({"region": "eu|west", "uid": "bob", "quota": 2.0, "roles": ["review"]}),
        json!({"region": "us", "uid": "carol", "quota": 4.0, "roles": ["audit"]}),
    ];
    let rows = members
        .iter()
        .map(|member| member.as_object().unwrap().clone())
        .collect();
    let pipeline = parse_pipeline("service-report | G region | A sum(quota) AS total | S region")
        .expect("report aggregation should parse");
    let grouped = apply_output_pipeline(OutputResult::from_rows(rows), &pipeline.stages)
        .expect("report aggregation should execute");
    let canonical = json!([
        {
            "groups": {"region": "eu|west"},
            "aggregates": {"total": 3.5},
            "rows": [members[0], members[1]]
        },
        {
            "groups": {"region": "us"},
            "aggregates": {"total": 4.0},
            "rows": [members[2]]
        }
    ]);
    let exported = render(
        &grouped,
        &crate::ui::RenderRequest::new(&export_settings(OutputFormat::Json)),
    );
    let restored_value: Value = serde_json::from_str(&exported).unwrap();
    assert_eq!(restored_value, canonical);

    let restored = OutputResult {
        items: output_items_from_value(restored_value),
        document: None,
        meta: OutputMeta {
            grouped: true,
            ..Default::default()
        },
    };
    assert_eq!(restored.items, grouped.items);
    assert_eq!(
        serde_json::from_str::<Value>(&render(
            &restored,
            &crate::ui::RenderRequest::new(&export_settings(OutputFormat::Json))
        ),)
        .unwrap(),
        canonical
    );

    let markdown = render(
        &restored,
        &crate::ui::RenderRequest::new(&export_settings(OutputFormat::Markdown)),
    );
    for expected in [
        "- region: eu|west",
        "- total: 3.5",
        "- region: us",
        "- total: 4",
        "eu\\|west",
        "alice\\|operator",
        "bob",
        "carol",
        "dns, vm",
        "review",
        "audit",
    ] {
        assert!(
            markdown.contains(expected),
            "missing {expected:?} in grouped Markdown:\n{markdown}"
        );
    }
    let mreg = render(
        &restored,
        &crate::ui::RenderRequest::new(&export_settings(OutputFormat::Mreg)),
    );
    for expected in [
        "region:",
        "eu|west",
        "us",
        "total:",
        "3.5",
        "4",
        "alice|operator",
        "bob",
        "carol",
        "dns, vm",
    ] {
        assert!(
            mreg.contains(expected),
            "missing {expected:?} in grouped Mreg:\n{mreg}"
        );
    }
}

#[test]
fn operational_guide_exports_keep_authored_entries_and_nested_policy_data() {
    let guide = Document {
        blocks: vec![
            DocBlock::section(
                "Usage",
                vec![DocBlock::Paragraph {
                    text: ("osp site host apply HOST [OPTIONS]").into(),
                }],
            ),
            DocBlock::section(
                "Operations",
                vec![
                    DocBlock::Paragraph {
                        text: ("Use `site host show` after **approval**.").into(),
                    },
                    DocBlock::Entries {
                        entries: vec![DocumentEntry {
                            role: Default::default(),
                            name: ("site host show").into(),
                            description: ("Read the `canonical` host record").into(),
                        }],
                    },
                    DocBlock::Entries {
                        entries: vec![DocumentEntry {
                            role: Default::default(),
                            name: ("site host apply").into(),
                            description: ("Apply an approved change").into(),
                        }],
                    },
                    DocBlock::Entries {
                        entries: vec![DocumentEntry {
                            role: Default::default(),
                            name: ("site whoami").into(),
                            description: ("").into(),
                        }],
                    },
                    DocBlock::Entries {
                        entries: vec![DocumentEntry {
                            role: Default::default(),
                            name: ("site host show | P uid").into(),
                            description: ("Copy the `uid` field").into(),
                        }],
                    },
                ],
            ),
            DocBlock::section(
                "Arguments",
                vec![DocBlock::Entries {
                    entries: vec![DocumentEntry {
                        role: Default::default(),
                        name: ("HOST").into(),
                        description: ("Canonical host name").into(),
                    }],
                }],
            ),
            DocBlock::section(
                "Options",
                vec![DocBlock::Entries {
                    entries: vec![DocumentEntry {
                        role: Default::default(),
                        name: ("--dry-run").into(),
                        description: ("Preview the approved change").into(),
                    }],
                }],
            ),
            DocBlock::section(
                "Common Invocation Options",
                vec![
                    DocBlock::Entries {
                        entries: vec![DocumentEntry {
                            role: Default::default(),
                            name: ("--profile").into(),
                            description: ("Select the service profile").into(),
                        }],
                    },
                    DocBlock::Entries {
                        entries: vec![DocumentEntry {
                            role: Default::default(),
                            name: ("--json").into(),
                            description: ("Export typed results").into(),
                        }],
                    },
                ],
            ),
            DocBlock::section(
                "Policy",
                vec![DocBlock::Data {
                    value: json!({
                        "owner": "alice",
                        "enabled": true,
                        "reviewers": ["bob", "carol"],
                        "service": {
                            "name": "directory",
                            "checks": [
                                {"name": "identity", "required": true},
                                ["dns", "network"]
                            ]
                        }
                    }),
                }],
            ),
        ],
    };
    let output = guide.to_output_result();
    let exported = render(
        &output,
        &crate::ui::RenderRequest::new(&export_settings(OutputFormat::Json)),
    );
    assert_eq!(
        serde_json::from_str::<Value>(&exported).unwrap(),
        serde_json::to_value(&guide).unwrap()
    );

    let restored: Document = serde_json::from_str(&exported).unwrap();
    assert_eq!(restored, guide);
    assert_eq!(
        restored.rows()[0]["text"],
        "osp site host apply HOST [OPTIONS]"
    );

    let reference = parse_pipeline("runbook | F name ~ ^--").unwrap();
    let flags = apply_output_pipeline(restored.to_output_result(), &reference.stages).unwrap();
    assert!(flags.meta.document_rows);
    assert!(flags.document.is_none());
    assert_eq!(
        flags
            .as_rows()
            .unwrap()
            .iter()
            .map(|row| row["name"].as_str().unwrap())
            .collect::<Vec<_>>(),
        ["--dry-run", "--profile", "--json"]
    );
    let flag_markdown = render(
        &flags,
        &crate::ui::RenderRequest::new(&export_settings(OutputFormat::Markdown)),
    );
    for flag in ["--dry-run", "--profile", "--json"] {
        assert!(flag_markdown.contains(flag), "{flag_markdown}");
    }

    let terminal = render(
        &output,
        &crate::ui::RenderRequest::new(&export_settings(OutputFormat::Auto)),
    );
    let visible_words = terminal.split_whitespace().collect::<Vec<_>>().join(" ");
    for expected in [
        "Operations",
        "Use site host show after approval.",
        "site host show Read the canonical host record",
        "site host apply Apply an approved change",
        "site whoami",
        "site host show | P uid Copy the uid field",
        "Policy",
        "alice",
        "true",
        "bob",
        "carol",
        "directory",
        "identity",
        "dns",
        "network",
    ] {
        assert!(
            visible_words.contains(expected),
            "missing {expected:?} in terminal guide:\n{terminal}"
        );
    }
    let markdown = render(
        &output,
        &crate::ui::RenderRequest::new(&export_settings(OutputFormat::Markdown)),
    );
    for expected in [
        "Operations",
        "Use `site host show` after **approval**.",
        "- `site host show` Read the `canonical` host record",
        "- `site host apply` Apply an approved change",
        "- `site whoami`",
        "- `site host show | P uid` Copy the `uid` field",
        "- owner: alice",
        "- enabled: true",
        "- reviewers (2):",
        "- bob",
        "- carol",
        "- name: directory",
        "- name: identity",
        "- dns",
        "- network",
    ] {
        assert!(
            markdown.contains(expected),
            "missing {expected:?} in guide Markdown:\n{markdown}"
        );
    }
    let mreg = render(
        &output,
        &crate::ui::RenderRequest::new(&export_settings(OutputFormat::Mreg)),
    );
    for expected in [
        "Operations",
        "site host show",
        "site host apply",
        "Policy",
        "alice",
        "bob",
        "carol",
        "directory",
        "identity",
        "dns",
        "network",
    ] {
        assert!(
            mreg.contains(expected),
            "missing {expected:?} in guide Mreg:\n{mreg}"
        );
    }
    assert_eq!(
        serde_json::from_value::<Document>(output.document.as_ref().unwrap().json_value()).unwrap(),
        guide
    );
}
