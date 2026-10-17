//! Filesystem candidates shared by command slots and standalone file prompts.

use std::path::PathBuf;

/// Returns matching paths and whether each is a directory. Directory values end
/// in `/`; `~` is expanded for lookup and preserved in the inserted value.
/// Hidden entries appear only when the current segment starts with `.`.
pub fn path_candidates(stub: &str) -> Vec<(String, bool)> {
    let (lookup, prefix, typed) = split_path_stub(stub);
    let Ok(entries) = std::fs::read_dir(lookup) else {
        return Vec::new();
    };
    let mut candidates = entries
        .flatten()
        .filter_map(|entry| {
            let name = entry.file_name().to_string_lossy().into_owned();
            if !name.starts_with(&typed) || (name.starts_with('.') && !typed.starts_with('.')) {
                return None;
            }
            let directory = entry.path().is_dir();
            Some((
                format!("{prefix}{name}{}", if directory { "/" } else { "" }),
                directory,
            ))
        })
        .collect::<Vec<_>>();
    candidates.sort();
    candidates
}

pub(crate) fn split_path_stub(stub: &str) -> (PathBuf, String, String) {
    let (prefix, typed) = stub
        .rfind('/')
        .map_or(("", stub), |index| (&stub[..=index], &stub[index + 1..]));
    let directory = if prefix.is_empty() { "." } else { prefix };
    let lookup =
        crate::normalize::expand_home(directory).unwrap_or_else(|| PathBuf::from(directory));
    (lookup, prefix.to_owned(), typed.to_owned())
}
