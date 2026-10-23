//! The docs pane beside the list (UI spec §5.2).

use unicode_width::UnicodeWidthChar;

use crate::style::{Role, Theme};
use crate::{Cell, Overlay};

/// What the docs pane shows for the highlighted item.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DocsView<'a> {
    /// The item and its aliases: `-x, --extract, --get`.
    pub title: &'a str,
    pub text: &'a str,
    /// Where the text came from: `man tar`.
    pub source: &'a str,
}

/// Docs pane width limits, in columns (§5.2).
pub const MIN_WIDTH: u16 = 32;
pub const MAX_WIDTH: u16 = 48;
/// At most this many lines of text.
const MAX_LINES: usize = 12;

fn width_of(s: &str) -> usize {
    s.chars().map(|c| c.width().unwrap_or(0)).sum()
}

/// Wrap `text` into lines of at most `width` columns.
fn wrap(text: &str, width: usize) -> Vec<String> {
    let mut lines = Vec::new();
    let mut line = String::new();
    for word in text.split_whitespace() {
        let needed = if line.is_empty() {
            width_of(word)
        } else {
            width_of(&line) + 1 + width_of(word)
        };
        if needed > width && !line.is_empty() {
            lines.push(std::mem::take(&mut line));
        }
        if !line.is_empty() {
            line.push(' ');
        }
        // A word longer than the line gets cut.
        if width_of(word) > width {
            line.extend(word.chars().take(width.saturating_sub(1)));
            line.push('…');
        } else {
            line.push_str(word);
        }
    }
    if !line.is_empty() {
        lines.push(line);
    }
    lines
}

fn cell(ch: char, role: Role, theme: Theme) -> Cell {
    Cell {
        ch,
        style: theme.style(role),
    }
}

/// Draw the pane as rows of cells, `width` columns wide and at most
/// `max_height` rows tall (borders included).
#[must_use]
pub fn render_docs(
    docs: &DocsView<'_>,
    width: u16,
    max_height: u16,
    theme: Theme,
) -> Vec<Vec<Cell>> {
    let inner = usize::from(width).saturating_sub(2);
    let text_width = inner.saturating_sub(2);
    let max_lines = usize::from(max_height).saturating_sub(2).min(MAX_LINES);
    let mut lines = wrap(docs.text, text_width);
    if lines.len() > max_lines {
        lines.truncate(max_lines);
        if let Some(last) = lines.last_mut() {
            while width_of(last) + 1 > text_width {
                last.pop();
            }
            last.push('…');
        }
    }
    let mut rows = Vec::with_capacity(lines.len() + 2);
    rows.push(border(docs.title, Role::Name, '╭', '╮', inner, theme));
    for line in &lines {
        let mut row = vec![cell('│', Role::Border, theme), cell(' ', Role::Text, theme)];
        let mut used = 1;
        for c in line.chars() {
            row.push(cell(c, Role::Text, theme));
            used += c.width().unwrap_or(0);
        }
        while used < inner {
            row.push(cell(' ', Role::Text, theme));
            used += 1;
        }
        row.push(cell('│', Role::Border, theme));
        rows.push(row);
    }
    rows.push(border(docs.source, Role::Label, '╰', '╯', inner, theme));
    rows
}

fn border(
    label: &str,
    role: Role,
    left: char,
    right: char,
    inner: usize,
    theme: Theme,
) -> Vec<Cell> {
    let mut row = vec![
        cell(left, Role::Border, theme),
        cell('─', Role::Border, theme),
    ];
    let label: String = label.chars().take(inner.saturating_sub(4)).collect();
    if !label.is_empty() {
        row.push(cell(' ', Role::Border, theme));
        for c in label.chars() {
            row.push(cell(c, role, theme));
        }
        row.push(cell(' ', Role::Border, theme));
    }
    while row.len() < inner + 1 {
        row.push(cell('─', Role::Border, theme));
    }
    row.push(cell(right, Role::Border, theme));
    row
}

/// Put the docs pane next to an already placed list: on the right if it
/// fits, else on the left, else inside the list's box (see [`inside`]). It lines up with the list's top
/// when the list is below the input, and with its bottom when above.
#[must_use]
pub fn attach(
    list: Overlay,
    docs: &DocsView<'_>,
    screen: (u16, u16),
    above: bool,
    theme: Theme,
) -> Overlay {
    let (rows, cols) = screen;
    let (lw, lh) = (list.width(), list.height());
    let room_right = cols.saturating_sub(list.left + lw);
    let room_left = list.left;
    let (on_right, width) = if room_right >= MIN_WIDTH {
        (true, room_right.min(MAX_WIDTH))
    } else if room_left >= MIN_WIDTH {
        (false, room_left.min(MAX_WIDTH))
    } else {
        return inside(list, docs, screen, above, theme);
    };
    let max_height = if above {
        list.top + lh
    } else {
        rows - list.top
    };
    let pane = render_docs(docs, width, max_height, theme);
    let ph = u16::try_from(pane.len()).unwrap_or(u16::MAX);
    let height = lh.max(ph);
    let top = if above {
        list.top + lh - height
    } else {
        list.top
    };
    let left = if on_right {
        list.left
    } else {
        list.left - width
    };
    let total = usize::from(lw + width);
    let mut out: Vec<Vec<Option<Cell>>> = vec![vec![None; total]; usize::from(height)];
    let (list_col, pane_col) = if on_right {
        (0, usize::from(lw))
    } else {
        (usize::from(width), 0)
    };
    let list_row = usize::from(list.top - top);
    for (r, row) in list.rows.into_iter().enumerate() {
        for (c, cell) in row.into_iter().enumerate() {
            out[list_row + r][list_col + c] = cell;
        }
    }
    let pane_row = if above { usize::from(height - ph) } else { 0 };
    for (r, row) in pane.into_iter().enumerate() {
        for (c, cell) in row.into_iter().enumerate() {
            out[pane_row + r][pane_col + c] = Some(cell);
        }
    }
    Overlay {
        top,
        left,
        rows: out,
    }
}

