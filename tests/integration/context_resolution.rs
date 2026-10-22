use clap::{Arg, Command};
use osp_cli::NativeSessionContext;
use osp_cli::context::{ContextContract, Fill, resolve};
use osp_cli::core::plugin::ResponseV1;
use serde_json::json;

fn args(values: &[&str]) -> Vec<String> {
    values.iter().map(|s| (*s).into()).collect()
}

#[test]
fn read_subject_uses_context_before_defaults_and_records_consumption() {
    let session = NativeSessionContext::default().for_service("inventory");
    session.set_context_value("host", "remembered.example");
    let command = Command::new("host")
        .arg(Arg::new("name").default_value("default.example"))
        .add(ContextContract::default().consumes("host", "name", Fill::Read));
    let resolved = resolve(command.clone(), &[], &session).unwrap();
    assert_eq!(resolved.args, args(&["remembered.example"]));
    let mut response = ResponseV1::ok(json!({"name":"remembered.example"}));
    resolved.finish(&mut response, &session);
    assert_eq!(
        response.meta.context.get("host").unwrap(),
        "remembered.example"
    );
    assert_eq!(
        response.messages[0].text,
        "host remembered.example (from context)"
    );

    let explicit = resolve(command, &args(&["explicit.example"]), &session).unwrap();
    assert_eq!(explicit.args, args(&["explicit.example"]));
    let mut response = ResponseV1::ok(json!({"name":"explicit.example"}));
    explicit.finish(&mut response, &session);
    assert!(response.meta.context.is_empty());
}

#[test]
fn explicit_marker_expands_only_the_declared_argument_in_nested_grammar() {
    let session = NativeSessionContext::default().for_service("inventory");
    session.set_context_value("host", "chosen.example");
    let command = Command::new("inventory").subcommand(
        Command::new("update")
            .arg(Arg::new("host").long("host").required(true))
            .arg(Arg::new("note").long("note"))
            .arg(
                Arg::new("verbose")
                    .short('v')
                    .action(clap::ArgAction::SetTrue),
            )
            .add(ContextContract::default().consumes("host", "--host", Fill::Explicit)),
    );
    let resolved = resolve(
        command,
        &args(&["update", "-v", "--note=.", "--host=."]),
        &session,
    )
    .unwrap();
    assert_eq!(
        resolved.args,
        args(&["update", "-v", "--note=.", "--host=chosen.example"])
    );
}

#[test]
fn singleton_response_selects_canonical_entities() {
    let session = NativeSessionContext::default().for_service("inventory");
    let command = Command::new("create").arg(Arg::new("name")).add(
        ContextContract::default()
            .produces("host", "/result/0/name")
            .produces("host", "/canonical")
            .produces("task", "/task")
            .produces("group", "$name"),
    );
    let resolved = resolve(command, &args(&["operators"]), &session).unwrap();
    let mut response = ResponseV1::ok(json!([{
        "result":[{"name":"canonical.example"}],
        "canonical":"canonical.example", "task":42
    }]));
    resolved.finish(&mut response, &session);
    assert_eq!(
        session.context_value("host").as_deref(),
        Some("canonical.example")
    );
    assert_eq!(session.context_value("task").as_deref(), Some("42"));
    assert_eq!(session.context_value("group").as_deref(), Some("operators"));
}

#[test]
fn declared_creation_context_survives_follow_up_failure() {
    let session = NativeSessionContext::default().for_service("inventory");
    let command = Command::new("create").add(ContextContract::default().produces("host", "/name"));
    let resolved = resolve(command, &[], &session).unwrap();
    let mut response = ResponseV1::ok(json!({"name":"unacknowledged.example"}));
    response.ok = false;
    response.context = Some([("host".into(), "created.example".into())].into());
    resolved.finish(&mut response, &session);
    assert_eq!(
        session.context_value("host").as_deref(),
        Some("created.example")
    );
}

#[test]
fn service_selections_share_hosts_and_keep_owned_entities_scoped() {
    let session = NativeSessionContext::default();
    let inventory = session.for_service("inventory");
    let jobs = session.for_service("jobs");
    inventory.set_context_value("host", "shared.example");
    inventory.set_context_value("task", "inventory-task");
    jobs.set_context_value("task", "jobs-task");
    assert_eq!(
        jobs.context_value("host").as_deref(),
        Some("shared.example")
    );
    assert_eq!(
        inventory.context_value("task").as_deref(),
        Some("inventory-task")
    );
    assert_eq!(jobs.context_value("task").as_deref(), Some("jobs-task"));
    assert!(
        session
            .prompt_entries()
            .iter()
            .any(|entry| entry.label == "inventory/task")
    );
    inventory.clear_service_context();
    assert_eq!(jobs.context_value("task").as_deref(), Some("jobs-task"));
    assert_eq!(session.context_entries().len(), 1);
}

#[test]
fn missing_read_entity_clears_the_consumed_selection() {
    let session = NativeSessionContext::default().for_service("inventory");
    session.set_context_value("host", "retired.example");
    let command = Command::new("read")
        .arg(Arg::new("host"))
        .add(ContextContract::default().consumes("host", "host", Fill::Read));
    let resolved = resolve(command, &[], &session).unwrap();
    let mut response = ResponseV1::ok(json!([]));
    response.meta.context_not_found = true;
    resolved.finish(&mut response, &session);
    assert_eq!(session.context_value("host"), None);
    assert!(
        response
            .messages
            .iter()
            .any(|message| message.text == "host retired.example no longer found; cleared context")
    );
}

#[test]
fn narrowed_empty_result_preserves_scope_and_offers_an_explicit_retry() {
    let session = NativeSessionContext::default().for_service("inventory");
    session.set_context_value("zone", "restricted-zone");
    let command = Command::new("read")
        .arg(Arg::new("host").required(true))
        .arg(Arg::new("zone").long("zone"))
        .add(ContextContract::default().consumes("zone", "--zone", Fill::Narrow));
    let resolved = resolve(command, &args(&["named.example"]), &session).unwrap();
    assert_eq!(
        resolved.args,
        args(&["named.example", "--zone", "restricted-zone"])
    );
    let mut response = ResponseV1::ok(json!([]));
    resolved.finish(&mut response, &session);
    assert_eq!(
        session.context_value("zone").as_deref(),
        Some("restricted-zone")
    );
    assert!(
        response
            .messages
            .iter()
            .any(|message| message.text == "hint: Rerun with --zone, or run context clear zone.")
    );
}
