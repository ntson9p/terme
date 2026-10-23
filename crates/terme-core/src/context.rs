//! Works out what is expected at the cursor: a command name, a subcommand,
//! an option, an option's value or a positional argument.

use crate::lexer::{Lexed, Op, Token, TokenKind, lex};
use crate::spec::{Arg, Command, Opt, SpecSet};

/// The word under the cursor, which a suggestion replaces.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Word {
    /// Char index where the word starts.
    pub start: usize,
    /// Char index one past the word's end (at or after the cursor).
    pub end: usize,
    /// The word's text up to the cursor, unquoted.
    pub prefix: String,
    /// The word's raw text up to the cursor, as typed.
    pub raw_prefix: String,
    /// A quote the user opened and hasn't closed yet.
    pub open_quote: Option<char>,
}

#[derive(Debug)]
pub enum Expect<'a> {
    /// Inside a comment, heredoc or another place with free text.
    FreeText,
    /// A command name.
    Command,
    /// A command terme has no spec for.
    Unknown { command: String },
    /// The file after a redirection like `>`.
    RedirectTarget,
    /// Somewhere inside a command terme knows.
    Spec(SpecContext<'a>),
}

#[derive(Debug)]
pub struct SpecContext<'a> {
    /// The deepest subcommand reached, e.g. `commit` in `git commit`.
    pub command: &'a Command,
    /// Names from the root, e.g. `["git", "commit"]`.
    pub path: Vec<&'a str>,
    pub slot: Slot<'a>,
    /// Options already on the line for this subcommand.
    pub used: Vec<&'a Opt>,
}

#[derive(Debug)]
pub enum Slot<'a> {
    /// The word starts with a dash. `cluster` is set when it's a valid bundle
    /// of short flags such as `-xz`.
    Option { cluster: Option<String> },
    /// The value of an option, either the next word or after `--opt=`.
    OptionValue { opt: &'a Opt, arg: &'a Arg },
    /// A positional word: maybe a subcommand, maybe an argument.
    Positional {
        subcommands: bool,
        arg: Option<&'a Arg>,
    },
}

#[derive(Debug)]
pub struct Context<'a> {
    pub word: Word,
    pub expect: Expect<'a>,
}

/// Commands that run another command; the command after them is what matters.
/// Each entry lists the wrapper's options that take a value.
const WRAPPERS: &[(&str, &[&str])] = &[
    (
        "sudo",
        &["-u", "-g", "-h", "-p", "-C", "-D", "-r", "-t", "-U", "-T"],
    ),
    ("doas", &["-u", "-C"]),
    ("time", &["-f", "-o"]),
    ("nice", &["-n"]),
    ("nohup", &[]),
    ("env", &["-u", "-C", "-S"]),
    ("exec", &["-a"]),
    ("command", &[]),
    ("builtin", &[]),
    ("watch", &["-n", "-d"]),
    ("xargs", &["-I", "-n", "-P", "-d", "-L", "-s", "-E", "-a"]),
];

fn char_to_byte(s: &str, char_idx: usize) -> usize {
    s.char_indices().nth(char_idx).map_or(s.len(), |(b, _)| b)
}

fn is_assignment(word: &str) -> bool {
    match word.split_once('=') {
        Some((name, _)) => {
            !name.is_empty()
                && name.chars().all(|c| c.is_ascii_alphanumeric() || c == '_')
                && !name.starts_with(|c: char| c.is_ascii_digit())
        }
        None => false,
    }
}

/// Analyze `line` with the cursor at char index `cursor`.
#[must_use]
pub fn analyze<'a>(line: &str, cursor: usize, specs: &'a SpecSet) -> Context<'a> {
    let chars: Vec<char> = line.chars().collect();
    let cursor = cursor.min(chars.len());
    let head = lex(&line[..char_to_byte(line, cursor)]);
    let full = lex(line);
    let word = current_word(&head, &full, &chars, cursor);

    if head.comment_start.is_some() {
        return Context {
            word,
            expect: Expect::FreeText,
        };
    }

    // Tokens before the current word.
    let before: Vec<&Token> = head
        .tokens
        .iter()
        .filter(|t| t.start < word.start)
        .collect();
    if let Some(last) = before.last() {
        match last.kind {
            TokenKind::Op(Op::Redirect) => {
                return Context {
                    word,
                    expect: Expect::RedirectTarget,
                };
            }
            TokenKind::Op(Op::Heredoc) => {
                return Context {
                    word,
                    expect: Expect::FreeText,
                };
            }
            _ => {}
        }
    }
    let segment_start = before
        .iter()
        .rposition(|t| matches!(t.kind, TokenKind::Op(op) if op.separates_commands()))
        .map_or(0, |i| i + 1);
    let words = segment_words(&before[segment_start..]);
    let expect = expect_for(&words, &word, specs);
    let mut ctx = Context { word, expect };
    shift_word_after_equals(&mut ctx, &chars);
    ctx
}

