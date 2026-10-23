//! The suggestion list: sizing, placement and drawing (UI spec §5.1, §7.5).

use unicode_width::UnicodeWidthChar;

use crate::style::{KindColor, Role, Theme};
use crate::{Cell, DocsView, Overlay};

/// One row of the list.
#[derive(Debug, Clone, Default)]
pub struct Item {
    /// The kind mark and its colour, when the list shows kinds.
    pub kind_mark: Option<(char, KindColor)>,
    pub name: String,
    /// Char indices in `name` to highlight.
    pub highlights: Vec<usize>,
    pub description: String,
    pub meta: String,
}

/// Everything needed to draw the list.
#[derive(Debug, Clone)]
pub struct PopupView<'a> {
    pub label: &'a str,
    pub items: &'a [Item],
    pub selected: usize,
    /// The user moved into the list (↓), so Enter inserts.
    pub focused: bool,
    pub max_rows: usize,
}

/// Where a popup goes on the screen.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Placement {
    pub top: u16,
    pub left: u16,
    pub height: u16,
    pub width: u16,
    pub above: bool,
}

/// The input line's position on the screen.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Anchor {
    /// Row and column where the word being completed starts.
    pub word_row: u16,
    pub word_col: u16,
    /// Row of the cursor (the input's last row when the line wraps).
    pub cursor_row: u16,
}

const MAX_WIDTH: usize = 64;
const MIN_WIDTH: usize = 24;
const MAX_NAME: usize = 32;
const MAX_DESC: usize = 30;
const MAX_META: usize = 14;
/// The smallest screen the popup is drawn on (UI spec §7.5).
pub const MIN_COLS: u16 = 40;
pub const MIN_ROWS: u16 = 8;

fn width_of(s: &str) -> usize {
    s.chars().map(|c| c.width().unwrap_or(0)).sum()
}

/// Cut `s` to `max` columns, ending with `…` when cut.
fn fit(s: &str, max: usize) -> String {
    if width_of(s) <= max {
        return s.to_string();
    }
    let mut out = String::new();
    let mut w = 0;
    for c in s.chars() {
        let cw = c.width().unwrap_or(0);
        if w + cw + 1 > max {
            break;
        }
        out.push(c);
        w += cw;
    }
    out.push('…');
    out
}

struct Columns {
    kind: bool,
    name: usize,
    desc: usize,
    meta: usize,
}

impl Columns {
    fn inner(&self) -> usize {
        let mut w = 1 + usize::from(self.kind) * 2 + self.name;
        if self.desc > 0 {
            w += 2 + self.desc;
        }
        if self.meta > 0 {
            w += 2 + self.meta;
        }
        w + 1
    }
}

fn columns(view: &PopupView<'_>, max_inner: usize) -> Columns {
    let mut c = Columns {
        kind: view.items.iter().any(|i| i.kind_mark.is_some()),
        name: view
            .items
            .iter()
            .map(|i| width_of(&i.name))
            .max()
            .unwrap_or(0)
            .min(MAX_NAME),
        desc: view
            .items
            .iter()
            .map(|i| width_of(&i.description))
            .max()
            .unwrap_or(0)
            .min(MAX_DESC),
        meta: view
            .items
            .iter()
            .map(|i| width_of(&i.meta))
            .max()
            .unwrap_or(0)
            .min(MAX_META),
    };
    // Too wide: give up meta, then shorten descriptions, then names, and only
    // then drop descriptions altogether.
    while c.inner() > max_inner {
        if c.meta > 0 {
            c.meta = 0;
        } else if c.desc > 10 {
            c.desc -= 1;
        } else if c.name > 20 {
            c.name -= 1;
        } else if c.desc > 0 {
            c.desc = 0;
        } else if c.name > 8 {
            c.name -= 1;
        } else {
            break;
        }
    }
    c
}

fn counter(view: &PopupView<'_>) -> String {
    let mark = if view.focused { " ↵" } else { "" };
    format!(" {}/{}{mark} ", view.selected + 1, view.items.len())
}

