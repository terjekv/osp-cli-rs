#[test]
fn describe_command_helpers_preserve_nested_completion_metadata_unit() {
    let suggestion = DescribeSuggestionV1 {
        value: "json".to_string(),
        meta: Some("format".to_string()),
        display: Some("JSON".to_string()),
        sort: Some("01".to_string()),
    };
    let command = DescribeCommandV1 {
        name: "ldap".to_string(),
        about: "lookup users".to_string(),
        auth: None,
        args: vec![DescribeArgV1 {
            name: Some("uid".to_string()),
            about: Some("user id".to_string()),
            required: false,
            multi: true,
            value_type: Some(crate::core::plugin::DescribeValueTypeV1::Path),
            suggestions: vec![suggestion.clone()],
        }],
        flags: std::collections::BTreeMap::from([(
            "--format".to_string(),
            DescribeFlagV1 {
                alias_of: None,
                about: Some("output format".to_string()),
                required: true,
                flag_only: false,
                multi: true,
                value_type: Some(crate::core::plugin::DescribeValueTypeV1::Path),
                suggestions: vec![suggestion.clone()],
            },
        )]),
        subcommands: vec![DescribeCommandV1 {
            name: "user".to_string(),
            about: String::new(),
            auth: None,
            args: Vec::new(),
            flags: Default::default(),
            subcommands: Vec::new(),
        }],
    };

    let spec = crate::completion::CompletionNode::from_describe(&command);
    assert_eq!(spec.name, "ldap");
    assert_eq!(spec.tooltip.as_deref(), Some("lookup users"));
    assert_eq!(spec.children.keys().cloned().collect::<Vec<_>>(), vec!["user".to_string()]);
    assert_eq!(
        spec.flag_hints
            .as_ref()
            .expect("required plugin flags should survive conversion")
            .required_common,
        ["--format"]
    );

    let arg = &spec.args[0];
    assert_eq!(arg.name.as_deref(), Some("uid"));
    assert_eq!(arg.tooltip.as_deref(), Some("user id"));
    assert!(arg.multi);
    assert_eq!(arg.value_type, Some(crate::completion::ValueType::Path));

    let flag = &spec.flags["--format"];
    assert_eq!(flag.tooltip.as_deref(), Some("output format"));
    assert!(flag.multi);
    assert_eq!(flag.value_type, Some(crate::completion::ValueType::Path));

    let entry = &flag.suggestions[0];
    assert_eq!(entry.value, "json");
    assert_eq!(entry.display.as_deref(), Some("JSON"));

}
