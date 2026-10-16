use std::collections::BTreeMap;

use super::{CommandLine, FlagOccurrence, TailItem};

#[test]
fn command_line_and_node_data_structs_preserve_internal_state_unit() {
    let mut command = CommandLine::default();
    command.push_head("service");
    command.push_head("deploy");
    command.push_flag_occurrence(FlagOccurrence {
        name: "--provider".to_string(),
        values: vec!["alpha".to_string()],
    });
    command.push_positional("guest01");
    command.merge_flag_values("--image", vec!["red".to_string()]);
    command.prepend_positional_values(["".to_string(), "tenant-a".to_string(), " ".to_string()]);
    command.set_pipe(vec!["V ip".to_string()]);

    assert_eq!(
        command.head(),
        &["service".to_string(), "deploy".to_string()]
    );
    assert!(command.has_flag("--provider"));
    assert_eq!(
        command.flag_values("--provider"),
        Some(&["alpha".to_string()][..])
    );
    assert_eq!(
        command.flag_values_map()["--image"],
        vec!["red".to_string()]
    );
    assert_eq!(
        command
            .last_flag_occurrence()
            .map(|flag| flag.name.as_str()),
        Some("--provider")
    );
    assert_eq!(
        command.positional_args().cloned().collect::<Vec<_>>(),
        vec!["tenant-a".to_string(), "guest01".to_string()]
    );
    assert_eq!(command.tail_len(), 3);
    assert!(matches!(command.tail()[0], TailItem::Positional(_)));
    assert_eq!(command.pipes(), &["V ip".to_string()]);
    assert!(command.has_pipe());

    let mut node = super::CompletionNode::default();
    node.prefilled_flags
        .insert("--provider".to_string(), vec!["alpha".to_string()]);
    node.prefilled_positionals = vec!["tenant-a".to_string()];
    node.flag_hints = Some(Default::default());

    assert_eq!(
        node.prefilled_flags,
        BTreeMap::from([("--provider".to_string(), vec!["alpha".to_string()])])
    );
    assert_eq!(node.prefilled_positionals, vec!["tenant-a".to_string()]);
    assert!(node.flag_hints.is_some());
}

#[test]
fn internal_example_1() {
    use crate::completion::model::MatchKind;

    let labels = [
        MatchKind::Pipe,
        MatchKind::Flag,
        MatchKind::Command,
        MatchKind::Subcommand,
        MatchKind::Value,
    ]
    .into_iter()
    .map(MatchKind::as_str)
    .collect::<Vec<_>>();

    assert_eq!(
        labels,
        vec!["pipe", "flag", "command", "subcommand", "value"]
    );
}
