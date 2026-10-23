//! Parses raw terminal input into keys, keeping each key's original bytes so
//! anything terme doesn't handle reaches the shell unchanged.

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Key {
    Char(char),
    Tab,
    BackTab,
    Enter,
    Backspace,
    Esc,
    Up,
    Down,
    Left,
    Right,
    Home,
    End,
    PageUp,
    PageDown,
    AltUp,
    AltDown,
    CtrlSpace,
    /// Ctrl plus a letter, as its lowercase letter.
    Ctrl(char),
    Alt(char),
    /// A whole bracketed paste, markers included.
    Paste,
    /// Anything else (function keys, mouse reports…).
    Other,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct KeyEvent {
    pub key: Key,
    pub raw: Vec<u8>,
}

const PASTE_END: &[u8] = b"\x1b[201~";

#[derive(Debug, Default)]
pub struct KeyParser {
    pending: Vec<u8>,
    in_paste: bool,
}

enum Step {
    Key(Key, usize),
    /// Need more bytes to decide.
    Incomplete,
}

impl KeyParser {
    /// Bytes are held back when they might be the start of a longer sequence;
    /// call [`KeyParser::flush`] after a short timeout to release them.
    pub fn feed(&mut self, bytes: &[u8]) -> Vec<KeyEvent> {
        self.pending.extend_from_slice(bytes);
        let mut out = Vec::new();
        let mut i = 0;
        loop {
            if self.in_paste {
                match find(&self.pending[i..], PASTE_END) {
                    Some(at) => {
                        let end = i + at + PASTE_END.len();
                        match out.last_mut() {
                            Some(KeyEvent {
                                key: Key::Paste,
                                raw,
                            }) => raw.extend_from_slice(&self.pending[i..end]),
                            _ => out.push(KeyEvent {
                                key: Key::Paste,
                                raw: self.pending[i..end].to_vec(),
                            }),
                        }
                        self.in_paste = false;
                        i = end;
                        continue;
                    }
                    None => {
                        // Pass the paste through as it arrives, keeping a few
                        // bytes in case the end marker is split.
                        let keep = PASTE_END.len().min(self.pending.len() - i);
                        let cut = self.pending.len() - keep;
                        if cut > i {
                            match out.last_mut() {
                                Some(KeyEvent {
                                    key: Key::Paste,
                                    raw,
                                }) => raw.extend_from_slice(&self.pending[i..cut]),
                                _ => out.push(KeyEvent {
                                    key: Key::Paste,
                                    raw: self.pending[i..cut].to_vec(),
                                }),
                            }
                            i = cut;
                        }
                        break;
                    }
                }
            }
            if i >= self.pending.len() {
                break;
            }
            match parse_one(&self.pending[i..]) {
                Step::Key(key, len) => {
                    let raw = self.pending[i..i + len].to_vec();
                    if raw.starts_with(b"\x1b[200~") {
                        self.in_paste = true;
                        out.push(KeyEvent {
                            key: Key::Paste,
                            raw,
                        });
                    } else {
                        out.push(KeyEvent { key, raw });
                    }
                    i += len;
                }
                Step::Incomplete => break,
            }
        }
        self.pending.drain(..i);
        out
    }

    /// Whether bytes are held back waiting for the rest of a sequence.
    #[must_use]
    pub fn has_pending(&self) -> bool {
        !self.pending.is_empty()
    }

    /// Release held-back bytes: a lone Esc becomes [`Key::Esc`].
    pub fn flush(&mut self) -> Vec<KeyEvent> {
        let raw = std::mem::take(&mut self.pending);
        if raw.is_empty() {
            return Vec::new();
        }
        if self.in_paste {
            return vec![KeyEvent {
                key: Key::Paste,
                raw,
            }];
        }
        let key = if raw == b"\x1b" { Key::Esc } else { Key::Other };
        vec![KeyEvent { key, raw }]
    }
}

fn find(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    haystack.windows(needle.len()).position(|w| w == needle)
}

fn parse_one(b: &[u8]) -> Step {
    match b[0] {
        0x1b => parse_escape(b),
        0x00 => Step::Key(Key::CtrlSpace, 1),
        0x09 => Step::Key(Key::Tab, 1),
        0x0d | 0x0a => Step::Key(Key::Enter, 1),
        0x7f | 0x08 => Step::Key(Key::Backspace, 1),
        c @ 0x01..=0x1a => Step::Key(Key::Ctrl(char::from(b'a' + c - 1)), 1),
        c if c < 0x80 => Step::Key(
            if c >= 0x20 {
                Key::Char(char::from(c))
            } else {
                Key::Other
            },
            1,
        ),
        c => {
            let len = match c {
                0xc0..=0xdf => 2,
                0xe0..=0xef => 3,
                0xf0..=0xf7 => 4,
                _ => return Step::Key(Key::Other, 1),
            };
            if b.len() < len {
                return Step::Incomplete;
            }
            match std::str::from_utf8(&b[..len])
                .ok()
                .and_then(|s| s.chars().next())
            {
                Some(ch) => Step::Key(Key::Char(ch), len),
                None => Step::Key(Key::Other, 1),
            }
        }
    }
}

fn parse_escape(b: &[u8]) -> Step {
    let Some(&next) = b.get(1) else {
        return Step::Incomplete;
    };
    match next {
        b'[' => parse_csi(b),
        b'O' => match b.get(2) {
            None => Step::Incomplete,
            Some(c) => Step::Key(
                match c {
                    b'A' => Key::Up,
                    b'B' => Key::Down,
                    b'C' => Key::Right,
                    b'D' => Key::Left,
                    b'H' => Key::Home,
                    b'F' => Key::End,
                    _ => Key::Other,
                },
                3,
            ),
        },
        0x1b => Step::Key(Key::Esc, 1),
        c if (0x20..0x7f).contains(&c) => Step::Key(Key::Alt(char::from(c)), 2),
        _ => Step::Key(Key::Esc, 1),
    }
}

fn parse_csi(b: &[u8]) -> Step {
    // ESC [ params... final, where final is 0x40..=0x7e.
    let Some(end) = b[2..].iter().position(|&c| (0x40..=0x7e).contains(&c)) else {
        return if b.len() > 32 {
            Step::Key(Key::Other, b.len())
        } else {
            Step::Incomplete
        };
    };
    let len = 2 + end + 1;
    let params = &b[2..2 + end];
    let key = match (params, b[2 + end]) {
        (b"" | b"1", b'A') => Key::Up,
        (b"" | b"1", b'B') => Key::Down,
        (b"" | b"1", b'C') => Key::Right,
        (b"" | b"1", b'D') => Key::Left,
        (b"1;3", b'A') => Key::AltUp,
        (b"1;3", b'B') => Key::AltDown,
        (b"" | b"1", b'H') | (b"1" | b"7", b'~') => Key::Home,
        (b"" | b"1", b'F') | (b"4" | b"8", b'~') => Key::End,
        (b"5", b'~') => Key::PageUp,
        (b"6", b'~') => Key::PageDown,
        (b"", b'Z') => Key::BackTab,
        (b"200", b'~') => Key::Paste,
        _ => Key::Other,
    };
    Step::Key(key, len)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn keys(p: &mut KeyParser, bytes: &[u8]) -> Vec<Key> {
        p.feed(bytes).into_iter().map(|e| e.key).collect()
    }

    #[test]
    fn plain_and_control_keys() {
        let mut p = KeyParser::default();
        assert_eq!(
            keys(&mut p, b"a\t\r\x7f\x00\x03"),
            [
                Key::Char('a'),
                Key::Tab,
                Key::Enter,
                Key::Backspace,
                Key::CtrlSpace,
                Key::Ctrl('c')
            ]
        );
    }

    #[test]
    fn arrows_in_both_modes() {
        let mut p = KeyParser::default();
        assert_eq!(
            keys(&mut p, b"\x1b[A\x1bOB\x1b[1;3A\x1b[6~"),
            [Key::Up, Key::Down, Key::AltUp, Key::PageDown]
        );
    }

    #[test]
    fn alt_letters_and_lone_esc() {
        let mut p = KeyParser::default();
        assert_eq!(keys(&mut p, b"\x1bh"), [Key::Alt('h')]);
        assert!(keys(&mut p, b"\x1b").is_empty());
        assert!(p.has_pending());
        assert_eq!(
            p.flush().into_iter().map(|e| e.key).collect::<Vec<_>>(),
            [Key::Esc]
        );
    }

    #[test]
    fn split_sequences_wait_for_the_rest() {
        let mut p = KeyParser::default();
        assert!(keys(&mut p, b"\x1b[").is_empty());
        assert_eq!(keys(&mut p, b"B"), [Key::Down]);
        assert!(keys(&mut p, &"é".as_bytes()[..1]).is_empty());
        assert_eq!(keys(&mut p, &"é".as_bytes()[1..]), [Key::Char('é')]);
    }

    #[test]
    fn pastes_stay_whole_even_with_tabs_inside() {
        let mut p = KeyParser::default();
        let ev = p.feed(b"\x1b[200~ls\t-l\r\x1b[201~x");
        assert_eq!(ev.len(), 2);
        assert_eq!(ev[0].key, Key::Paste);
        assert_eq!(ev[0].raw, b"\x1b[200~ls\t-l\r\x1b[201~");
        assert_eq!(ev[1].key, Key::Char('x'));
    }

    #[test]
    fn paste_split_across_reads() {
        let mut p = KeyParser::default();
        let mut raw = Vec::new();
        for chunk in [&b"\x1b[200~hel"[..], b"lo\x1b[20", b"1~"] {
            for e in p.feed(chunk) {
                assert_eq!(e.key, Key::Paste);
                raw.extend(e.raw);
            }
        }
        assert_eq!(raw, b"\x1b[200~hello\x1b[201~");
        assert!(!p.has_pending());
    }

    #[test]
    fn raw_bytes_are_kept() {
        let mut p = KeyParser::default();
        let ev = p.feed(b"\x1bOA");
        assert_eq!(ev[0].raw, b"\x1bOA");
    }
}
