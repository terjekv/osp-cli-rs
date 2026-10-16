//! Shared reads of clap metadata for completion and the plugin wire boundary.

pub(crate) fn plain(value: Option<&clap::builder::StyledStr>) -> Option<String> {
    value
        .map(ToString::to_string)
        .map(|text| text.trim().to_string())
        .filter(|text| !text.is_empty())
}

pub(crate) fn arg_help(arg: &clap::Arg) -> Option<String> {
    plain(arg.get_long_help().or_else(|| arg.get_help()))
}

pub(crate) fn arg_name(arg: &clap::Arg) -> String {
    arg.get_value_names()
        .and_then(|names| names.first())
        .map(ToString::to_string)
        .unwrap_or_else(|| arg.get_id().to_string())
}

pub(crate) fn is_multi(arg: &clap::Arg) -> bool {
    arg.get_num_args()
        .is_some_and(|range| range.min_values() > 1 || range.max_values() > 1)
        || matches!(arg.get_action(), clap::ArgAction::Append)
}

pub(crate) fn is_path(arg: &clap::Arg) -> bool {
    matches!(
        arg.get_value_hint(),
        clap::ValueHint::AnyPath
            | clap::ValueHint::FilePath
            | clap::ValueHint::DirPath
            | clap::ValueHint::ExecutablePath
    )
}

pub(crate) fn flag_spellings(arg: &clap::Arg) -> Vec<String> {
    arg.get_long()
        .map(|name| format!("--{name}"))
        .into_iter()
        .chain(arg.get_short().map(|name| format!("-{name}")))
        .chain(
            arg.get_long_and_visible_aliases()
                .into_iter()
                .flatten()
                .filter(|alias| Some(*alias) != arg.get_long())
                .map(|name| format!("--{name}")),
        )
        .chain(
            arg.get_short_and_visible_aliases()
                .into_iter()
                .flatten()
                .filter(|alias| Some(*alias) != arg.get_short())
                .map(|name| format!("-{name}")),
        )
        .collect()
}
