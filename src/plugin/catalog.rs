//! Shared recursive authorization metadata collection.
use std::collections::BTreeMap;
use crate::core::command_policy::CommandPath;
use crate::core::plugin::{DescribeCommandV1, DescribeCommandAuthV1};

pub(crate) fn collect_command_auth(
    registry: &mut BTreeMap<CommandPath, DescribeCommandAuthV1>,
    command: &DescribeCommandV1,
    prefix: &[String],
) {
    collect_command_auth_with_parent(registry, command, prefix, None);
}

fn collect_command_auth_with_parent(
    registry: &mut BTreeMap<CommandPath, DescribeCommandAuthV1>,
    command: &DescribeCommandV1,
    prefix: &[String],
    parent_auth: Option<&DescribeCommandAuthV1>,
) {
    let mut segments = prefix.to_vec();
    segments.push(command.name.clone());
    let path = CommandPath::new(segments.clone());
    let auth = command.auth.as_ref().or(parent_auth);
    if let Some(auth) = auth {
        registry.insert(path, auth.clone());
    }
    for subcommand in &command.subcommands {
        collect_command_auth_with_parent(registry, subcommand, &segments, auth);
    }
}
