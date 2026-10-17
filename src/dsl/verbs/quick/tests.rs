use serde_json::json;

use super::{
    apply_groups_with_plan, apply_value, apply_value_with_plan, apply_with_plan, compile,
    value_matches_token,
};
use crate::core::{output_model::Group, row::Row};
use crate::dsl::parse::key_spec::ExactMode;

fn row(value: serde_json::Value) -> Row {
    value
        .as_object()
        .cloned()
        .expect("fixture should be an object")
}

#[test]
fn compile_classifies_structural_selectors_and_rejects_invalid_fuzzy_forms_unit() {
    let plan = compile("!sections[0].entries[0]").expect("quick should compile");
    assert!(plan.spec.is_structural());

    assert!(compile("   ").is_err());
    assert!(compile("% ?uid").is_err());
    assert!(compile("% ==uid").is_err());
    assert!(compile("% sections[0].uid").is_err());
}

#[test]
fn quick_search_keeps_leading_scope_letters_in_ordinary_tokens_unit() {
    let provider_rows = vec![
        row(json!({"provider": "vmware"})),
        row(json!({"provider": "mware"})),
    ];
    assert_eq!(
        apply_with_plan(provider_rows.clone(), &compile("vmware").unwrap()).unwrap(),
        vec![row(json!({"provider": "vmware"}))]
    );
    assert_eq!(
        apply_with_plan(provider_rows, &compile("=vmware").unwrap()).unwrap(),
        vec![row(json!({"provider": "vmware"}))]
    );

    let key_rows = vec![row(json!({"kind": "vm"})), row(json!({"ind": "vm"}))];
    assert_eq!(
        apply_with_plan(key_rows, &compile("kind").unwrap()).unwrap(),
        vec![row(json!({"kind": "vm"}))]
    );
}

#[test]
fn quick_family_narrows_single_rows_and_filters_multiple_rows_unit() {
    let matching = row(json!({
        "uid": "alice",
        "roles": ["eng", "ops"],
        "city": "Oslo"
    }));
    let other = row(json!({"name": "bob", "team": "eng"}));

    for (spec, expected) in [
        ("ops", row(json!({"roles": ["ops"]}))),
        ("V ops", row(json!({"roles": ["ops"]}))),
        ("K uid", row(json!({"uid": "alice"}))),
        ("?uid", matching.clone()),
    ] {
        assert_eq!(
            apply_with_plan(vec![matching.clone()], &compile(spec).unwrap()).unwrap(),
            vec![expected],
            "{spec}"
        );
    }
    assert_eq!(
        apply_with_plan(
            vec![matching.clone(), other.clone()],
            &compile("ops").unwrap()
        )
        .unwrap(),
        vec![matching.clone()]
    );
    assert_eq!(
        apply_with_plan(vec![matching.clone()], &compile("! ops").unwrap()).unwrap(),
        vec![row(
            json!({"uid": "alice", "roles": ["eng"], "city": "Oslo"})
        )]
    );
    assert_eq!(
        apply_with_plan(vec![other.clone()], &compile("! ops").unwrap()).unwrap(),
        vec![other]
    );

    let value_matches_key = row(json!({"status": "running", "note": "status"}));
    assert_eq!(
        apply_with_plan(vec![value_matches_key], &compile("V status").unwrap()).unwrap(),
        vec![row(json!({"note": "status"}))]
    );

    // Like osprov-cli: on one object, keep fields whose names match and, in
    // every other field, only the values and list items that contain the text.
    let account = row(json!({
        "uid": "oistes",
        "uioAffiliation": "ANSATT@373034",
        "netgroups": ["it-uio-azure-users", "los-alle", "mattermost-uio", "usit"],
        "filegroups": ["oistes", "ucore"],
        "dn": "uid=oistes,cn=users,dc=uio,dc=no",
    }));
    assert_eq!(
        apply_with_plan(vec![account.clone()], &compile("uio").unwrap()).unwrap(),
        vec![row(json!({
            "uioAffiliation": "ANSATT@373034",
            "netgroups": ["it-uio-azure-users", "mattermost-uio"],
            "dn": "uid=oistes,cn=users,dc=uio,dc=no",
        }))]
    );
    assert_eq!(
        apply_with_plan(vec![account], &compile("uid").unwrap()).unwrap(),
        vec![row(json!({"uid": "oistes"}))]
    );
    for source in [
        row(json!({"metadata": {"owner": "Alice"}})),
        row(json!({"members": [{"uid": "alice"}]})),
    ] {
        assert_eq!(
            apply_with_plan(vec![source.clone()], &compile("V Alice").unwrap()).unwrap(),
            vec![source]
        );
    }

    let groups = vec![Group {
        groups: row(json!({"team": "ops"})),
        aggregates: row(json!({"count": 1})),
        rows: vec![matching.clone()],
    }];
    let filtered =
        apply_groups_with_plan(groups, &compile("uid").unwrap()).expect("group quick should work");
    assert_eq!(filtered[0].rows, vec![row(json!({"uid": "alice"}))]);
}

