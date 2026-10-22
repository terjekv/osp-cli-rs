//! The shared pure table fit rule. Column order is producer priority.

use crate::ui::settings::TableOverflow;
use crate::ui::text::table_cell_width;
use unicode_width::UnicodeWidthStr;

pub(super) fn fitted_table_widths(
    natural: &[usize],
    headers: &[String],
    rows: &[Vec<String>],
    available_width: Option<usize>,
    overflow: TableOverflow,
) -> Vec<usize> {
    let Some(available_width) = available_width else {
        return natural.to_vec();
    };
    if matches!(overflow, TableOverflow::None) || natural.is_empty() {
        return natural.to_vec();
    }

    let mut widths = natural.to_vec();
    let mut minimum = headers
        .iter()
        .map(|cell| table_cell_width(cell, overflow).max(1))
        .collect::<Vec<_>>();
    if matches!(overflow, TableOverflow::Ellipsis | TableOverflow::Wrap) {
        // Protect identifier cells and comma-separated identifier lists without
        // treating an isolated URL or long word inside prose as a column floor.
        let secondary_room = available_width
            .saturating_sub(widths.len() * 3 + 1)
            .saturating_sub(natural.first().copied().unwrap_or(0));
        let token_width_limit = (available_width / 3)
            .min(secondary_room / widths.len().saturating_sub(1).max(1))
            .max(1);
        for (column, minimum_width) in minimum.iter_mut().enumerate() {
            for cell in rows.iter().filter_map(|row| row.get(column)) {
                let tokens = cell.split(", ").collect::<Vec<_>>();
                if tokens
                    .iter()
                    .all(|token| !token.is_empty() && !token.chars().any(char::is_whitespace))
                {
                    let token_width = tokens
                        .iter()
                        .enumerate()
                        .map(|(index, token)| {
                            UnicodeWidthStr::width(*token) + usize::from(index + 1 < tokens.len())
                        })
                        .max()
                        .unwrap_or(1)
                        .min(token_width_limit);
                    *minimum_width = (*minimum_width).max(token_width);
                }
            }
        }
    }
    // The first column names the row; cutting it would make rows unusable,
    // so it keeps its whole width and later columns give way instead.
    if let (Some(first), Some(natural_first)) = (minimum.first_mut(), natural.first()) {
        *first = (*first).max(*natural_first);
    }
    // Column order expresses product priority. Drop secondary columns when
    // headings or useful tokens cannot fit; prose can use the remaining room.
    while widths.len() > 1
        && minimum[..widths.len()].iter().sum::<usize>() + widths.len() * 3 + 1 > available_width
    {
        widths.pop();
    }
    let budget = available_width.saturating_sub(widths.len() * 3 + 1);
    while widths.iter().sum::<usize>() > budget {
        let Some(index) = (0..widths.len())
            .filter(|index| widths[*index] > minimum[*index])
            .max_by_key(|index| widths[*index])
        else {
            break;
        };
        let excess = widths.iter().sum::<usize>() - budget;
        let reduction = excess
            .div_ceil(widths.len())
            .min(widths[index] - minimum[index]);
        widths[index] -= reduction;
    }
    if widths.len() == 1 {
        widths[0] = widths[0].min(budget.max(1));
    }
    widths
}
