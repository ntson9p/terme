//! Popup layout and rendering. Everything here is pure: it produces cells and
//! escape sequences, and never touches a terminal.

pub mod docs;
pub mod popup;
pub mod style;

pub use docs::DocsView;
pub use popup::{Anchor, Item, Placement, PopupView, render, render_with_docs};
pub use style::{Color, KindColor, Role, Style, Theme};

/// One character cell of an overlay.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Cell {
    pub ch: char,
    pub style: Style,
}

/// A rectangle of cells drawn on top of the screen. `None` cells are
/// transparent: the screen shows through, as between a short list and a
/// taller docs pane.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Overlay {
    /// Screen row of the first line (0-based).
    pub top: u16,
    /// Screen column of the first cell (0-based).
    pub left: u16,
    pub rows: Vec<Vec<Option<Cell>>>,
}

impl Overlay {
    #[must_use]
    pub fn height(&self) -> u16 {
        u16::try_from(self.rows.len()).unwrap_or(u16::MAX)
    }

    /// Width in columns (every row is the same width).
    #[must_use]
    pub fn width(&self) -> u16 {
        u16::try_from(self.rows.first().map_or(0, Vec::len)).unwrap_or(u16::MAX)
    }

    /// The characters only, one line per row, for tests and snapshots.
    /// Transparent cells show as spaces.
    #[must_use]
    pub fn text(&self) -> String {
        self.rows
            .iter()
            .map(|r| {
                r.iter()
                    .map(|c| c.map_or(' ', |c| c.ch))
                    .collect::<String>()
            })
            .collect::<Vec<_>>()
            .join("\n")
    }

    /// Escape sequences that draw the overlay, skipping transparent cells.
    /// They leave the cursor and the style wherever the last cell left them,
    /// so the caller restores both.
    #[must_use]
    pub fn to_ansi(&self) -> Vec<u8> {
        let mut out = String::new();
        for (i, row) in self.rows.iter().enumerate() {
            let line = u32::from(self.top) + u32::try_from(i).unwrap_or(0) + 1;
            let mut current: Option<Style> = None;
            let mut positioned = false;
            for (c, cell) in row.iter().enumerate() {
                let Some(cell) = cell else {
                    positioned = false;
                    continue;
                };
                if !positioned {
                    let col = u32::from(self.left) + u32::try_from(c).unwrap_or(0) + 1;
                    out.push_str(&format!("\x1b[{line};{col}H"));
                    positioned = true;
                }
                if current != Some(cell.style) {
                    out.push_str(&cell.style.sgr());
                    current = Some(cell.style);
                }
                out.push(cell.ch);
            }
        }
        out.push_str("\x1b[0m");
        out.into_bytes()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn transparent_cells_are_skipped() {
        let c = |ch| {
            Some(Cell {
                ch,
                style: Style::default(),
            })
        };
        let o = Overlay {
            top: 1,
            left: 2,
            rows: vec![vec![c('a'), None, c('b')]],
        };
        assert_eq!(
            String::from_utf8(o.to_ansi()).unwrap(),
            // Moving the cursor keeps the style, so it isn't sent again.
            "\x1b[2;3H\x1b[0ma\x1b[2;5Hb\x1b[0m"
        );
        assert_eq!(o.text(), "a b");
    }
}
