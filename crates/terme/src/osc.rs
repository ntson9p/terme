//! Pulls terme's own messages out of the shell's output.
//!
//! The shell hooks report state with a private OSC sequence,
//! `ESC ] 6973 ; payload BEL`. Those must never reach the real terminal, so
//! this filter removes them, wherever the reads happen to split them.

const PRIVATE: &[u8] = b"6973";
/// Payloads longer than this are cut (a command line is never this long).
const MAX_PAYLOAD: usize = 64 * 1024;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Piece {
    /// Output to show, in order.
    Text(Vec<u8>),
    /// A private message's payload, e.g. `line;5;git commit`.
    Message(Vec<u8>),
}

#[derive(Debug, Default)]
enum State {
    #[default]
    Normal,
    /// Saw ESC.
    Esc,
    /// Saw `ESC ]` and these digits so far.
    Number(Vec<u8>),
    /// Inside a private message.
    Payload(Vec<u8>),
    /// Saw ESC inside a private message (maybe the `ESC \` terminator).
    PayloadEsc(Vec<u8>),
}

#[derive(Debug, Default)]
pub struct Filter {
    state: State,
}

impl Filter {
    pub fn feed(&mut self, input: &[u8]) -> Vec<Piece> {
        let mut out = Vec::new();
        let mut text = Vec::new();
        for &b in input {
            self.state = match std::mem::take(&mut self.state) {
                State::Normal => {
                    if b == 0x1b {
                        State::Esc
                    } else {
                        text.push(b);
                        State::Normal
                    }
                }
                State::Esc => match b {
                    b']' => State::Number(Vec::new()),
                    0x1b => {
                        text.push(0x1b);
                        State::Esc
                    }
                    _ => {
                        text.extend_from_slice(&[0x1b, b]);
                        State::Normal
                    }
                },
                State::Number(mut digits) => {
                    if b == b';' && digits == PRIVATE {
                        State::Payload(Vec::new())
                    } else if b.is_ascii_digit()
                        && digits.len() < PRIVATE.len()
                        && PRIVATE.starts_with(&[&digits[..], &[b]].concat())
                    {
                        digits.push(b);
                        State::Number(digits)
                    } else {
                        // Some other OSC: let it through untouched.
                        text.extend_from_slice(b"\x1b]");
                        text.extend_from_slice(&digits);
                        if b == 0x1b {
                            State::Esc
                        } else {
                            text.push(b);
                            State::Normal
                        }
                    }
                }
                State::Payload(mut payload) => match b {
                    0x07 => {
                        flush_text(&mut out, &mut text);
                        out.push(Piece::Message(payload));
                        State::Normal
                    }
                    0x1b => State::PayloadEsc(payload),
                    _ => {
                        if payload.len() < MAX_PAYLOAD {
                            payload.push(b);
                        }
                        State::Payload(payload)
                    }
                },
                State::PayloadEsc(mut payload) => {
                    if b == b'\\' {
                        flush_text(&mut out, &mut text);
                        out.push(Piece::Message(payload));
                        State::Normal
                    } else {
                        payload.push(0x1b);
                        payload.push(b);
                        State::Payload(payload)
                    }
                }
            };
        }
        flush_text(&mut out, &mut text);
        out
    }

    /// Whether bytes are held back because they might start a private message.
    #[must_use]
    pub fn has_pending(&self) -> bool {
        matches!(self.state, State::Esc | State::Number(_))
    }

    /// Give up waiting: release held-back bytes as text.
    pub fn flush(&mut self) -> Vec<u8> {
        match std::mem::take(&mut self.state) {
            State::Esc => vec![0x1b],
            State::Number(digits) => [&b"\x1b]"[..], &digits].concat(),
            other => {
                self.state = other;
                Vec::new()
            }
        }
    }
}

fn flush_text(out: &mut Vec<Piece>, text: &mut Vec<u8>) {
    if !text.is_empty() {
        out.push(Piece::Text(std::mem::take(text)));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn run(chunks: &[&[u8]]) -> (Vec<u8>, Vec<String>) {
        let mut f = Filter::default();
        let mut text = Vec::new();
        let mut messages = Vec::new();
        for chunk in chunks {
            for piece in f.feed(chunk) {
                match piece {
                    Piece::Text(t) => text.extend(t),
                    Piece::Message(m) => messages.push(String::from_utf8(m).unwrap()),
                }
            }
        }
        text.extend(f.flush());
        (text, messages)
    }

    #[test]
    fn removes_private_messages() {
        let (text, msgs) = run(&[b"$ \x1b]6973;line;3;git\x07git"]);
        assert_eq!(text, b"$ git");
        assert_eq!(msgs, ["line;3;git"]);
    }

    #[test]
    fn keeps_other_sequences() {
        let input = b"\x1b]133;B\x07\x1b[31mred\x1b[0m\x1b]0;title\x1b\\";
        let (text, msgs) = run(&[input]);
        assert_eq!(text, input);
        assert!(msgs.is_empty());
    }

    #[test]
    fn handles_any_split() {
        let input = b"a\x1b]6973;line;1;x\x1b\\b\x1b]697c";
        for cut in 0..input.len() {
            let (text, msgs) = run(&[&input[..cut], &input[cut..]]);
            assert_eq!(text, b"ab\x1b]697c", "cut at {cut}");
            assert_eq!(msgs, ["line;1;x"], "cut at {cut}");
        }
    }

    #[test]
    fn st_terminator_and_escapes_inside() {
        let (_, msgs) = run(&[b"\x1b]6973;line;0;a\x1bb\x07"]);
        assert_eq!(msgs, ["line;0;a\x1bb"]);
    }
}
