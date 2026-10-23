//! A copy of the real terminal's screen, kept by feeding it the same bytes.
//! terme uses it to know where the cursor and the command line are, and to
//! restore the cells a popup covered.

use std::num::NonZeroU16;

use unicode_width::UnicodeWidthChar;

/// What the shell hooks reported through OSC 133 prompt marks.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Mark {
    /// `A`: a prompt is about to be printed.
    PromptStart,
    /// `B`: the prompt ended; the command line starts at this cell.
    InputStart { row: u16, col: u16 },
    /// `C`: a command started running.
    CommandStart,
    /// `D`: a command finished.
    CommandDone,
    /// The main screen scrolled up a row (not an OSC mark, but it moves the
    /// command line, so it's kept in order with the marks).
    Scrolled,
}

#[derive(Debug, Default)]
pub struct Events {
    pub marks: Vec<Mark>,
}

impl vt100::Callbacks for Events {
    fn unhandled_osc(&mut self, screen: &mut vt100::Screen, params: &[&[u8]]) {
        if params.first() != Some(&&b"133"[..]) {
            return;
        }
        let mark = match params.get(1).copied() {
            Some(b"A") => Mark::PromptStart,
            Some(b"B") => {
                let (row, col) = screen.cursor_position();
                Mark::InputStart { row, col }
            }
            Some(b"C") => Mark::CommandStart,
            Some(b"D") => Mark::CommandDone,
            _ => return,
        };
        self.marks.push(mark);
    }

    fn on_scroll(&mut self, _contents: vt100::capture::RowContents<'_>, alternate_screen: bool) {
        if !alternate_screen {
            self.marks.push(Mark::Scrolled);
        }
    }
}

pub struct Shadow {
    parser: vt100::Parser<Events>,
}

fn nz(n: u16) -> NonZeroU16 {
    NonZeroU16::new(n.max(1)).expect("max(1) is non-zero")
}

impl Shadow {
    #[must_use]
    pub fn new(rows: u16, cols: u16) -> Self {
        Self {
            parser: vt100::Parser::new_with_callbacks(nz(rows), nz(cols), 0, Events::default()),
        }
    }

    pub fn process(&mut self, bytes: &[u8]) {
        self.parser.process(bytes);
    }

    /// Marks seen since the last call, in order.
    pub fn take_marks(&mut self) -> Vec<Mark> {
        std::mem::take(&mut self.parser.callbacks_mut().marks)
    }

    pub fn set_size(&mut self, rows: u16, cols: u16) {
        self.parser.set_size(nz(rows), nz(cols));
    }

    #[must_use]
    pub fn screen(&self) -> &vt100::Screen {
        self.parser.screen()
    }

    #[must_use]
    pub fn size(&self) -> (u16, u16) {
        let (r, c) = self.screen().size();
        (r.get(), c.get())
    }

    #[must_use]
    pub fn cursor(&self) -> (u16, u16) {
        self.screen().cursor_position()
    }

    /// The command line as bash drew it, starting at `start`, and the cursor's
    /// char index in it. Text after the cursor loses trailing blanks; text
    /// before it is exact, so a typed trailing space is kept.
    #[must_use]
    pub fn read_line(&self, start: (u16, u16)) -> Option<(String, usize)> {
        let screen = self.screen();
        let (rows, cols) = self.size();
        let (cur_row, cur_col) = self.cursor();
        if start.0 >= rows || (cur_row, cur_col) < start {
            return None;
        }
        let mut before = String::new();
        let mut after = String::new();
        let (mut row, mut col) = start;
        loop {
            if col >= cols {
                if !screen.row_wrapped(row) || row + 1 >= rows {
                    break;
                }
                row += 1;
                col = 0;
                continue;
            }
            let at_or_after_cursor = (row, col) >= (cur_row, cur_col);
            let cell = screen.cell(row, col)?;
            col += 1;
            if cell.is_wide_continuation() {
                continue;
            }
            let text = if cell.has_contents() {
                cell.contents()
            } else {
                " "
            };
            if at_or_after_cursor {
                after.push_str(text);
            } else {
                before.push_str(text);
            }
        }
        // The cursor can sit past the last cell bash printed, e.g. after a
        // typed space; cells it skipped over are blanks and count.
        let cursor = before.chars().count();
        let line = format!("{before}{}", after.trim_end());
        Some((line, cursor))
    }

    /// Screen position of char index `index` of a line that starts at `start`.
    #[must_use]
    pub fn position_of(&self, start: (u16, u16), line: &str, index: usize) -> (u16, u16) {
        let (_, cols) = self.size();
        let mut col = u32::from(start.1);
        let mut row = u32::from(start.0);
        for c in line.chars().take(index) {
            let w = u32::try_from(c.width().unwrap_or(0)).unwrap_or(1);
            if col + w > u32::from(cols) {
                row += 1;
                col = 0;
            }
            col += w;
        }
        if col >= u32::from(cols) {
            row += 1;
            col = 0;
        }
        (
            u16::try_from(row).unwrap_or(u16::MAX),
            u16::try_from(col).unwrap_or(u16::MAX),
        )
    }

