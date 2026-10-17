use crate::core::output_model::ColumnAlignment;
use crate::ui::text::escape_markdown_table_cell;
use unicode_width::UnicodeWidthStr;

use crate::ui::doc::TableBlock;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct PreparedTable {
    pub headers: Vec<PreparedCell>,
    pub rows: Vec<Vec<PreparedCell>>,
    pub widths: Vec<usize>,
    pub column_align: Vec<ColumnAlignment>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct PreparedCell {
    pub raw: String,
    pub markdown: String,
    pub width: usize,
    pub command: bool,
    pub style: Option<String>,
}

impl PreparedTable {
    pub(super) fn for_terminal(block: &TableBlock) -> Self {
        Self::prepare(block, 0)
    }

    pub(super) fn for_markdown(block: &TableBlock) -> Self {
        Self::prepare(block, 3)
    }

    fn prepare(block: &TableBlock, min_cell_width: usize) -> Self {
        let headers = prepare_cells(&block.headers);
        let rows = block
            .rows
            .iter()
            .enumerate()
            .map(|(index, row)| {
                let mut cells = prepare_cells(row);
                for (col, (cell, header)) in cells.iter_mut().zip(&block.headers).enumerate() {
                    cell.style = block
                        .cell_styles
                        .get(index)
                        .and_then(|row| row.get(col))
                        .cloned()
                        .flatten();
                    cell.command = header == "command";
                }
                cells
            })
            .collect::<Vec<_>>();
        let mut widths = headers
            .iter()
            .map(|cell| cell.width.max(min_cell_width))
            .collect::<Vec<_>>();

        for row in &rows {
            for (index, cell) in row.iter().enumerate() {
                if let Some(width) = widths.get_mut(index) {
                    *width = (*width).max(cell.width.max(min_cell_width));
                }
            }
        }

        Self {
            headers,
            rows,
            widths,
            column_align: block.column_align.clone(),
        }
    }
}

fn prepare_cells(cells: &[String]) -> Vec<PreparedCell> {
    cells.iter().map(|cell| PreparedCell::new(cell)).collect()
}

impl PreparedCell {
    fn new(raw: &str) -> Self {
        Self {
            command: false,
            style: None,
            raw: raw.to_string(),
            markdown: escape_markdown_table_cell(raw),
            width: UnicodeWidthStr::width(raw),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::PreparedTable;
    use crate::core::output_model::ColumnAlignment;
    use crate::ui::doc::{KeyValueRow, KeyValueValue, TableBlock};

    #[test]
    fn prepared_table_captures_shared_width_and_markdown_shape_unit() {
        let table = TableBlock {
            cell_styles: Vec::new(),
            summary: vec![KeyValueRow {
                key: "count".to_string(),
                value: KeyValueValue::Scalar("1".to_string()),
                indent: None,
                gap: None,
            }],
            headers: vec!["na|me".to_string(), "id".to_string()],
            rows: vec![vec!["ali|ce".to_string(), "42".to_string()]],
            column_align: vec![ColumnAlignment::Left, ColumnAlignment::Right],
            nested: false,
        };

        let prepared = PreparedTable::for_markdown(&table);

        assert_eq!(prepared.widths, vec![6, 3]);
        assert_eq!(prepared.headers[0].raw, "na|me");
        assert_eq!(prepared.headers[0].markdown, "na\\|me");
        assert_eq!(prepared.rows[0][0].raw, "ali|ce");
        assert_eq!(prepared.rows[0][0].markdown, "ali\\|ce");
        assert_eq!(
            prepared.column_align,
            vec![ColumnAlignment::Left, ColumnAlignment::Right]
        );
    }
}
