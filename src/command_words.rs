//! Canonical command-word matching shared by execution and completion.

use std::collections::BTreeSet;

/// Execution policy for unique command prefixes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum CommandPrefixes {
    /// Require complete command words.
    Off,
    /// Accept prefixes inside the REPL.
    #[default]
    Interactive,
    /// Accept prefixes in both CLI and REPL invocations.
    Always,
}

impl CommandPrefixes {
    /// Reads the host's single prefix setting.
    pub fn from_config(config: &crate::config::ResolvedConfig) -> Self {
        match config.get_string("command.prefixes") {
            Some("off") => Self::Off,
            Some("always") => Self::Always,
            _ => Self::Interactive,
        }
    }

    /// Whether prefixes are executable in the current invocation mode.
    pub fn enabled(self, interactive: bool) -> bool {
        self == Self::Always || (self == Self::Interactive && interactive)
    }
}

/// Resolves an exact command word or its unique prefix among canonical names.
/// Unknown words return `None`; ambiguity returns all matching canonical names.
pub fn resolve<'a>(
    word: &str,
    names: impl IntoIterator<Item = &'a str>,
) -> Result<Option<String>, Vec<String>> {
    let names = names.into_iter().collect::<BTreeSet<_>>();
    if names.contains(word) {
        return Ok(Some(word.to_owned()));
    }
    let matches = names
        .into_iter()
        .filter(|name| !word.is_empty() && name.starts_with(word))
        .map(str::to_owned)
        .collect::<Vec<_>>();
    match matches.as_slice() {
        [] => Ok(None),
        [name] => Ok(Some(name.clone())),
        _ => Err(matches),
    }
}

/// Returns the shortest prefix that resolves to `name`, including exact precedence.
pub fn shortest_prefix(name: &str, names: &[&str]) -> usize {
    name.char_indices()
        .map(|(i, c)| i + c.len_utf8())
        .find(|&end| {
            resolve(&name[..end], names.iter().copied())
                .ok()
                .flatten()
                .as_deref()
                == Some(name)
        })
        .unwrap_or(name.len())
}

/// Canonicalizes only command words in a clap grammar, leaving argument values intact.
/// Excluded root commands must be spelled literally (for example authentication controls).
pub fn normalize(
    grammar: &clap::Command,
    args: &[String],
    enabled: bool,
    excluded: &[&str],
) -> anyhow::Result<Vec<String>> {
    let mut result = args.to_vec();
    let mut selected = grammar;
    let mut path = vec![grammar.get_name().to_owned()];
    let mut correction = false;
    let mut disabled_prefix = false;
    for (index, word) in args.iter().enumerate() {
        if word.starts_with('-') || !selected.has_subcommands() {
            break;
        }
        let names = selected
            .get_subcommands()
            .filter(|child| !child.is_hide_set())
            .map(clap::Command::get_name)
            .collect::<Vec<_>>();
        // Explicit clap aliases retain their existing meaning, but are never suggested.
        if let Some(child) = selected.find_subcommand(word) {
            result[index] = child.get_name().to_owned();
            path.push(child.get_name().to_owned());
            selected = child;
            continue;
        }
        let candidates = names
            .iter()
            .copied()
            .filter(|name| index != 0 || !excluded.contains(name))
            .collect::<Vec<_>>();
        let canonical = match resolve(word, candidates.iter().copied()) {
            Ok(Some(name)) => {
                disabled_prefix |= !enabled;
                name
            }
            Err(matches) => anyhow::bail!(
                "Ambiguous command: {}",
                matches
                    .into_iter()
                    .map(|name| format!("{} {name}", path.join(" ")))
                    .collect::<Vec<_>>()
                    .join(", ")
            ),
            Ok(None) => {
                let distance = if word.chars().count() >= 4 { 2 } else { 1 };
                let Some((_, name)) = candidates
                    .iter()
                    .map(|name| (strsim::levenshtein(word, name), name))
                    .filter(|(score, _)| *score <= distance)
                    .min()
                else {
                    break;
                };
                correction = true;
                (*name).to_owned()
            }
        };
        result[index] = canonical.clone();
        path.push(canonical.clone());
        let Some(child) = selected.find_subcommand(&canonical) else {
            break;
        };
        selected = child;
    }
    if correction {
        anyhow::bail!("Did you mean `{}`?", path.join(" "));
    }
    if disabled_prefix {
        anyhow::bail!(
            "Command abbreviations are disabled. Use `{}`.",
            path.join(" ")
        );
    }
    Ok(result)
}
