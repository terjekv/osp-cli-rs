use crate::core::output::OutputFormat;
use crate::core::output_model::{OutputResult, output_items_to_rows};

pub(crate) fn render(output: &OutputResult, format: OutputFormat) -> String {
    let delimiter = if format == OutputFormat::Csv {
        ','
    } else {
        '\t'
    };
    let rows = output_items_to_rows(&output.items);
    let columns = output
        .meta
        .display_columns
        .as_ref()
        .unwrap_or(&output.meta.key_index);
    let mut text = String::new();
    let mut record = |cells: Vec<String>| {
        let fields = cells
            .into_iter()
            .map(|cell| {
                let cell = skim::helper::item::strip_ansi(&cell).0;
                if cell.contains([delimiter, '"', '\r', '\n']) {
                    format!("\"{}\"", cell.replace('"', "\"\""))
                } else {
                    cell
                }
            })
            .collect::<Vec<_>>();
        text.push_str(&fields.join(&delimiter.to_string()));
        text.push_str("\r\n");
    };
    if !columns.is_empty() {
        record(columns.clone());
    }
    for row in rows {
        record(
            columns
                .iter()
                .map(|key| match row.get(key) {
                    None | Some(serde_json::Value::Null) => String::new(),
                    Some(serde_json::Value::String(text)) => text.clone(),
                    Some(value) => value.to_string(),
                })
                .collect(),
        );
    }
    text
}
