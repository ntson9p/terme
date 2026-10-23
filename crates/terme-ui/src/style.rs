//! Cell styles and the colour roles from the UI spec (§13.1).

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Color {
    #[default]
    Default,
    /// One of the 16 palette colours: 0–7 normal, 8–15 bright.
    Ansi(u8),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Style {
    pub fg: Color,
    pub bg: Color,
    pub bold: bool,
    pub dim: bool,
    pub underline: bool,
    pub reverse: bool,
}

impl Style {
    /// The SGR sequence that sets exactly this style, starting from a reset.
    #[must_use]
    pub fn sgr(&self) -> String {
        let mut codes = vec!["0".to_string()];
        if self.bold {
            codes.push("1".into());
        }
        if self.dim {
            codes.push("2".into());
        }
        if self.underline {
            codes.push("4".into());
        }
        if self.reverse {
            codes.push("7".into());
        }
        match self.fg {
            Color::Default => {}
            Color::Ansi(n) if n < 8 => codes.push(format!("{}", 30 + u16::from(n))),
            Color::Ansi(n) => codes.push(format!("{}", 90 + u16::from(n - 8))),
        }
        match self.bg {
            Color::Default => {}
            Color::Ansi(n) if n < 8 => codes.push(format!("{}", 40 + u16::from(n))),
            Color::Ansi(n) => codes.push(format!("{}", 100 + u16::from(n - 8))),
        }
        format!("\x1b[{}m", codes.join(";"))
    }
}

/// What a cell is for; the theme turns roles into styles.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Role {
    Border,
    Label,
    Name,
    /// Plain text, such as the docs pane's body.
    Text,
    Match,
    Muted,
    Gutter,
    Kind(KindColor),
    /// The focused row's background, and text on it.
    Selected,
    SelectedMatch,
    SelectedMuted,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum KindColor {
    Blue,
    Cyan,
    Green,
    Magenta,
    Yellow,
    Plain,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Theme {
    /// The 16-colour palette, so it matches any terminal theme.
    Ansi16,
    /// No colour at all (`NO_COLOR`): bold, dim and reverse video only.
    Mono,
}

const BLUE: u8 = 4;
const BRIGHT_BLACK: u8 = 8;
const WHITE: u8 = 7;
const BRIGHT_WHITE: u8 = 15;

impl Theme {
    #[must_use]
    pub fn style(self, role: Role) -> Style {
        let plain = Style::default();
        match self {
            Theme::Ansi16 => {
                let fg = |n| Style {
                    fg: Color::Ansi(n),
                    ..plain
                };
                let selected = Style {
                    fg: Color::Ansi(BRIGHT_WHITE),
                    bg: Color::Ansi(BLUE),
                    ..plain
                };
                match role {
                    Role::Border | Role::Label | Role::Muted => fg(BRIGHT_BLACK),
                    Role::Text => plain,
                    Role::Name => Style {
                        bold: true,
                        ..plain
                    },
                    Role::Match => Style {
                        bold: true,
                        ..fg(BLUE)
                    },
                    Role::Gutter => fg(BLUE),
                    Role::Kind(k) => match k {
                        KindColor::Blue => fg(BLUE),
                        KindColor::Cyan => fg(6),
                        KindColor::Green => fg(2),
                        KindColor::Magenta => fg(5),
                        KindColor::Yellow => fg(3),
                        KindColor::Plain => plain,
                    },
                    Role::Selected => Style {
                        bold: true,
                        ..selected
                    },
                    Role::SelectedMatch => Style {
                        bold: true,
                        underline: true,
                        ..selected
                    },
                    Role::SelectedMuted => Style {
                        fg: Color::Ansi(WHITE),
                        ..selected
                    },
                }
            }
            Theme::Mono => match role {
                Role::Border | Role::Label | Role::Muted => Style { dim: true, ..plain },
                Role::Name | Role::Match => Style {
                    bold: true,
                    ..plain
                },
                Role::Gutter | Role::Kind(_) | Role::Text => plain,
                Role::Selected | Role::SelectedMuted => Style {
                    reverse: true,
                    ..plain
                },
                Role::SelectedMatch => Style {
                    reverse: true,
                    bold: true,
                    ..plain
                },
            },
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sgr_codes() {
        let s = Style {
            fg: Color::Ansi(15),
            bg: Color::Ansi(4),
            bold: true,
            ..Style::default()
        };
        assert_eq!(s.sgr(), "\x1b[0;1;97;44m");
        assert_eq!(Style::default().sgr(), "\x1b[0m");
    }
}
