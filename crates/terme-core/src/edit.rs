//! Turns an accepted suggestion into keystrokes for the shell's line editor.
//!
//! terme never writes the line itself: it types into the shell, the same way
//! the user would, so the shell's own state (history, undo, redraw) stays right.

use crate::complete::{Candidate, Suffix};
use crate::context::Word;

/// Keystrokes that replace the word under the cursor with a suggestion.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Edit {
    /// First move the cursor right this many chars, to the end of the word.
    pub forward: usize,
    /// Then delete this many chars before the cursor.
    pub backspaces: usize,
    /// Then type this.
    pub text: String,
    /// Finally move right this many chars (over a space that was already there).
    pub after_forward: usize,
}

/// Ctrl+F: forward-char in the emacs keymaps of bash and zsh.
const FORWARD: u8 = 0x06;
/// DEL: backward-delete-char.
const BACKSPACE: u8 = 0x7f;

impl Edit {
    #[must_use]
    pub fn keystrokes(&self) -> Vec<u8> {
        let mut out = vec![FORWARD; self.forward];
        out.extend(std::iter::repeat_n(BACKSPACE, self.backspaces));
        out.extend_from_slice(self.text.as_bytes());
        out.extend(std::iter::repeat_n(FORWARD, self.after_forward));
        out
    }

    #[must_use]
    pub fn is_noop(&self) -> bool {
        self.forward == 0 && self.backspaces == 0 && self.text.is_empty() && self.after_forward == 0
    }
}

/// Plan the keystrokes to put `candidate` in place of `word`.
#[must_use]
pub fn plan(line: &str, word: &Word, cursor: usize, candidate: &Candidate) -> Edit {
    let chars: Vec<char> = line.chars().collect();
    let end = word.end.min(chars.len());
    let existing = &chars[word.start.min(end)..end];
    let new: Vec<char> = candidate.insert.chars().collect();
    let common = existing.iter().zip(&new).take_while(|(a, b)| a == b).count();
    let mut text: String = new[common..].iter().collect();
    let mut after_forward = 0;
    if candidate.suffix == Suffix::Space {
        if chars.get(end) == Some(&' ') {
            after_forward = 1;
        } else {
            text.push(' ');
        }
    }
    Edit {
        forward: end.saturating_sub(cursor),
        backspaces: existing.len() - common,
        text,
        after_forward,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::complete::Kind;
    use crate::matching::MatchKind;

    fn candidate(insert: &str, suffix: Suffix) -> Candidate {
        Candidate {
            insert: insert.into(),
            suffix,
            display: insert.into(),
            highlights: vec![],
            description: String::new(),
            meta: String::new(),
            kind: Kind::Option,
            match_kind: MatchKind::Prefix,
            priority: 0,
            opens_more: false,
        }
    }

    fn word(line: &str, start: usize, end: usize, cursor: usize) -> Word {
        let chars: Vec<char> = line.chars().collect();
        Word {
            start,
            end,
            prefix: chars[start..cursor].iter().collect(),
            raw_prefix: chars[start..cursor].iter().collect(),
            open_quote: None,
        }
    }

    #[test]
    fn completes_the_rest_of_the_word() {
        let line = "git commit --a";
        let e = plan(line, &word(line, 11, 14, 14), 14, &candidate("--amend", Suffix::Space));
        assert_eq!(e, Edit { forward: 0, backspaces: 0, text: "mend ".into(), after_forward: 0 });
        assert_eq!(e.keystrokes(), b"mend ");
    }

    #[test]
    fn replaces_a_mismatched_word() {
        let line = "git comit";
        let e = plan(line, &word(line, 4, 9, 9), 9, &candidate("commit", Suffix::Space));
        assert_eq!(e, Edit { forward: 0, backspaces: 2, text: "mit ".into(), after_forward: 0 });
    }

    #[test]
    fn cursor_inside_the_word() {
        let line = "git chec main";
        let e = plan(line, &word(line, 4, 8, 6), 6, &candidate("checkout", Suffix::Space));
        assert_eq!(e, Edit { forward: 2, backspaces: 0, text: "kout".into(), after_forward: 1 });
        assert_eq!(e.keystrokes(), b"\x06\x06kout\x06");
    }

    #[test]
    fn no_space_after_folders() {
        let line = "cd sr";
        let e = plan(line, &word(line, 3, 5, 5), 5, &candidate("src/", Suffix::Nothing));
        assert_eq!(e.text, "c/");
    }
}