#[test]
fn quick_scopes_and_modes_share_single_object_selection_unit() {
    let source = row(json!({
        "uioAffiliation": ["employee", "staff"],
        "netgroups": ["UIO", "uio-ans", "other"],
        "metadata": {"owner": "uio", "city": "Oslo"},
        "commands": [
            {"name": "doctor", "details": {"tags": ["uio", "other"]}},
            {"name": "config", "details": {"tags": ["settings"]}}
        ]
    }));
    let values = json!({
        "netgroups": ["UIO", "uio-ans"],
        "metadata": {"owner": "uio"},
        "commands": [{"name": "doctor", "details": {"tags": ["uio", "other"]}}]
    });
    for (query, expected) in [
        ("V uio", values.clone()),
        ("V % uio", values),
        (
            "V % docter",
            json!({"commands": [{"name": "doctor", "details": {"tags": ["uio", "other"]}}]}),
        ),
        ("K uio", json!({"uioAffiliation": ["employee", "staff"]})),
        ("K % uio", json!({"uioAffiliation": ["employee", "staff"]})),
        (
            "=uio",
            json!({
                "netgroups": ["UIO"], "metadata": {"owner": "uio"},
                "commands": [{"name": "doctor", "details": {"tags": ["uio", "other"]}}]
            }),
        ),
        (
            "==uio",
            json!({
                "metadata": {"owner": "uio"},
                "commands": [{"name": "doctor", "details": {"tags": ["uio", "other"]}}]
            }),
        ),
        (
            "!uio",
            json!({
                "netgroups": ["other"], "metadata": {"city": "Oslo"},
                "commands": [{"name": "config", "details": {"tags": ["settings"]}}]
            }),
        ),
        (
            "! V uio",
            json!({
                "uioAffiliation": ["employee", "staff"],
                "netgroups": ["other"], "metadata": {"city": "Oslo"},
                "commands": [{"name": "config", "details": {"tags": ["settings"]}}]
            }),
        ),
        (
            "!=uio",
            json!({
                "uioAffiliation": ["employee", "staff"],
                "netgroups": ["uio-ans", "other"], "metadata": {"city": "Oslo"},
                "commands": [{"name": "config", "details": {"tags": ["settings"]}}]
            }),
        ),
        (
            "=NETGROUPS",
            json!({"netgroups": ["UIO", "uio-ans", "other"]}),
        ),
        (
            "K !=netgroups",
            json!({
                "uioAffiliation": ["employee", "staff"],
                "metadata": {"owner": "uio", "city": "Oslo"},
                "commands": source["commands"].clone()
            }),
        ),
        ("?metadata.owner", serde_json::Value::Object(source.clone())),
        ("K metadata.owner", json!({"metadata": {"owner": "uio"}})),
        (
            "!metadata.owner",
            json!({
                "uioAffiliation": ["employee", "staff"],
                "netgroups": ["UIO", "uio-ans", "other"],
                "metadata": {"city": "Oslo"}, "commands": source["commands"].clone()
            }),
        ),
    ] {
        assert_eq!(
            apply_with_plan(vec![source.clone()], &compile(query).unwrap()).unwrap(),
            vec![row(expected)],
            "{query}"
        );
    }
    for query in ["V metadata", "V netgroups", "==NETGROUPS"] {
        assert!(
            apply_with_plan(vec![source.clone()], &compile(query).unwrap())
                .unwrap()
                .is_empty(),
            "{query}"
        );
    }

    let nested = row(json!({"name": {"first": "Alice"}, "provider": {"name": "service"}}));
    for query in ["name", "=NAME", "==name"] {
        assert_eq!(
            apply_with_plan(vec![nested.clone()], &compile(query).unwrap()).unwrap(),
            vec![row(json!({"name": {"first": "Alice"}}))],
            "{query}"
        );
    }
    assert_eq!(
        apply_with_plan(vec![nested.clone()], &compile("K % name").unwrap()).unwrap(),
        vec![nested]
    );

    let groups = vec![
        Group {
            groups: row(json!({"team": "ops"})),
            aggregates: Row::new(),
            rows: vec![source.clone()],
        },
        Group {
            groups: row(json!({"team": "eng"})),
            aggregates: Row::new(),
            rows: vec![source.clone(), row(json!({"uid": "other"}))],
        },
    ];
    let filtered = apply_groups_with_plan(groups.clone(), &compile("V uio").unwrap()).unwrap();
    assert_eq!(filtered[0].groups, groups[0].groups);
    assert_eq!(filtered[0].rows[0]["netgroups"], json!(["UIO", "uio-ans"]));
    assert_eq!(filtered[1].groups, groups[1].groups);
    assert_eq!(filtered[1].rows, vec![source]);
}