/// Decide where the popup goes, or `None` if there's no room for it.
#[must_use]
pub fn place(
    screen: (u16, u16),
    anchor: Anchor,
    height: u16,
    width: u16,
    name_offset: u16,
) -> Option<Placement> {
    let (rows, cols) = screen;
    if rows < MIN_ROWS || cols < MIN_COLS || width > cols {
        return None;
    }
    let first_input_row = anchor.word_row.min(anchor.cursor_row);
    let last_input_row = anchor.word_row.max(anchor.cursor_row);
    let below = rows.saturating_sub(last_input_row + 1);
    let above_space = first_input_row;
    // At least 3 items plus borders.
    let min_height = 5.min(height);
    let (top, height, is_above) = if height <= below {
        (last_input_row + 1, height, false)
    } else if height <= above_space {
        (first_input_row - height, height, true)
    } else if below >= above_space && below >= min_height {
        (last_input_row + 1, below, false)
    } else if above_space >= min_height {
        (0, above_space, true)
    } else {
        return None;
    };
    // Line the item names up with the start of the word.
    let left = anchor
        .word_col
        .saturating_sub(name_offset)
        .min(cols - width);
    Some(Placement {
        top,
        left,
        height,
        width,
        above: is_above,
    })
}

/// Draw the popup for a screen of `screen` (rows, cols) with the input at `anchor`.
#[must_use]
pub fn render(
    view: &PopupView<'_>,
    screen: (u16, u16),
    anchor: Anchor,
    theme: Theme,
) -> Option<Overlay> {
    if view.items.is_empty() {
        return None;
    }
    let (_, cols) = screen;
    let max_total = MAX_WIDTH
        .min(usize::from(cols) * 3 / 5)
        .max(MIN_WIDTH)
        .min(usize::from(cols));
    let mut cols_ = columns(view, max_total - 2);
    let label = fit(view.label, max_total.saturating_sub(8));
    let count = counter(view);
    let min_inner = (width_of(&label) + 4)
        .max(width_of(&count) + 4)
        .max(MIN_WIDTH - 2);
    let mut inner = cols_.inner().max(min_inner);
    if inner + 2 > max_total {
        inner = max_total - 2;
    }
    // Give spare width to the description column so rows end cleanly.
    if cols_.inner() < inner && cols_.desc > 0 {
        cols_.desc += inner - cols_.inner();
    }
    let rows_wanted = view.items.len().min(view.max_rows.max(1));
    let height = u16::try_from(rows_wanted + 2).unwrap_or(u16::MAX);
    let width = u16::try_from(inner + 2).unwrap_or(u16::MAX);
    let name_offset = 2 + 2 * u16::from(cols_.kind);
    let placement = place(screen, anchor, height, width, name_offset)?;
    let visible = usize::from(placement.height) - 2;

    // Keep the selected item in view.
    let scroll = if view.selected >= visible {
        view.selected + 1 - visible
    } else {
        0
    };

    let mut rows: Vec<Vec<Cell>> = Vec::with_capacity(usize::from(placement.height));
    rows.push(border_top(&label, inner, theme));
    for (i, item) in view.items.iter().enumerate().skip(scroll).take(visible) {
        let selected = i == view.selected;
        rows.push(item_row(item, &cols_, inner, selected, view.focused, theme));
    }
    rows.push(border_bottom(&count, inner, theme));
    Some(Overlay {
        top: placement.top,
        left: placement.left,
        rows: rows
            .into_iter()
            .map(|r| r.into_iter().map(Some).collect())
            .collect(),
    })
}

/// The list plus, when there's room beside it, the docs pane (§5.2).
#[must_use]
pub fn render_with_docs(
    view: &PopupView<'_>,
    docs: Option<&DocsView<'_>>,
    screen: (u16, u16),
    anchor: Anchor,
    theme: Theme,
) -> Option<Overlay> {
    let list = render(view, screen, anchor, theme)?;
    let Some(docs) = docs else {
        return Some(list);
    };
    let above = list.top < anchor.word_row.min(anchor.cursor_row);
    Some(crate::docs::attach(list, docs, screen, above, theme))
}

fn cell(ch: char, role: Role, theme: Theme) -> Cell {
    Cell {
        ch,
        style: theme.style(role),
    }
}

fn push_str(row: &mut Vec<Cell>, s: &str, role: Role, theme: Theme) {
    for c in s.chars() {
        row.push(cell(c, role, theme));
    }
}