    /// Bytes that redraw the cells of a rectangle from this screen, then put
    /// the cursor and the current style back as the shell left them.
    #[must_use]
    pub fn restore(&self, top: u16, left: u16, height: u16, width: u16) -> Vec<u8> {
        let screen = self.screen();
        let (rows, cols) = self.size();
        let mut out = String::new();
        for row in top..(top + height).min(rows) {
            // Start on a whole character if the rectangle cuts a wide one.
            let mut col = left.min(cols);
            if col > 0
                && screen
                    .cell(row, col)
                    .is_some_and(vt100::Cell::is_wide_continuation)
            {
                col -= 1;
            }
            out.push_str(&format!("\x1b[{};{}H", row + 1, col + 1));
            let end = (left + width).min(cols);
            let mut last_style = String::new();
            while col < end {
                let Some(cell) = screen.cell(row, col) else {
                    break;
                };
                let style = sgr_of(cell);
                if style != last_style {
                    out.push_str(&style);
                    last_style.clone_from(&style);
                }
                if cell.has_contents() {
                    out.push_str(cell.contents());
                    col += if cell.is_wide() { 2 } else { 1 };
                    continue;
                }
                // Erase runs of empty cells (ECH) rather than printing spaces,
                // so they stay empty: copying text won't pick up blanks.
                let mut run = 1;
                while col + run < end
                    && screen
                        .cell(row, col + run)
                        .is_some_and(|c| !c.has_contents() && sgr_of(c) == style)
                {
                    run += 1;
                }
                out.push_str(&format!("\x1b[{run}X\x1b[{run}C"));
                col += run;
            }
        }
        out.push_str(&self.cursor_and_style());
        out.into_bytes()
    }

    /// Bytes that put the cursor, its visibility and the current style back to
    /// what the shell expects.
    #[must_use]
    pub fn cursor_and_style(&self) -> String {
        let screen = self.screen();
        let (row, col) = screen.cursor_position();
        let visibility = if screen.hide_cursor() {
            "\x1b[?25l"
        } else {
            "\x1b[?25h"
        };
        format!(
            "\x1b[0m{}\x1b[{};{}H{visibility}",
            screen.attributes_formatted(),
            row + 1,
            col + 1
        )
    }
}

fn color_codes(color: vt100::Color, base: u8, out: &mut Vec<String>) {
    match color {
        vt100::Color::Default => {}
        vt100::Color::Idx(n) if n < 8 => out.push((base + n).to_string()),
        vt100::Color::Idx(n) if n < 16 => out.push((base + 60 + n - 8).to_string()),
        vt100::Color::Idx(n) => out.push(format!("{};5;{n}", base + 8)),
        vt100::Color::Rgb(r, g, b) => out.push(format!("{};2;{r};{g};{b}", base + 8)),
    }
}

fn sgr_of(cell: &vt100::Cell) -> String {
    let mut codes = vec!["0".to_string()];
    if cell.bold() {
        codes.push("1".into());
    }
    if cell.dim() {
        codes.push("2".into());
    }
    if cell.italic() {
        codes.push("3".into());
    }
    if cell.underline() {
        codes.push("4".into());
    }
    if cell.inverse() {
        codes.push("7".into());
    }
    color_codes(cell.fgcolor(), 30, &mut codes);
    color_codes(cell.bgcolor(), 40, &mut codes);
    format!("\x1b[{}m", codes.join(";"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn records_prompt_marks_with_positions() {
        let mut s = Shadow::new(10, 40);
        s.process(b"out\r\n\x1b]133;A\x07$ \x1b]133;B\x07ls");
        assert_eq!(
            s.take_marks(),
            [Mark::PromptStart, Mark::InputStart { row: 1, col: 2 }]
        );
    }

    #[test]
    fn reads_the_line_keeping_a_typed_space() {
        let mut s = Shadow::new(10, 40);
        s.process(b"$ git ");
        assert_eq!(s.read_line((0, 2)), Some(("git ".into(), 4)));
        s.process(b"commit\x1b[3D");
        assert_eq!(s.read_line((0, 2)), Some(("git commit".into(), 7)));
    }

    #[test]
    fn reads_a_wrapped_line() {
        let mut s = Shadow::new(10, 10);
        s.process(b"$ echo 1234567890");
        assert_eq!(s.read_line((0, 2)), Some(("echo 1234567890".into(), 15)));
        assert_eq!(s.position_of((0, 2), "echo 1234567890", 5), (0, 7));
        assert_eq!(s.position_of((0, 2), "echo 1234567890", 15), (1, 7));
    }

    #[test]
    fn restores_cells_with_their_style() {
        let mut s = Shadow::new(5, 20);
        s.process(b"\x1b[31mred\x1b[0m plain\r\n$ ");
        let bytes = String::from_utf8(s.restore(0, 0, 1, 9)).unwrap();
        assert!(
            bytes.starts_with("\x1b[1;1H\x1b[0;31mred\x1b[0m plain"),
            "{bytes:?}"
        );
        assert!(bytes.ends_with("\x1b[2;3H\x1b[?25h"), "{bytes:?}");
        // Replaying the restore over a scribbled copy gives the same screen.
        let mut copy = Shadow::new(5, 20);
        copy.process(b"\x1b[31mred\x1b[0m plain\r\n$ ");
        copy.process(b"\x1b[1;1HXXXXXXXXX");
        copy.process(bytes.as_bytes());
        assert_eq!(copy.screen().contents(), s.screen().contents());
        assert_eq!(copy.cursor(), s.cursor());
    }

    #[test]
    fn counts_scrolling() {
        let mut s = Shadow::new(3, 10);
        s.process(b"1\r\n2\r\n3\r\n4\r\n5");
        assert_eq!(s.take_marks(), [Mark::Scrolled, Mark::Scrolled]);
    }
}
