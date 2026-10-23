//! Popup layout and rendering. Everything here is pure: it produces cells and
//! escape sequences, and never touches a terminal.

pub mod popup;
pub mod style;

pub use popup::{Anchor, Item, Placement, PopupView, render};
pub use style::{Color, KindColor, Role, Style, Theme};

/// One character cell of an overlay.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Cell {
    pub ch: char,
    pub style: Style,
}

/// A rectangle of cells drawn on top of the screen.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Overlay {
    /// Screen row of the first line (0-based).
    pub top: u16,
    /// Screen column of the first cell (0-based).
    pub left: u16,
    pub rows: Vec<Vec<Cell>>,
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

    /// Whether the cell at screen position (row, col) belongs to this overlay.
    #[must_use]
    pub fn covers(&self, row: u16, col: u16) -> bool {
        row >= self.top
            && row < self.top + self.height()
            && col >= self.left
            && col < self.left + self.width()
    }

    /// The characters only, one line per row, for tests and snapshots.
    #[must_use]
    pub fn text(&self) -> String {
        self.rows
            .iter()
            .map(|r| r.iter().map(|c| c.ch).collect::<String>())
            .collect::<Vec<_>>()
            .join("\n")
    }

    /// Escape sequences that draw the overlay. They leave the cursor and the
    /// style wherever the last cell left them, so the caller restores both.
    #[must_use]
    pub fn to_ansi(&self) -> Vec<u8> {
        let mut out = String::new();
        for (i, row) in self.rows.iter().enumerate() {
            let line = u32::from(self.top) + u32::try_from(i).unwrap_or(0) + 1;
            out.push_str(&format!("\x1b[{line};{}H", u32::from(self.left) + 1));
            let mut current: Option<Style> = None;
            for cell in row {
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