/// Lines of docs shown inside the list's box on narrow screens (§7.5).
const INSIDE_LINES: usize = 4;

/// On narrow screens: the docs go inside the list's box, under a divider,
/// and the source moves into the bottom border. Without room for at least
/// one line, the list is returned unchanged.
fn inside(
    list: Overlay,
    docs: &DocsView<'_>,
    screen: (u16, u16),
    above: bool,
    theme: Theme,
) -> Overlay {
    let (rows, _) = screen;
    let (lw, lh) = (usize::from(list.width()), list.height());
    let room = usize::from(if above {
        list.top
    } else {
        rows.saturating_sub(list.top + lh)
    });
    // One divider row, then the text.
    let max_lines = room.saturating_sub(1).min(INSIDE_LINES);
    let inner = lw.saturating_sub(2);
    let mut lines = wrap(docs.text, inner.saturating_sub(2));
    if max_lines == 0 || lines.is_empty() {
        return list;
    }
    if lines.len() > max_lines {
        lines.truncate(max_lines);
        if let Some(last) = lines.last_mut() {
            while width_of(last) + 1 > inner.saturating_sub(2) {
                last.pop();
            }
            last.push('…');
        }
    }
    let mut rows_out = list.rows;
    let mut bottom = rows_out.pop().unwrap_or_default();
    let mut divider = vec![Some(cell('├', Role::Border, theme))];
    divider.extend((0..inner).map(|_| Some(cell('─', Role::Border, theme))));
    divider.push(Some(cell('┤', Role::Border, theme)));
    rows_out.push(divider);
    for line in &lines {
        let mut row = vec![
            Some(cell('│', Role::Border, theme)),
            Some(cell(' ', Role::Text, theme)),
        ];
        let mut used = 1;
        for c in line.chars() {
            row.push(Some(cell(c, Role::Text, theme)));
            used += c.width().unwrap_or(0);
        }
        while used < inner {
            row.push(Some(cell(' ', Role::Text, theme)));
            used += 1;
        }
        row.push(Some(cell('│', Role::Border, theme)));
        rows_out.push(row);
    }
    // `╰─ man tar ───── 1/22 ──╯`: the source goes at the start of the
    // bottom border, if it fits before the counter.
    let label: Vec<char> = format!(" {} ", docs.source).chars().collect();
    let free = bottom
        .iter()
        .skip(1)
        .take_while(|c| c.is_some_and(|c| c.ch == '─'))
        .count();
    if label.len() + 2 <= free {
        for (i, ch) in label.into_iter().enumerate() {
            bottom[2 + i] = Some(cell(ch, Role::Label, theme));
        }
    }
    rows_out.push(bottom);
    let extra = u16::try_from(lines.len() + 1).unwrap_or(0);
    Overlay {
        top: if above { list.top - extra } else { list.top },
        left: list.left,
        rows: rows_out,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wraps_and_cuts() {
        assert_eq!(wrap("one two three four", 9), ["one two", "three", "four"]);
        assert_eq!(wrap("abcdefghijkl", 5), ["abcd…"]);
    }

    #[test]
    fn draws_a_box() {
        let docs = DocsView {
            title: "--amend",
            text: "Replace the tip of the current branch by creating a new commit.",
            source: "man git-commit",
        };
        let rows = render_docs(&docs, 36, 20, Theme::Ansi16);
        let text: Vec<String> = rows
            .iter()
            .map(|r| r.iter().map(|c| c.ch).collect())
            .collect();
        insta::assert_snapshot!(text.join("\n"));
        assert!(rows.iter().all(|r| r.len() == 36));
    }

    #[test]
    fn long_text_is_cut_to_the_room_available() {
        let text = "word ".repeat(200);
        let docs = DocsView {
            title: "-x",
            text: &text,
            source: "man x",
        };
        let rows = render_docs(&docs, 36, 7, Theme::Ansi16);
        assert_eq!(rows.len(), 7);
        let last_line: String = rows[5].iter().map(|c| c.ch).collect();
        assert!(last_line.contains('…'));
    }
}