#[test]
fn dotted_visible_values_and_structural_paths_narrow_single_rows_unit() {
    let visible = row(json!({"key": "theme.name", "value": "dracula"}));
    assert_eq!(
        apply_with_plan(vec![visible.clone()], &compile("theme.name").unwrap()).unwrap(),
        vec![row(json!({"key": "theme.name"}))]
    );

    let source = row(json!({
        "meta": "people",
        "users": [
            {"uid": "alice", "team": "ops"},
            {"uid": "bob", "team": "eng"}
        ]
    }));
    assert_eq!(
        apply_with_plan(vec![source.clone()], &compile("users[1]").unwrap()).unwrap(),
        vec![row(json!({"uid": "bob", "team": "eng"}))]
    );
    assert_eq!(
        apply_with_plan(vec![source.clone()], &compile("! users[0].uid").unwrap()).unwrap(),
        vec![row(json!({
            "meta": "people",
            "users": [{"team": "ops"}, {"uid": "bob", "team": "eng"}]
        }))]
    );
    assert!(
        apply_with_plan(vec![source], &compile("users[9]").unwrap())
            .unwrap()
            .is_empty()
    );
}

#[test]
fn value_quick_narrows_matching_document_fields_and_selector_pairs_unit() {
    let source = json!({
        "commands": [
            {"name": "help", "short_help": "Show overview"},
            {"name": "doctor", "short_help": "Run diagnostics"}
        ]
    });
    assert_eq!(
        apply_value(source.clone(), "doctor").unwrap(),
        json!([{"commands": [{"name": "doctor", "short_help": "Run diagnostics"}]}])
    );

    let addressed = json!({
        "meta": "people",
        "users": [
            {"uid": "alice", "team": "ops"},
            {"uid": "bob", "team": "eng"}
        ]
    });
    assert_eq!(
        apply_value_with_plan(addressed.clone(), &compile("users[1]").unwrap()).unwrap(),
        json!([{"uid": "bob", "team": "eng"}])
    );
}

#[test]
fn quick_value_matching_handles_exact_fuzzy_and_array_values_unit() {
    assert!(value_matches_token(
        &json!(["OPS", "eng"]),
        "OPS",
        ExactMode::CaseSensitive,
        false
    ));
    assert!(value_matches_token(
        &json!(["OPS", "eng"]),
        "ops",
        ExactMode::CaseInsensitive,
        false
    ));
    assert!(value_matches_token(
        &json!(["operations"]),
        "oprtns",
        ExactMode::None,
        true
    ));
}
