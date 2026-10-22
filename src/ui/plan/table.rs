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
            let atomic_cell = |row_index, cell: &str| {
                let rendered = if overflow == TableOverflow::Wrap {
                    cell.to_owned()
                } else {
                    escape_table_line_breaks(cell)
                };
                data.summarized_cells.contains(&(row_index, column))
                    || rendered.is_empty()
                    || (!rendered.chars().any(char::is_whitespace)
                        && table_cell_width(cell, overflow) <= ATOMIC_MAX_WIDTH)
            };
            let has_list = data.list_items.keys().any(|(_, index)| *index == column);
            let list_column = has_list
                && data
                    .rows
                    .iter()
                    .enumerate()
                    .filter_map(|(row_index, row)| row.get(column).map(|cell| (row_index, cell)))
                    .all(|(row_index, cell)| {
                        data.list_items.contains_key(&(row_index, column))
                            || atomic_cell(row_index, cell)
                    });
            let atomic = !has_list
                && data.rows.iter().enumerate().all(|(row_index, row)| {
                    row.get(column)
                        .is_none_or(|cell| atomic_cell(row_index, cell))
                });
            if column == 0 || atomic {
                natural[column]
            } else {
                let item_width = if list_column || overflow == TableOverflow::Wrap {
                    data.rows
                        .iter()
                        .enumerate()
                        .map(|(row_index, row)| {
                            if let Some(items) = data.list_items.get(&(row_index, column)) {
                                if overflow == TableOverflow::Wrap {
                                    items
                                        .iter()
                                        .map(|item| table_cell_width(item, TableOverflow::None))
                                        .max()
                                        .unwrap_or(0)
                                } else {
                                    items.first().map_or(0, |first| {
                                        table_cell_width(first, TableOverflow::None)
                                            + if items.len() > 1 {
                                                2 + (items.len() - 1).to_string().len()
                                            } else {
                                                0
                                            }
                                    })
                                }
                            } else if list_column {
                                row.get(column)
                                    .map_or(0, |cell| table_cell_width(cell, overflow))
                            } else {
                                0
                            }
                        })
                        .max()
                        .unwrap_or(0)
                } else {
                    0
                };
                let summary_width = data
                    .summarized_cells
                    .iter()
                    .filter(|(_, index)| *index == column)
                    .map(|(row, column)| table_cell_width(&data.rows[*row][*column], overflow))
                    .max()
                    .unwrap_or(0);
                let protected = table_cell_width(header, overflow)
                    .max(item_width)
                    .max(summary_width);
                if list_column {
                    protected
                } else {
                    natural[column].min(PROSE_MIN_WIDTH.max(protected))
                }
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
        let extra = spare.min(natural[index].saturating_sub(*width));
        *width += extra;
        spare -= extra;
    }
    widths
}

// These names are literal DSL terms; both commas and whitespace delimit P terms.
fn pick_term(name: &str) -> String {
    if name
        .chars()
        .any(|ch| ch.is_whitespace() || matches!(ch, ',' | '\'' | '"' | '\\' | '|'))
    {
        crate::core::shell_words::quote_for_shell(
            name,
            crate::core::shell_words::QuoteStyle::Single,
        )
    } else {
        name.to_owned()
    }
}

fn wrap_footer(text: &str, room: usize, indented: bool) -> Vec<String> {
    use crate::ui::text::{crop_display_width, display_width};
    let mut lines = Vec::new();
    let mut remaining = text;
    while !remaining.is_empty() {
        let indent = if indented || !lines.is_empty() {
            "  "
        } else {
            ""
        };
        let width = room.saturating_sub(indent.len()).max(1);
        if display_width(remaining) <= width {
            lines.push(format!("{indent}{remaining}"));
            break;
        }
        let prefix = crop_display_width(remaining, width);
        let end = prefix
            .rfind(char::is_whitespace)
            .filter(|end| *end > 0)
            .unwrap_or_else(|| {
                if prefix.is_empty() {
                    remaining.chars().next().map(char::len_utf8).unwrap_or(0)
                } else {
                    prefix.len()
                }
            });
        lines.push(format!("{indent}{}", remaining[..end].trim_end()));
        remaining = remaining[end..].trim_start();
    }
    lines
}

pub(super) fn fit_footer(
    data: &super::ProjectedTable,
    widths: &[usize],
    settings: &crate::ui::settings::ResolvedRenderSettings,
    indent: usize,
    actions: Option<bool>,
) -> Vec<String> {
    if !settings.display_limits || !settings.table_hints {
        return Vec::new();
    }
    let hidden = &data.headers[widths.len()..];
    let shortened = widths
        .iter()
        .enumerate()
        .map(|(column, width)| {
            data.rows
                .iter()
                .enumerate()
                .filter(|(row_index, row)| {
                    let list = data.list_items.contains_key(&(*row_index, column));
                    data.summarized_cells.contains(&(*row_index, column))
                        || (settings.table_overflow == TableOverflow::Ellipsis
                            || (list && settings.table_overflow == TableOverflow::Clip))
                            && row.get(column).is_some_and(|cell| {
                                table_cell_width(cell, settings.table_overflow) > *width
                            })
                })
                .count()
        })
        .collect::<Vec<_>>();
    let count = shortened.iter().sum::<usize>();
    if hidden.is_empty() && count == 0 {
        return Vec::new();
    }
    let room = settings
        .width
        .unwrap_or(usize::MAX)
        .saturating_sub(settings.margin + indent)
        .max(1);
    let mut footer = Vec::new();
    if !hidden.is_empty() {
        footer.extend(wrap_footer(
            &format!("+{} hidden: {}", hidden.len(), hidden.join(", ")),
            room,
            false,
        ));
    }
    if count > 0 {
        let names = data
            .headers
            .iter()
            .zip(&shortened)
            .filter(|(_, count)| **count > 0)
            .map(|(name, _)| name.as_str())
            .collect::<Vec<_>>();
        footer.extend(wrap_footer(
            &format!("{count} values shortened in {}", names.join(", ")),
            room,
            false,
        ));
    }
    if let Some(cards) = actions {
        let mut hints = vec!["full width: | LESS".to_string()];
        if cards {
            hints.push("all fields: --mreg".to_string());
        }
        let second = hidden.first().or_else(|| {
            shortened
                .iter()
                .enumerate()
                .filter(|(_, count)| **count > 0)
                .max_by_key(|(index, count)| (**count, std::cmp::Reverse(*index)))
                .map(|(index, _)| &data.headers[index])
        });
        if let (Some(first), Some(second)) = (data.headers.first(), second) {
            hints.push(format!(
                "pick: | P {},{}",
                pick_term(first),
                pick_term(second)
            ));
        }
        let mut line = String::new();
        for hint in hints {
            if !line.is_empty()
                && crate::ui::text::display_width(&line)
                    + 5
                    + crate::ui::text::display_width(&hint)
                    + 2
                    > room
            {
                footer.extend(wrap_footer(&line, room, true));
                line.clear();
            }
            if !line.is_empty() {
                line.push_str("     ");
            }
            line.push_str(&hint);
        }
        footer.extend(wrap_footer(&line, room, true));
    }
    footer
}
