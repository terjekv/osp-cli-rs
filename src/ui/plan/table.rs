//! The shared pure table fit rule. Column order is producer priority.

use crate::ui::settings::TableOverflow;
use crate::ui::text::{escape_table_line_breaks, table_cell_width};

const ATOMIC_MAX_WIDTH: usize = 24;
const PROSE_MIN_WIDTH: usize = 12;

pub(super) fn fitted_table_widths(
    natural: &[usize],
    data: &super::ProjectedTable,
    available_width: Option<usize>,
    overflow: TableOverflow,
) -> Vec<usize> {
    let Some(available_width) = available_width else {
        return natural.to_vec();
    };
    if matches!(overflow, TableOverflow::None) || natural.is_empty() {
        return natural.to_vec();
    }

    let minimum = data
        .headers
        .iter()
        .enumerate()
        .map(|(column, header)| {
            let list_column = data.list_items.keys().any(|(_, index)| *index == column);
            let atomic = !list_column
                && data
                    .rows
                    .iter()
                    .filter_map(|row| row.get(column))
                    .all(|cell| {
                        let rendered = if overflow == TableOverflow::Wrap {
                            cell.clone()
                        } else {
                            escape_table_line_breaks(cell)
                        };
                        rendered.is_empty()
                            || (!rendered.chars().any(char::is_whitespace)
                                && table_cell_width(cell, overflow) <= ATOMIC_MAX_WIDTH)
                    });
            if column == 0 || atomic {
                natural[column]
            } else {
                let item_width = if overflow == TableOverflow::Wrap {
                    data.list_items
                        .iter()
                        .filter(|((_, index), _)| *index == column)
                        .flat_map(|(_, items)| items)
                        .map(|item| table_cell_width(item, TableOverflow::None))
                        .max()
                        .unwrap_or(0)
                } else {
                    0
                };
                natural[column].min(
                    PROSE_MIN_WIDTH
                        .max(table_cell_width(header, overflow))
                        .max(item_width),
                )
            }
        })
        .collect::<Vec<_>>();
    let mut widths = minimum;
    // Keep the first column whole even when it alone exceeds the available width.
    while widths.len() > 1 && widths.iter().sum::<usize>() + widths.len() * 3 + 1 > available_width
    {
        widths.pop();
    }
    let mut spare = available_width
        .saturating_sub(widths.len() * 3 + 1)
        .saturating_sub(widths.iter().sum());
    for (index, width) in widths.iter_mut().enumerate() {
        let extra = spare.min(natural[index] - *width);
        *width += extra;
        spare -= extra;
    }
    widths
}
