//! Reads options out of rendered man pages (the text `man` prints).
//!
//! Rendered text is used instead of the troff source because it looks the
//! same whatever macro package the page was written with (man, mdoc,
//! asciidoc's docbook output): an option line indented a few columns, and its
//! description indented further, on the same line or the lines below.

use crate::spec::{Arg, Command, Opt, Template};

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ManPage {
    /// From the NAME section: `ls - list directory contents` gives
    /// `list directory contents`.
    pub summary: String,
    /// The first paragraph of DESCRIPTION.
    pub description: String,
    pub options: Vec<ManOption>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ManOption {
    /// Every spelling, as written: `-x`, `--extract`, `--get`.
    pub names: Vec<String>,
    /// The value it takes, if any: `ARCHIVE`, `commit`.
    pub arg: Option<String>,
    /// The first paragraph of its description, unwrapped.
    pub text: String,
}

impl ManOption {
    /// A short, lowercase phrase for the list: the first clause of the text.
    #[must_use]
    pub fn short_description(&self) -> String {
        short_phrase(&self.text)
    }
}

/// The first clause of `text`, lowercased like the spec's voice (§13.4).
#[must_use]
pub fn short_phrase(text: &str) -> String {
    let mut end = text.len();
    for pat in [". ", "; ", ", or ", " (", ": "] {
        if let Some(i) = text.find(pat) {
            end = end.min(i);
        }
    }
    let mut s = text[..end].trim().trim_end_matches('.').to_string();
    // Lowercase a leading capital unless it starts an acronym (IPv4, URL).
    let mut chars = s.chars();
    if let (Some(first), Some(second)) = (chars.next(), chars.next())
        && first.is_uppercase()
        && second.is_lowercase()
    {
        s = first.to_lowercase().chain(s.chars().skip(1)).collect();
    }
    s
}

/// Remove bold/underline overstrikes (`c\bc`, `_\bc`) and colour escapes,
/// and expand tabs.
#[must_use]
pub fn clean(raw: &str) -> String {
    let mut out = String::with_capacity(raw.len());
    let mut chars = raw.chars().peekable();
    let mut col = 0usize;
    while let Some(c) = chars.next() {
        match c {
            '\x08' => {
                if out.pop().is_some() {
                    col = col.saturating_sub(1);
                }
            }
            '\x1b' => {
                if chars.peek() == Some(&'[') {
                    for c in chars.by_ref() {
                        if c.is_ascii_alphabetic() {
                            break;
                        }
                    }
                }
            }
            '\t' => {
                let next = (col / 8 + 1) * 8;
                out.extend(std::iter::repeat_n(' ', next - col));
                col = next;
            }
            '\n' => {
                out.push('\n');
                col = 0;
            }
            c => {
                out.push(c);
                col += 1;
            }
        }
    }
    out
}

fn indent(line: &str) -> usize {
    line.len() - line.trim_start_matches(' ').len()
}

fn is_section_header(line: &str) -> bool {
    !line.is_empty() && !line.starts_with(' ')
}

/// Parse a rendered man page (cleaned or not).
#[must_use]
pub fn parse(rendered: &str) -> ManPage {
    let text = clean(rendered);
    let lines: Vec<&str> = text.lines().map(str::trim_end).collect();
    let mut page = ManPage::default();
    let mut section = "";
    let mut i = 0;
    while i < lines.len() {
        let line = lines[i];
        if is_section_header(line) {
            section = line.trim();
            i += 1;
            continue;
        }
        match section {
            "NAME" if page.summary.is_empty() && !line.trim().is_empty() => {
                let para = paragraph(&lines, i, indent(line));
                page.summary = para
                    .split_once(" - ")
                    .or_else(|| para.split_once(" — "))
                    .map_or(para.as_str(), |(_, d)| d)
                    .trim()
                    .to_string();
            }
            "DESCRIPTION" if page.description.is_empty() && !line.trim().is_empty() && !line.trim().starts_with('-') => {
                page.description = paragraph(&lines, i, indent(line));
            }
            _ => {}
        }
        let lists_options = section == "DESCRIPTION" || section.contains("OPTION") || section.contains("SWITCHES");
        if lists_options && let Some((names, arg, rest)) = option_line(line) {
            let ind = indent(line);
            // Option lines stacked with no text between them share the text below.
            let mut names = names;
            let mut arg = arg;
            let mut first_text = rest;
            let mut j = i + 1;
            while first_text.is_empty() && j < lines.len() && indent(lines[j]) == ind {
                let Some((more, more_arg, more_rest)) = option_line(lines[j]) else { break };
                names.extend(more);
                arg = arg.or(more_arg);
                first_text = more_rest;
                j += 1;
            }
            let mut text = first_text;
            // The description continues on deeper-indented lines, up to the
            // first blank line once some text has been seen.
            while j < lines.len() {
                let l = lines[j];
                if l.trim().is_empty() {
                    if !text.is_empty() {
                        break;
                    }
                    j += 1;
                    continue;
                }
                if indent(l) <= ind || is_section_header(l) {
                    break;
                }
                if !text.is_empty() {
                    text.push(' ');
                }
                text.push_str(l.trim());
                j += 1;
            }
            if !page.options.iter().any(|o| o.names.iter().any(|n| names.contains(n))) {
                page.options.push(ManOption {
                    names,
                    arg,
                    text: squash(&text),
                });
            }
            i = j;
            continue;
        }
        i += 1;
    }
    page
}

/// Join a paragraph that starts at `start` and is indented at least `ind`.
fn paragraph(lines: &[&str], start: usize, ind: usize) -> String {
    let mut out = String::new();
    for l in &lines[start..] {
        if l.trim().is_empty() || indent(l) < ind || is_section_header(l) {
            break;
        }
        if !out.is_empty() {
            out.push(' ');
        }
        out.push_str(l.trim());
    }
    squash(&out)
}

/// Collapse runs of spaces and undo hyphenation at line ends.
fn squash(s: &str) -> String {
    let joined = s.replace("- ", "-").replace("‐ ", "");
    let mut out = String::with_capacity(joined.len());
    let mut space = false;
    for c in s.chars() {
        if c == ' ' {
            if !space {
                out.push(c);
            }
            space = true;
        } else {
            out.push(c);
            space = false;
        }
    }
    let _ = joined;
    out.trim().to_string()
}

/// If `line` starts an option entry, its names, value and any text on the
/// same line.
fn option_line(line: &str) -> Option<(Vec<String>, Option<String>, String)> {
    let ind = indent(line);
    if !(2..=12).contains(&ind) {
        return None;
    }
    let body = &line[ind..];
    if !body.starts_with('-') {
        return None;
    }
    // The option part ends at the first run of two spaces.
    let (head, rest) = match body.find("  ") {
        Some(at) => (&body[..at], body[at..].trim()),
        None => (body, ""),
    };
    let mut names = Vec::new();
    let mut arg = None;
    for item in head.split(", ") {
        let item = item.trim();
        let name_len = item
            .char_indices()
            .find(|&(i, c)| !(c == '-' && i < 2 || c.is_ascii_alphanumeric() || (i >= 2 && matches!(c, '-' | '_' | '.'))))
            .map_or(item.len(), |(i, _)| i);
        let name = &item[..name_len];
        let dashes = name.chars().take_while(|&c| c == '-').count();
        if dashes == 0 || dashes > 2 || name.len() <= dashes {
            return None;
        }
        let value = item[name_len..].trim_start_matches(['=', '[', ' ']).trim_end_matches(']');
        let value = value.trim_start_matches('=').trim_matches(['<', '>']);
        if !value.is_empty() {
            // Prose that happens to start with a dash is not an option.
            if value.len() > 40 || value.split(' ').count() > 3 {
                return None;
            }
            arg.get_or_insert_with(|| value.to_string());
        }
        names.push(name.to_string());
    }
    if names.is_empty() || names.len() > 6 {
        return None;
    }
    Some((names, arg, rest.to_string()))
}

/// A spec built from a man page, for commands with no curated spec.
#[must_use]
pub fn to_command(name: &str, page: &ManPage) -> Command {
    let options = page
        .options
        .iter()
        .map(|o| Opt {
            names: o.names.clone(),
            description: o.short_description(),
            arg: o.arg.as_ref().map(|a| Arg {
                name: a.clone(),
                template: value_template(a),
                ..Arg::default()
            }),
            ..Opt::default()
        })
        .collect();
    Command {
        names: vec![name.to_string()],
        description: page.summary.clone(),
        options,
        args: vec![Arg {
            name: "FILE".into(),
            optional: true,
            variadic: true,
            template: Some(Template::Filepaths),
            ..Arg::default()
        }],
        ..Command::default()
    }
}

/// Values named like paths get path suggestions.
fn value_template(arg: &str) -> Option<Template> {
    let a = arg.to_ascii_lowercase();
    if a.contains("dir") || a.contains("folder") {
        Some(Template::Folders)
    } else if a.contains("file") || a.contains("path") || a == "archive" {
        Some(Template::Filepaths)
    } else {
        None
    }
}

/// Parse `apropos`/`whatis` output (`name (1) - description`) into pairs.
#[must_use]
pub fn parse_whatis(output: &str) -> Vec<(String, String)> {
    output
        .lines()
        .filter_map(|line| {
            let (left, desc) = line.split_once(" - ")?;
            let names = left.rsplit_once(" (").map_or(left, |(n, _)| n);
            Some(
                names
                    .split(", ")
                    .map(|n| (n.trim().to_string(), desc.trim().to_string()))
                    .collect::<Vec<_>>(),
            )
        })
        .flatten()
        .filter(|(n, _)| !n.is_empty() && !n.contains(' '))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    const SORT: &str = "SORT(1)                         User Commands                        SORT(1)

NAME
       sort - sort lines of text files

SYNOPSIS
       sort [OPTION]... [FILE]...

DESCRIPTION
       Write sorted concatenation of all FILE(s) to standard output.

       Mandatory arguments to long options are mandatory for short options too.

       -b, --ignore-leading-blanks
              ignore leading blanks

       -f, --ignore-case
              fold lower case to upper case characters

       -k, --key=KEYDEF
              sort via a key; KEYDEF gives location and type

       -o, --output=FILE
              write result to FILE instead of standard output

       -t, --field-separator=SEP
              use SEP instead of non-blank to blank transition

       --help display this help and exit

       With no FILE, or when FILE is -, read standard input.
";

    const SSH: &str = "NAME
       ssh — OpenSSH remote login client

DESCRIPTION
       ssh (SSH client) is a program for logging into a remote machine.

       The options are as follows:

       -4      Forces ssh to use IPv4 addresses only.

       -L [bind_address:]port:host:hostport
               Specifies that connections to the given TCP port on the local
               host are to be forwarded to the given host and port.

       -p port
               Port to connect to on the remote host.
";

    const GIT_COMMIT: &str = "OPTIONS
       -a, --all
           Automatically stage files that have been modified and deleted, but
           new files you have not told Git about are not affected.

       -C <commit>, --reuse-message=<commit>
           Take an existing commit object, and reuse the log message.

       --dry-run
           Do not create a commit, but show a list of paths that are to be
           committed.
";

    fn names(page: &ManPage) -> Vec<String> {
        page.options.iter().map(|o| o.names.join(",")).collect()
    }

    #[test]
    fn gnu_style() {
        let page = parse(SORT);
        assert_eq!(page.summary, "sort lines of text files");
        assert_eq!(page.description, "Write sorted concatenation of all FILE(s) to standard output.");
        assert_eq!(
            names(&page),
            ["-b,--ignore-leading-blanks", "-f,--ignore-case", "-k,--key", "-o,--output", "-t,--field-separator", "--help"]
        );
        let k = &page.options[2];
        assert_eq!(k.arg.as_deref(), Some("KEYDEF"));
        assert_eq!(k.short_description(), "sort via a key");
        assert_eq!(page.options[5].text, "display this help and exit");
    }

    #[test]
    fn mdoc_style() {
        let page = parse(SSH);
        assert_eq!(page.summary, "OpenSSH remote login client");
        assert_eq!(names(&page), ["-4", "-L", "-p"]);
        assert_eq!(page.options[0].short_description(), "forces ssh to use IPv4 addresses only");
        assert_eq!(page.options[1].arg.as_deref(), Some("[bind_address:]port:host:hostport"));
        assert!(page.options[1].text.starts_with("Specifies that connections"));
        assert_eq!(page.options[2].arg.as_deref(), Some("port"));
    }

    #[test]
    fn asciidoc_style() {
        let page = parse(GIT_COMMIT);
        assert_eq!(names(&page), ["-a,--all", "-C,--reuse-message", "--dry-run"]);
        assert_eq!(page.options[1].arg.as_deref(), Some("commit"));
        assert_eq!(
            page.options[0].text,
            "Automatically stage files that have been modified and deleted, but new files you have not told Git about are not affected."
        );
        assert_eq!(page.options[0].short_description(), "automatically stage files that have been modified and deleted");
    }

    #[test]
    fn overstrikes_and_tabs() {
        assert_eq!(clean("N\x08NA\x08AM\x08ME\x08E\n\t-x"), "NAME\n        -x");
        assert_eq!(clean("_\x08f_\x08i_\x08l_\x08e"), "file");
        assert_eq!(clean("\x1b[1mbold\x1b[0m"), "bold");
    }

    #[test]
    fn stacked_option_lines_share_a_description() {
        let page = parse("OPTIONS\n       --foo\n       --bar\n              do both\n");
        assert_eq!(names(&page), ["--foo,--bar"]);
        assert_eq!(page.options[0].text, "do both");
    }

    #[test]
    fn prose_starting_with_a_dash_is_not_an_option() {
        let page = parse("DESCRIPTION\n       - this line is a bullet point in some page\n       -- is the end of options\n");
        assert!(page.options.is_empty(), "{page:?}");
    }

    #[test]
    fn builds_a_spec() {
        let page = parse(SORT);
        let cmd = to_command("sort", &page);
        assert_eq!(cmd.description, "sort lines of text files");
        let o = cmd.find_option("--output").unwrap();
        assert_eq!(o.description, "write result to FILE instead of standard output");
        assert_eq!(o.arg.as_ref().unwrap().template, Some(Template::Filepaths));
        assert!(cmd.find_option("-k").unwrap().arg.as_ref().unwrap().template.is_none());
    }

    #[test]
    fn whatis_lines() {
        let out = "ls (1)               - list directory contents\n\
                   git-commit (1)       - Record changes to the repository\n\
                   pgrep, pkill (1)     - look up processes\n";
        let pairs = parse_whatis(out);
        assert_eq!(pairs[0], ("ls".to_string(), "list directory contents".to_string()));
        assert_eq!(pairs[2].0, "pgrep");
        assert_eq!(pairs[3].0, "pkill");
    }
}