fn border_top(label: &str, inner: usize, theme: Theme) -> Vec<Cell> {
    let mut row = vec![
        cell('╭', Role::Border, theme),
        cell('─', Role::Border, theme),
        cell(' ', Role::Border, theme),
    ];
    push_str(&mut row, label, Role::Label, theme);
    row.push(cell(' ', Role::Border, theme));
    while row.len() < inner + 1 {
        row.push(cell('─', Role::Border, theme));
    }
    row.push(cell('╮', Role::Border, theme));
    row
}

fn border_bottom(count: &str, inner: usize, theme: Theme) -> Vec<Cell> {
    let mut row = vec![cell('╰', Role::Border, theme)];
    let fill = inner.saturating_sub(width_of(count) + 2);
    for _ in 0..fill {
        row.push(cell('─', Role::Border, theme));
    }
    push_str(&mut row, count, Role::Label, theme);
    while row.len() < inner + 1 {
        row.push(cell('─', Role::Border, theme));
    }
    row.push(cell('╯', Role::Border, theme));
    row
}

fn item_row(
    item: &Item,
    c: &Columns,
    inner: usize,
    selected: bool,
    focused: bool,
    theme: Theme,
) -> Vec<Cell> {
    let on_bar = selected && focused;
    let (name_role, match_role, muted_role, fill_role) = if on_bar {
        (
            Role::Selected,
            Role::SelectedMatch,
            Role::SelectedMuted,
            Role::Selected,
        )
    } else {
        (Role::Name, Role::Match, Role::Muted, Role::Muted)
    };
    let mut row = vec![cell('│', Role::Border, theme)];
    let gutter = match (selected, focused) {
        (true, true) => cell('▌', Role::Selected, theme),
        (true, false) => cell('▏', Role::Gutter, theme),
        _ => cell(' ', fill_role, theme),
    };
    row.push(gutter);
    if c.kind {
        match item.kind_mark {
            Some((mark, color)) => {
                let role = if on_bar {
                    Role::Selected
                } else {
                    Role::Kind(color)
                };
                row.push(cell(mark, role, theme));
            }
            None => row.push(cell(' ', fill_role, theme)),
        }
        row.push(cell(' ', fill_role, theme));
    }
    let name = fit(&item.name, c.name);
    let mut w = 0;
    for (i, ch) in name.chars().enumerate() {
        let role = if item.highlights.contains(&i) {
            match_role
        } else {
            name_role
        };
        row.push(cell(ch, role, theme));
        w += ch.width().unwrap_or(0);
    }
    pad(&mut row, c.name.saturating_sub(w), fill_role, theme);
    if c.desc > 0 {
        pad(&mut row, 2, fill_role, theme);
        let desc = fit(&item.description, c.desc);
        push_str(&mut row, &desc, muted_role, theme);
        pad(
            &mut row,
            c.desc.saturating_sub(width_of(&desc)),
            fill_role,
            theme,
        );
    }
    if c.meta > 0 {
        pad(&mut row, 2, fill_role, theme);
        let meta = fit(&item.meta, c.meta);
        pad(
            &mut row,
            c.meta.saturating_sub(width_of(&meta)),
            fill_role,
            theme,
        );
        push_str(&mut row, &meta, muted_role, theme);
    }
    // Pad to the right border.
    let used: usize = row
        .iter()
        .skip(1)
        .map(|cl| cl.ch.width().unwrap_or(0))
        .sum();
    pad(&mut row, inner.saturating_sub(used), fill_role, theme);
    row.push(cell('│', Role::Border, theme));
    row
}

