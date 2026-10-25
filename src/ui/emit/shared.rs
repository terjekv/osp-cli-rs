use crate::ui::visible_inline_text;

pub(super) fn indent_lines(text: &str, margin: usize) -> String {
    let prefix = " ".repeat(margin);
    text.lines()
        .map(|line| format!("{prefix}{line}"))
        .collect::<Vec<_>>()
        .join("\n")
}

pub(super) fn format_list_item(item: &str, inline_markup: bool) -> String {
    if inline_markup {
        visible_inline_text(item)
    } else {
        item.to_string()
    }
}

/// Applies destination margin uniformly, preserving the emitter's newlines.
pub(crate) fn apply_margin(text: String, margin: usize) -> String {
    if margin == 0 || text.is_empty() {
        return text;
    }
    let prefix = " ".repeat(margin);
    text.split_inclusive('\n')
        .map(|line| {
            if line == "\n" {
                line.to_string()
            } else {
                format!("{prefix}{line}")
            }
        })
        .collect()
}

/// Fits already styled prose without collapsing authored label alignment.
pub(crate) fn fit_prose(text: &str, width: usize) -> String {
    use unicode_width::UnicodeWidthChar;

    let mut lines = Vec::new();
    for line in text.lines() {
        let indent = line.len() - line.trim_start_matches(' ').len();
        let mut remaining = line;
        let mut continuation = false;
        let mut style = String::new();
        loop {
            let available = width
                .saturating_sub(if continuation { indent } else { 0 })
                .max(1);
            let mut columns = 0;
            let mut space = None;
            let mut end = remaining.len();
            let mut chars = remaining.char_indices();
            while let Some((index, ch)) = chars.next() {
                if ch == '\x1b' {
                    for (_, ch) in chars.by_ref() {
                        if ch.is_ascii_alphabetic() {
                            break;
                        }
                    }
                    continue;
                }
                columns += ch.width().unwrap_or(0);
                if columns > available {
                    // Even a two-column scalar in a one-column viewport must advance.
                    let next = if index == 0 { ch.len_utf8() } else { index };
                    end = space.filter(|index| *index > 0).unwrap_or(next);
                    break;
                }
                if ch == ' ' {
                    space = Some(index);
                }
            }
            let (part, rest) = remaining.split_at(end);
            let prefix = if continuation {
                " ".repeat(indent)
            } else {
                String::new()
            };
            lines.push(format!("{prefix}{style}{}", part.trim_end_matches(' ')));
            // Replay the active SGR after artwork resets the next line's palette.
            let mut escapes = part;
            while let Some(index) = escapes.find("\x1b[") {
                escapes = &escapes[index..];
                let Some(end) = escapes.find('m') else { break };
                let sgr = &escapes[..=end];
                if sgr == "\x1b[0m" {
                    style.clear();
                } else {
                    style.push_str(sgr);
                }
                escapes = &escapes[end + 1..];
            }
            if rest.is_empty() {
                break;
            }
            if !style.is_empty()
                && let Some(line) = lines.last_mut()
            {
                line.push_str("\x1b[0m");
            }
            remaining = rest.trim_start_matches(' ');
            continuation = true;
        }
    }
    let mut rendered = lines.join("\n");
    if text.ends_with('\n') {
        rendered.push('\n');
    }
    rendered
}
