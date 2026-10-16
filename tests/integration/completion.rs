use osp_cli::completion::{
    ArgNode, CompletionEngine, CompletionNode, CompletionTreeBuilder, ContextScope,
    DynamicCompletion, FlagCompletions, FlagNode, SuggestionEntry, SuggestionOutput,
};
use std::collections::BTreeMap;

fn suggestion_values(outputs: Vec<SuggestionOutput>) -> Vec<String> {
    outputs
        .into_iter()
        .filter_map(|entry| match entry {
            SuggestionOutput::Item(item) => Some(item.text),
            SuggestionOutput::PathSentinel => None,
        })
        .collect()
}

fn provider_cursor(line: &str) -> usize {
    line.find("--provider").expect("provider flag in test line") - 1
}

fn completion_tree(context_scope: ContextScope) -> osp_cli::completion::CompletionTree {
    let mut tree = CompletionTreeBuilder
        .build_from_specs(
            &[
                CompletionNode::new("service").subcommand(CompletionNode::new("deploy").flag(
                    "--image",
                    FlagNode {
                        suggestions: vec![
                            SuggestionEntry::from("red"),
                            SuggestionEntry::from("blue"),
                        ],
                        ..FlagNode::default()
                    },
                )),
                CompletionNode::new("hidden").flag(
                    "--provider",
                    FlagNode {
                        suggestions: vec![
                            SuggestionEntry::from("alpha"),
                            SuggestionEntry::from("beta"),
                        ],
                        context_only: true,
                        context_scope,
                        ..FlagNode::default()
                    },
                ),
            ],
            [],
        )
        .expect("completion tree should build");
    tree.root
        .children
        .get_mut("service")
        .unwrap()
        .children
        .get_mut("deploy")
        .unwrap()
        .dynamic = Some(DynamicCompletion::new(|cmd| {
        let values = match cmd
            .flag_values("--provider")
            .and_then(|values| values.first())
            .map(String::as_str)
        {
            Some("alpha") => vec![SuggestionEntry::from("red")],
            Some("beta") => vec![SuggestionEntry::from("blue")],
            _ => vec![SuggestionEntry::from("red"), SuggestionEntry::from("blue")],
        };
        FlagCompletions {
            values: BTreeMap::from([("--image".into(), values)]),
            ..FlagCompletions::default()
        }
    }));
    tree
}

#[test]
fn completion_engine_merges_global_context_flags_from_later_tokens() {
    let catalogue = osp_cli::completion::model::ValueCatalog::default();
    let mut tree = completion_tree(ContextScope::Global);
    tree.root
        .children
        .get_mut("service")
        .unwrap()
        .children
        .get_mut("deploy")
        .unwrap()
        .flags
        .insert(
            "--hostname".into(),
            FlagNode {
                catalogs: vec![catalogue.clone()],
                ..FlagNode::default()
            },
        );
    let engine = CompletionEngine::new(tree.clone());
    let line = "service deploy --image  --provider alpha";
    let cursor = provider_cursor(line);

    let (_, suggestions) = engine.complete(line, cursor);
    let values = suggestion_values(suggestions);
    assert!(values.contains(&"red".to_string()));
    assert!(!values.contains(&"blue".to_string()));

    let analysis = engine.analyze(line, cursor);
    assert_eq!(analysis.context.matched_path, vec!["service", "deploy"]);
    assert_eq!(analysis.context.flag_scope_path, vec!["service", "deploy"]);
    assert_eq!(
        analysis
            .parsed
            .cursor_cmd
            .flag_values("--provider")
            .expect("provider should merge into cursor context"),
        &vec!["alpha".to_string()][..]
    );

    // A refreshed catalogue reaches an existing engine through its cloned tree.
    catalogue.replace(
        (0..30)
            .rev()
            .map(|index| format!("node-{index:02}"))
            .chain(["node-00".into(), "other-host".into()])
            .collect(),
    );
    let line = "service deploy --hostname node-";
    assert_eq!(
        suggestion_values(engine.complete(line, line.len()).1),
        (0..25)
            .map(|index| format!("node-{index:02}"))
            .collect::<Vec<_>>()
    );
    assert!(catalogue.contains("node-29"));
    catalogue.replace(vec!["node-refreshed".into()]);
    assert_eq!(
        suggestion_values(engine.complete(line, line.len()).1),
        vec!["node-refreshed"]
    );
}

#[test]
fn completion_engine_keeps_subtree_context_flags_outside_matched_scope() {
    let engine = CompletionEngine::new(completion_tree(ContextScope::Subtree));
    let line = "service deploy --image  --provider alpha";

    let (_, suggestions) = engine.complete(line, provider_cursor(line));
    let values = suggestion_values(suggestions);
    assert!(values.contains(&"red".to_string()));
    assert!(values.contains(&"blue".to_string()));
}

#[test]
fn completion_engine_applies_dynamic_positional_slot_overrides() {
    for inherited_flags in [false, true] {
        let mut deploy = CompletionNode::new("deploy")
            .arg(ArgNode::named("scope").suggestions([
                SuggestionEntry::from("local"),
                SuggestionEntry::from("empty"),
            ]))
            .arg(ArgNode::named("image").suggestions([SuggestionEntry::from("static-image")]))
            .arg(
                ArgNode::named("tags")
                    .multi()
                    .suggestions([SuggestionEntry::from("static-tag")]),
            );
        deploy.dynamic = Some(DynamicCompletion::new(|cmd| {
            let suppress = cmd.head().iter().any(|value| value == "empty");
            FlagCompletions {
                positionals: BTreeMap::from([
                    (
                        1,
                        if suppress {
                            Vec::new()
                        } else {
                            vec![SuggestionEntry::from("image value")]
                        },
                    ),
                    (
                        2,
                        if suppress {
                            Vec::new()
                        } else {
                            vec![
                                SuggestionEntry::from("tag-one"),
                                SuggestionEntry::from("tag-two"),
                            ]
                        },
                    ),
                ]),
                ..FlagCompletions::default()
            }
        }));
        let mut service = CompletionNode::new("service");
        if inherited_flags {
            service = service.flag("--switch", FlagNode::new().flag_only());
        } else {
            deploy = deploy.flag("--switch", FlagNode::new().flag_only());
        }
        let tree = CompletionTreeBuilder
            .build_from_specs(&[service.subcommand(deploy)], [])
            .expect("completion tree should build");
        let engine = CompletionEngine::new(tree);

        for (line, expected) in [
            ("service deploy l", vec!["local"]),
            ("service deploy local i", vec!["image value"]),
            ("service deploy local \"im", vec!["image value"]),
            ("service deploy empty i", vec![]),
            (
                "service deploy local \"image value\" t",
                vec!["tag-one", "tag-two"],
            ),
            (
                "service deploy local \"image value\" tag-one t",
                vec!["tag-one", "tag-two"],
            ),
            ("service deploy empty \"image value\" t", vec![]),
        ] {
            assert_eq!(
                suggestion_values(engine.complete(line, line.len()).1),
                expected,
                "{line}, inherited flags: {inherited_flags}",
            );
        }
        let line = "service deploy local im --switch";
        assert_eq!(
            suggestion_values(engine.complete(line, line.find(" --switch").unwrap()).1),
            vec!["image value"],
        );
    }
}
