use unicode_width::UnicodeWidthStr;

const GRID_PADDING: usize = 4;
const GRID_COLUMN_WEIGHT: usize = 3;

pub(crate) struct PreparedGridList {
    pub rows: Vec<Vec<String>>,
    pub source_indices: Vec<Vec<Option<usize>>>,
    pub column_widths: Vec<usize>,
    pub gap: usize,
}

impl PreparedGridList {
    pub(crate) fn from_items(values: &[String], available_width: usize) -> Self {
        let (rows, column_widths) = arrange_in_grid(
            values,
            available_width,
            GRID_PADDING,
            None,
            GRID_COLUMN_WEIGHT,
        );
        let height = values.len().div_ceil(column_widths.len().max(1));
        let preserves_empty_rows = values.len() <= 1
            || values
                .iter()
                .any(|value| UnicodeWidthStr::width(value.as_str()) > available_width);
        let source_indices = (0..height)
            .map(|row| {
                (0..column_widths.len())
                    .map(|column| {
                        let index = column * height + row;
                        (index < values.len()).then_some(index)
                    })
                    .collect::<Vec<_>>()
            })
            .filter(|row| {
                preserves_empty_rows || row.iter().flatten().any(|index| !values[*index].is_empty())
            })
            .collect();
        Self {
            rows,
            source_indices,
            column_widths,
            gap: GRID_PADDING,
        }
    }
}

fn arrange_in_grid(
    values: &[String],
    available_width: usize,
    grid_padding: usize,
    grid_columns: Option<usize>,
    column_weight: usize,
) -> (Vec<Vec<String>>, Vec<usize>) {
    let n = values.len();
    if n <= 1 {
        return (
            vec![values.to_vec()],
            vec![
                values
                    .first()
                    .map_or(0, |value| UnicodeWidthStr::width(value.as_str())),
            ],
        );
    }

    if let Some(forced) = grid_columns {
        return build_grid_matrix(values, forced.max(1).min(n));
    }

    if values
        .iter()
        .any(|value| UnicodeWidthStr::width(value.as_str()) > available_width)
    {
        let width = values
            .iter()
            .map(|value| UnicodeWidthStr::width(value.as_str()))
            .max()
            .unwrap_or(0);
        return (
            values.iter().cloned().map(|value| vec![value]).collect(),
            vec![width],
        );
    }

    let mut best_cols = 1usize;
    let mut best_score = usize::MAX;
    let mut best_widths = vec![UnicodeWidthStr::width(values[0].as_str())];
    for cols in 1..=n {
        let rows = n.div_ceil(cols);
        // `rows - cols * weight` only decreases as columns grow, so its
        // distance from zero is lowest once; stop as soon as it rises.
        let score = rows.abs_diff(cols.saturating_mul(column_weight));
        if score > best_score {
            break;
        }
        let col_widths = compute_grid_column_widths(values, cols, rows);
        let total_width = col_widths
            .iter()
            .fold(0usize, |total, width| total.saturating_add(*width))
            .saturating_add(grid_padding.saturating_mul(cols.saturating_sub(1)));
        if total_width > available_width {
            break;
        }

        best_score = score;
        best_cols = cols;
        best_widths = col_widths;
    }

    let rows = n.div_ceil(best_cols);
    let matrix = build_grid_rows(values, best_cols, rows);
    (matrix, best_widths)
}

fn build_grid_matrix(values: &[String], columns: usize) -> (Vec<Vec<String>>, Vec<usize>) {
    let rows = values.len().div_ceil(columns);
    let widths = compute_grid_column_widths(values, columns, rows);
    let matrix = build_grid_rows(values, columns, rows);
    (matrix, widths)
}

fn compute_grid_column_widths(values: &[String], columns: usize, rows: usize) -> Vec<usize> {
    let mut column_widths = vec![0usize; columns];
    for (index, value) in values.iter().enumerate() {
        let column_index = index / rows;
        if column_index >= columns {
            continue;
        }
        column_widths[column_index] =
            column_widths[column_index].max(UnicodeWidthStr::width(value.as_str()));
    }
    column_widths
}

fn build_grid_rows(values: &[String], columns: usize, rows: usize) -> Vec<Vec<String>> {
    let mut matrix = vec![vec![String::new(); columns]; rows];
    for (index, value) in values.iter().enumerate() {
        let row_index = index % rows;
        let column_index = index / rows;
        if column_index >= columns {
            continue;
        }
        matrix[row_index][column_index] = value.clone();
    }

    matrix
        .into_iter()
        .filter(|row| row.iter().any(|cell| !cell.is_empty()))
        .collect()
}

#[cfg(test)]
#[cfg_attr(coverage_nightly, coverage(off))]
mod tests {
    use super::{PreparedGridList, arrange_in_grid};

    #[test]
    fn prepared_grid_list_preserves_cells_when_width_is_too_narrow_unit() {
        let values = vec!["alphabet".to_string(), "beta".to_string()];

        let grid = PreparedGridList::from_items(&values, 4);

        assert_eq!(
            grid.rows,
            vec![vec!["alphabet".to_string()], vec!["beta".to_string()]]
        );
        assert_eq!(grid.column_widths[0], 8);
        assert_eq!(grid.gap, 4);
    }

    #[test]
    fn grid_layout_handles_single_items_and_forced_columns_unit() {
        let single = PreparedGridList::from_items(&["solo".to_string()], 20);
        assert_eq!(single.rows, vec![vec!["solo".to_string()]]);
        assert_eq!(single.column_widths, vec![4]);

        let values = vec![
            "a".to_string(),
            "bb".to_string(),
            "ccc".to_string(),
            "dddd".to_string(),
        ];
        let (rows, widths) = arrange_in_grid(&values, 40, 2, Some(2), 3);

        assert_eq!(rows.len(), 2);
        assert_eq!(rows[0], vec!["a".to_string(), "ccc".to_string()]);
        assert_eq!(rows[1], vec!["bb".to_string(), "dddd".to_string()]);
        assert_eq!(widths, vec![2, 4]);
    }

    #[test]
    fn grid_layout_prefers_balanced_columns_when_width_allows_unit() {
        let values = vec![
            "one".to_string(),
            "two".to_string(),
            "three".to_string(),
            "four".to_string(),
        ];

        let (rows, widths) = arrange_in_grid(&values, 20, 2, None, 1);

        assert_eq!(rows.len(), 2);
        assert_eq!(widths.len(), 2);
        assert_eq!(rows[0], vec!["one".to_string(), "three".to_string()]);
        assert_eq!(rows[1], vec!["two".to_string(), "four".to_string()]);
    }
}
