//! Splits a shell command line into words and operators.
//!
//! Positions are char indices into the line, because the shells report the
//! cursor in characters and edits are sent as one keystroke per character.

/// Shell operators that matter for finding the command under the cursor.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Op {
    /// `|`, `|&`
    Pipe,
    /// `||`
    Or,
    /// `&&`
    And,
    /// `;`, `;;`
    Semi,
    /// `&`
    Background,
    /// `(` or `)`
    Paren,
    /// `>`, `>>`, `<`, `2>`, `&>`, `>&`, `<<<` and friends.
    Redirect,
    /// `<<`, `<<-`: the next word is a heredoc delimiter.
    Heredoc,
}

impl Op {
    /// Whether a new command starts after this operator.
    #[must_use]
    pub fn separates_commands(self) -> bool {
        matches!(
            self,
            Op::Pipe | Op::Or | Op::And | Op::Semi | Op::Background | Op::Paren
        )
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TokenKind {
    Word,
    Op(Op),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Token {
    pub kind: TokenKind,
    /// First char index of the token in the line.
    pub start: usize,
    /// One past the last char index.
    pub end: usize,
    /// For words: the text with quotes and escapes removed.
    pub value: String,
    /// For words: the quote still open at the end of the token, if any.
    pub open_quote: Option<char>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Lexed {
    pub tokens: Vec<Token>,
    /// Char index where a `#` comment starts, if the line has one.
    pub comment_start: Option<usize>,
}

fn is_op_char(c: char) -> bool {
    matches!(c, '|' | '&' | ';' | '<' | '>' | '(' | ')')
}

/// Lex a whole line.
#[must_use]
pub fn lex(line: &str) -> Lexed {
    let chars: Vec<char> = line.chars().collect();
    let mut out = Lexed::default();
    let mut i = 0;
    while i < chars.len() {
        let c = chars[i];
        if c.is_whitespace() {
            i += 1;
            continue;
        }
        if c == '#' {
            out.comment_start = Some(i);
            break;
        }
        // A run of digits right before `>` or `<` is a file descriptor, as in `2>`.
        let digits = chars[i..].iter().take_while(|c| c.is_ascii_digit()).count();
        if digits > 0 && matches!(chars.get(i + digits), Some('>' | '<')) {
            let (op, len) = lex_op(&chars, i + digits);
            push_op(&mut out, op, i, i + digits + len);
            i += digits + len;
            continue;
        }
        if is_op_char(c) {
            let (op, len) = lex_op(&chars, i);
            push_op(&mut out, op, i, i + len);
            i += len;
            continue;
        }
        let (token, next) = lex_word(&chars, i);
        out.tokens.push(token);
        i = next;
    }
    out
}

fn push_op(out: &mut Lexed, op: Op, start: usize, end: usize) {
    out.tokens.push(Token {
        kind: TokenKind::Op(op),
        start,
        end,
        value: String::new(),
        open_quote: None,
    });
}

fn lex_op(chars: &[char], i: usize) -> (Op, usize) {
    let rest = |n: usize| chars.get(i + n).copied();
    match (chars[i], rest(1), rest(2)) {
        ('|', Some('|'), _) => (Op::Or, 2),
        ('|', Some('&'), _) => (Op::Pipe, 2),
        ('|', _, _) => (Op::Pipe, 1),
        ('&', Some('&'), _) => (Op::And, 2),
        ('&', Some('>'), Some('>')) => (Op::Redirect, 3),
        ('&', Some('>'), _) => (Op::Redirect, 2),
        ('&', _, _) => (Op::Background, 1),
        (';', Some(';'), _) => (Op::Semi, 2),
        (';', _, _) => (Op::Semi, 1),
        ('(' | ')', _, _) => (Op::Paren, 1),
        ('<', Some('<'), Some('<')) => (Op::Redirect, 3),
        ('<', Some('<'), Some('-')) => (Op::Heredoc, 3),
        ('<', Some('<'), _) => (Op::Heredoc, 2),
        ('<', Some('&' | '>'), _) => (Op::Redirect, 2),
        ('<', _, _) => (Op::Redirect, 1),
        ('>', Some('>' | '&' | '|'), _) => (Op::Redirect, 2),
        ('>', _, _) => (Op::Redirect, 1),
        _ => (Op::Semi, 1),
    }
}

fn lex_word(chars: &[char], start: usize) -> (Token, usize) {
    let mut value = String::new();
    let mut quote: Option<char> = None;
    let mut i = start;
    while i < chars.len() {
        let c = chars[i];
        match quote {
            Some('\'') => {
                if c == '\'' {
                    quote = None;
                } else {
                    value.push(c);
                }
            }
            Some(_) => match c {
                '"' => quote = None,
                '\\' if matches!(chars.get(i + 1), Some('"' | '\\' | '$' | '`')) => {
                    i += 1;
                    value.push(chars[i]);
                }
                _ => value.push(c),
            },
            None => match c {
                c if c.is_whitespace() || is_op_char(c) => break,
                '\'' | '"' => quote = Some(c),
                '\\' => {
                    if let Some(&next) = chars.get(i + 1) {
                        i += 1;
                        if next != '\n' {
                            value.push(next);
                        }
                    }
                }
                _ => value.push(c),
            },
        }
        i += 1;
    }
    let token = Token {
        kind: TokenKind::Word,
        start,
        end: i,
        value,
        open_quote: quote,
    };
    (token, i)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn words(line: &str) -> Vec<String> {
        lex(line)
            .tokens
            .into_iter()
            .filter(|t| t.kind == TokenKind::Word)
            .map(|t| t.value)
            .collect()
    }

    #[test]
    fn splits_simple_words() {
        assert_eq!(words("git commit --amend"), ["git", "commit", "--amend"]);
    }

    #[test]
    fn handles_quotes_and_escapes() {
        assert_eq!(
            words(r#"echo "a b" 'c d' e\ f"#),
            ["echo", "a b", "c d", "e f"]
        );
        assert_eq!(words(r#"echo "say \"hi\"""#), ["echo", r#"say "hi""#]);
    }

    #[test]
    fn reports_an_open_quote() {
        let lexed = lex(r#"git commit -m "fix th"#);
        let last = lexed.tokens.last().unwrap();
        assert_eq!(last.value, "fix th");
        assert_eq!(last.open_quote, Some('"'));
    }

    #[test]
    fn finds_operators() {
        let kinds: Vec<_> = lex("ps aux | grep x && echo ok; ls > out 2>&1 &")
            .tokens
            .into_iter()
            .map(|t| t.kind)
            .collect();
        use TokenKind::{Op as O, Word as W};
        assert_eq!(
            kinds,
            [
                W,
                W,
                O(Op::Pipe),
                W,
                W,
                O(Op::And),
                W,
                W,
                O(Op::Semi),
                W,
                O(Op::Redirect),
                W,
                O(Op::Redirect),
                W,
                O(Op::Background)
            ]
        );
    }

    #[test]
    fn finds_comments_only_at_word_start() {
        assert_eq!(lex("echo a#b # note").comment_start, Some(9));
        assert_eq!(words("echo a#b # note"), ["echo", "a#b"]);
    }

    #[test]
    fn positions_are_char_indices() {
        let lexed = lex("echo héllo wörld");
        let t = &lexed.tokens[2];
        assert_eq!((t.start, t.end), (11, 16));
    }
}
