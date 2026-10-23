//! The real terminal: raw mode and size.

use std::io;
use std::os::fd::AsFd;

use rustix::termios::{self, OptionalActions, Termios};

/// Puts the terminal in raw mode and restores it when dropped, including on
/// panic (the panic hook in `main` drops it explicitly).
pub struct RawMode {
    saved: Termios,
}

impl RawMode {
    pub fn enable() -> io::Result<Self> {
        let stdin = io::stdin();
        let saved = termios::tcgetattr(stdin.as_fd())?;
        let mut raw = saved.clone();
        raw.make_raw();
        termios::tcsetattr(stdin.as_fd(), OptionalActions::Now, &raw)?;
        Ok(Self { saved })
    }

    pub fn restore(&self) {
        let _ = termios::tcsetattr(io::stdin().as_fd(), OptionalActions::Now, &self.saved);
    }
}

impl Drop for RawMode {
    fn drop(&mut self) {
        self.restore();
    }
}

/// (rows, cols) of the terminal on stdout, with a sane fallback.
#[must_use]
pub fn size() -> (u16, u16) {
    match termios::tcgetwinsize(io::stdout().as_fd()) {
        Ok(ws) if ws.ws_row > 0 && ws.ws_col > 0 => (ws.ws_row, ws.ws_col),
        _ => (24, 80),
    }
}

#[must_use]
pub fn is_tty() -> bool {
    termios::isatty(io::stdin().as_fd()) && termios::isatty(io::stdout().as_fd())
}
