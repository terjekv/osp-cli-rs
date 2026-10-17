pub(crate) fn normalize_identifier(value: &str) -> String {
    value.trim().to_ascii_lowercase()
}

pub(crate) fn normalize_optional_identifier(value: Option<String>) -> Option<String> {
    value
        .map(|value| normalize_identifier(&value))
        .filter(|value| !value.is_empty())
}

/// Expands a leading home-directory marker, failing only when home is needed
/// but unavailable. Callers choose whether that is an error or a literal path.
pub(crate) fn expand_home(path: &str) -> Option<std::path::PathBuf> {
    if path == "~" {
        return crate::config::default_home_dir();
    }
    if let Some(rest) = path.strip_prefix("~/").or_else(|| path.strip_prefix("~\\")) {
        return crate::config::default_home_dir().map(|home| home.join(rest));
    }
    Some(std::path::PathBuf::from(path))
}