fn pad(row: &mut Vec<Cell>, n: usize, role: Role, theme: Theme) {
    for _ in 0..n {
        row.push(cell(' ', role, theme));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn items(rows: &[(&str, &str)]) -> Vec<Item> {
        rows.iter()
            .map(|(name, desc)| Item {
                name: (*name).into(),
                description: (*desc).into(),
                ..Item::default()
            })
            .collect()
    }

    fn anchor(row: u16, col: u16) -> Anchor {
        Anchor {
            word_row: row,
            word_col: col,
            cursor_row: row,
        }
    }

    #[test]
    fn git_commit_options_match_the_spec_mockup() {
        let items = {
            let mut v = items(&[
                ("--amend", "redo last commit"),
                ("--all  -a", "stage all changes"),
                ("--author=‹who›", "set the author"),
                ("--allow-empty", "allow no changes"),
            ]);
            v[0].highlights = vec![0, 1, 2];
            v
        };
        let view = PopupView {
            label: "option",
            items: &items,
            selected: 0,
            focused: false,
            max_rows: 8,
        };
        let o = render(&view, (24, 100), anchor(0, 20), Theme::Ansi16).unwrap();
        assert_eq!((o.top, o.left), (1, 18));
        insta::assert_snapshot!(o.text());
    }

    #[test]
    fn focused_shows_the_bar_and_enter_mark() {
        let items = items(&[("commit", "save changes"), ("checkout", "switch branches")]);
        let view = PopupView {
            label: "subcommand",
            items: &items,
            selected: 1,
            focused: true,
            max_rows: 8,
        };
        let o = render(&view, (24, 80), anchor(5, 6), Theme::Ansi16).unwrap();
        insta::assert_snapshot!(o.text());
    }

    #[test]
    fn flips_above_at_the_bottom() {
        let items = items(&[("a", "one"), ("b", "two"), ("c", "three")]);
        let view = PopupView {
            label: "option",
            items: &items,
            selected: 0,
            focused: false,
            max_rows: 8,
        };
        let o = render(&view, (24, 80), anchor(22, 10), Theme::Ansi16).unwrap();
        assert_eq!(o.top, 17);
        assert_eq!(o.top + u16::try_from(o.rows.len()).unwrap(), 22);
    }

    #[test]
    fn scrolls_to_keep_the_selection_visible() {
        let names: Vec<(String, &str)> = (0..20).map(|i| (format!("item{i:02}"), "x")).collect();
        let items: Vec<Item> = names
            .iter()
            .map(|(n, d)| Item {
                name: n.clone(),
                description: (*d).into(),
                ..Item::default()
            })
            .collect();
        let view = PopupView {
            label: "option",
            items: &items,
            selected: 12,
            focused: true,
            max_rows: 8,
        };
        let o = render(&view, (40, 80), anchor(0, 0), Theme::Ansi16).unwrap();
        let text = o.text();
        assert!(text.contains("▌item12"));
        assert!(text.contains("item05") && !text.contains("item04"));
        assert!(text.contains("13/20 ↵"));
    }

    #[test]
    fn shifts_left_at_the_right_edge_and_cuts_long_text() {
        let items = items(&[(
            "--a-very-long-option-name-indeed",
            "with a description that goes on and on",
        )]);
        let view = PopupView {
            label: "option",
            items: &items,
            selected: 0,
            focused: false,
            max_rows: 8,
        };
        let o = render(&view, (24, 60), anchor(0, 55), Theme::Ansi16).unwrap();
        let width = o.rows[0].len();
        assert!(usize::from(o.left) + width <= 60);
        assert!(o.text().contains('…'));
    }

    #[test]
    fn no_popup_on_tiny_screens() {
        let items = items(&[("a", "b")]);
        let view = PopupView {
            label: "x",
            items: &items,
            selected: 0,
            focused: false,
            max_rows: 8,
        };
        assert!(render(&view, (6, 80), anchor(0, 0), Theme::Ansi16).is_none());
        assert!(render(&view, (24, 30), anchor(0, 0), Theme::Ansi16).is_none());
    }

    #[test]
    fn rows_are_all_the_same_width() {
        let mut items = items(&[("feature/login", ""), ("main", ""), ("notes.txt", "")]);
        items[0].kind_mark = Some(('⎇', KindColor::Magenta));
        items[0].meta = "2 days ago".into();
        items[2].kind_mark = Some(('▤', KindColor::Plain));
        let view = PopupView {
            label: "branch",
            items: &items,
            selected: 0,
            focused: false,
            max_rows: 8,
        };
        let o = render(&view, (24, 80), anchor(3, 20), Theme::Ansi16).unwrap();
        let widths: Vec<usize> = o.rows.iter().map(Vec::len).collect();
        assert!(
            widths.windows(2).all(|w| w[0] == w[1]),
            "{widths:?}\n{}",
            o.text()
        );
        insta::assert_snapshot!(o.text());
    }
}