fn current_word(head: &Lexed, full: &Lexed, chars: &[char], cursor: usize) -> Word {
    let in_word = head
        .tokens
        .last()
        .filter(|t| t.kind == TokenKind::Word && t.end == cursor && head.comment_start.is_none());
    match in_word {
        Some(t) => {
            let end = full
                .tokens
                .iter()
                .find(|f| f.start == t.start)
                .map_or(cursor, |f| f.end.max(cursor));
            Word {
                start: t.start,
                end,
                prefix: t.value.clone(),
                raw_prefix: chars[t.start..cursor].iter().collect(),
                open_quote: t.open_quote,
            }
        }
        None => Word {
            start: cursor,
            end: cursor,
            prefix: String::new(),
            raw_prefix: String::new(),
            open_quote: None,
        },
    }
}

/// The words of one command, without redirections and their targets.
fn segment_words(tokens: &[&Token]) -> Vec<String> {
    let mut words = Vec::new();
    let mut skip_next = false;
    for t in tokens {
        match t.kind {
            TokenKind::Op(Op::Redirect | Op::Heredoc) => skip_next = true,
            TokenKind::Op(_) => {}
            TokenKind::Word => {
                if skip_next {
                    skip_next = false;
                } else {
                    words.push(t.value.clone());
                }
            }
        }
    }
    words
}

fn expect_for<'a>(words: &[String], word: &Word, specs: &'a SpecSet) -> Expect<'a> {
    // Skip `FOO=bar` assignments and wrappers like `sudo -u root`.
    let mut i = 0;
    while i < words.len() {
        let w = words[i].as_str();
        if is_assignment(w) {
            i += 1;
            continue;
        }
        if let Some((_, value_opts)) = WRAPPERS.iter().find(|(name, _)| *name == w) {
            i += 1;
            while i < words.len() && (words[i].starts_with('-') || is_assignment(&words[i])) {
                let takes_value = value_opts.contains(&words[i].as_str());
                i += 1 + usize::from(takes_value);
            }
            continue;
        }
        break;
    }
    if i >= words.len() {
        return Expect::Command;
    }
    let name = words[i].rsplit('/').next().unwrap_or(&words[i]);
    let Some(spec) = specs.get(name) else {
        return Expect::Unknown {
            command: name.to_string(),
        };
    };
    Expect::Spec(walk(spec, &words[i + 1..], word))
}

fn walk<'a>(spec: &'a Command, words: &[String], word: &Word) -> SpecContext<'a> {
    let mut node = spec;
    let mut path = vec![spec.name()];
    let mut used: Vec<&'a Opt> = Vec::new();
    let mut positional = 0usize;
    let mut pending: Option<&'a Opt> = None;
    let mut dashdash = false;

    for w in words {
        if pending.take().is_some() {
            continue;
        }
        if !dashdash && w == "--" {
            dashdash = true;
            continue;
        }
        if !dashdash && w.starts_with("--") && w.len() > 2 {
            let (name, has_value) = w
                .split_once('=')
                .map_or((w.as_str(), false), |(n, _)| (n, true));
            if let Some(opt) = node.find_option(name) {
                used.push(opt);
                if opt.takes_value() && !has_value {
                    pending = Some(opt);
                }
            }
            continue;
        }
        if !dashdash && w.starts_with('-') && w.len() > 1 {
            if let Some(opt) = node.find_option(w) {
                used.push(opt);
                if opt.takes_value() {
                    pending = Some(opt);
                }
                continue;
            }
            let letters: Vec<char> = w[1..].chars().collect();
            for (n, c) in letters.iter().enumerate() {
                let Some(opt) = node.find_option(&format!("-{c}")) else {
                    break;
                };
                used.push(opt);
                if opt.takes_value() {
                    // The rest of the word is the value, or the next word is.
                    if n + 1 == letters.len() {
                        pending = Some(opt);
                    }
                    break;
                }
            }
            continue;
        }
        if positional == 0 && !dashdash {
            if let Some(sub) = node.find_subcommand(w) {
                node = sub;
                path.push(sub.name());
                used.clear();
                continue;
            }
        }
        positional += 1;
    }

    let prefix = word.prefix.as_str();
    let slot = if let Some(opt) = pending {
        match &opt.arg {
            Some(arg) => Slot::OptionValue { opt, arg },
            None => Slot::Positional {
                subcommands: false,
                arg: None,
            },
        }
    } else if !dashdash && prefix.starts_with("--") && prefix.contains('=') {
        let name = prefix.split('=').next().unwrap_or_default();
        match node.find_option(name) {
            Some(opt) if opt.arg.is_some() => Slot::OptionValue {
                opt,
                arg: opt.arg.as_ref().expect("checked above"),
            },
            _ => Slot::Option { cluster: None },
        }
    } else if !dashdash && prefix.starts_with('-') {
        let cluster = (!prefix.starts_with("--") && node.is_short_cluster(prefix))
            .then(|| prefix.to_string());
        Slot::Option { cluster }
    } else {
        Slot::Positional {
            subcommands: positional == 0 && !dashdash && !node.subcommands.is_empty(),
            arg: arg_at(node, positional),
        }
    };
    SpecContext {
        command: node,
        path,
        slot,
        used,
    }
}

