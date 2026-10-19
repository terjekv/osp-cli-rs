use unicode_width::UnicodeWidthStr;

use crate::ui::doc::{KeyValueRow, KeyValueValue};

pub(super) fn display_key(row: &KeyValueRow) -> String {
    let counted_label = row.key.rsplit_once('(').is_some_and(|(_, count)| {
        count.ends_with(')') && count.starts_with(|c: char| c.is_ascii_digit())
    });
    match &row.value {
        KeyValueValue::Array(items) if should_include_count(items) && !counted_label => {
            format!("{} ({})", row.key, items.len())
        }
        _ => row.key.clone(),
    }
}

pub(super) fn aligned_display_key_width(rows: &[KeyValueRow]) -> usize {
    rows.iter()
        .filter(|row| !matches!(&row.value, KeyValueValue::Array(items) if items.len() > 5 && items.iter().all(|item| scalar_text(item).is_some())))
        .map(|row| UnicodeWidthStr::width(display_key(row).as_str()))
        .max()
        .unwrap_or(0)
}

/// A count is shown only when it says something: never for a single row.
fn should_include_count(items: &[KeyValueValue]) -> bool {
    items.len() != 1
}

pub(super) fn scalar_text(value: &KeyValueValue) -> Option<&str> {
    match value {
        KeyValueValue::Empty => Some(""),
        KeyValueValue::Scalar(text) => Some(text),
        KeyValueValue::Array(_) | KeyValueValue::Object(_) => None,
    }
}

#[cfg(test)]
mod tests {
    use super::{aligned_display_key_width, display_key};
    use crate::ui::doc::{KeyValueRow, KeyValueValue};

    #[test]
    fn display_key_adds_counts_for_multiline_collection_values_unit() {
        let scalar = KeyValueRow {
            value_style: None,
            key: "uid".to_string(),
            value: KeyValueValue::Scalar("alice".to_string()),
            indent: None,
            gap: None,
        };
        let scalar_array = KeyValueRow {
            value_style: None,
            key: "groups".to_string(),
            value: KeyValueValue::Array(vec![
                KeyValueValue::Scalar("dev".to_string()),
                KeyValueValue::Scalar("ops".to_string()),
            ]),
            indent: None,
            gap: None,
        };

        assert_eq!(display_key(&scalar), "uid");
        assert_eq!(display_key(&scalar_array), "groups (2)");
        assert_eq!(aligned_display_key_width(&[scalar, scalar_array]), 10);
    }
}