fn arg_at(node: &Command, index: usize) -> Option<&Arg> {
    node.args
        .get(index)
        .or_else(|| node.args.last().filter(|a| a.variadic))
}

/// For `--opt=val`, the word being completed is just `val`.
fn shift_word_after_equals(ctx: &mut Context<'_>, chars: &[char]) {
    let Expect::Spec(spec) = &ctx.expect else {
        return;
    };
    if !matches!(spec.slot, Slot::OptionValue { .. }) || !ctx.word.prefix.starts_with("--") {
        return;
    }
    let Some(eq) = chars[ctx.word.start..ctx.word.end]
        .iter()
        .position(|&c| c == '=')
    else {
        return;
    };
    let w = &mut ctx.word;
    w.start += eq + 1;
    w.prefix = w
        .prefix
        .split_once('=')
        .map_or_else(String::new, |(_, v)| v.to_string());
    w.raw_prefix = w
        .raw_prefix
        .split_once('=')
        .map_or_else(String::new, |(_, v)| v.to_string());
}

#[cfg(test)]
mod tests {
    use super::*;

    fn specs() -> SpecSet {
        SpecSet::builtin()
    }

    /// `|` in the test line marks the cursor.
    fn at(line_with_cursor: &str) -> (String, usize) {
        let cursor = line_with_cursor
            .chars()
            .position(|c| c == '█')
            .expect("cursor mark");
        (line_with_cursor.replacen('█', "", 1), cursor)
    }

    fn describe(line_with_cursor: &str) -> String {
        let specs = specs();
        let (line, cursor) = at(line_with_cursor);
        let ctx = analyze(&line, cursor, &specs);
        let what = match &ctx.expect {
            Expect::FreeText => "free".to_string(),
            Expect::Command => "command".to_string(),
            Expect::Unknown { command } => format!("unknown {command}"),
            Expect::RedirectTarget => "redirect".to_string(),
            Expect::Spec(s) => {
                let slot = match &s.slot {
                    Slot::Option { cluster: None } => "option".to_string(),
                    Slot::Option { cluster: Some(c) } => format!("cluster {c}"),
                    Slot::OptionValue { opt, .. } => format!("value of {}", opt.names[0]),
                    Slot::Positional { subcommands, arg } => format!(
                        "positional{}{}",
                        if *subcommands { " +sub" } else { "" },
                        arg.map(|a| format!(" {}", a.name)).unwrap_or_default()
                    ),
                };
                format!("{} {slot}", s.path.join(" "))
            }
        };
        format!("{what} [{}]", ctx.word.prefix)
    }

    #[test]
    fn command_names() {
        assert_eq!(describe("gi█"), "command [gi]");
        assert_eq!(describe("█"), "command []");
        assert_eq!(describe("ls | gr█"), "command [gr]");
        assert_eq!(describe("FOO=1 sudo -u root gi█"), "command [gi]");
    }

    #[test]
    fn subcommands_and_options() {
        assert_eq!(describe("git c█"), "git positional +sub [c]");
        assert_eq!(describe("git commit --a█"), "git commit option [--a]");
        assert_eq!(
            describe("sudo git commit █"),
            "git commit positional pathspec []"
        );
        assert_eq!(describe("git -C /tmp sta█"), "git positional +sub [sta]");
    }

    #[test]
    fn option_values() {
        assert_eq!(describe("git commit -m █"), "git commit value of -m []");
        assert_eq!(
            describe("git commit --author=Jo█"),
            "git commit value of --author [Jo]"
        );
        assert_eq!(describe("tar -xzf █"), "tar value of -f []");
        assert_eq!(describe("tar -xzf a.tgz █"), "tar positional FILE []");
    }

    #[test]
    fn clusters() {
        assert_eq!(describe("tar -xz█"), "tar cluster -xz [-xz]");
        assert_eq!(describe("tar -x█"), "tar cluster -x [-x]");
        assert_eq!(describe("tar --ex█"), "tar option [--ex]");
    }

    #[test]
    fn other_places() {
        assert_eq!(describe("echo hi > ou█"), "redirect [ou]");
        assert_eq!(describe("ls # comment █"), "free []");
        assert_eq!(describe("./deploy.sh --█"), "unknown deploy.sh [--]");
        assert_eq!(
            describe("git commit -- fi█"),
            "git commit positional pathspec [fi]"
        );
    }

    #[test]
    fn word_spans() {
        let specs = specs();
        let ctx = analyze("git commit --author=Jo", 22, &specs);
        assert_eq!((ctx.word.start, ctx.word.end), (20, 22));
        let ctx = analyze("git chec main", 6, &specs);
        assert_eq!(
            (ctx.word.start, ctx.word.end, ctx.word.prefix.as_str()),
            (4, 8, "ch")
        );
    }
}
