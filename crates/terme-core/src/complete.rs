//! Turns a [`Context`] into a ranked list of suggestions.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use crate::context::{Context, Expect, Slot, SpecContext, Word};
use crate::matching::{Match, MatchKind, match_description, match_name};
use crate::spec::{Arg, Generator, Opt, SpecSet, Template};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Kind {
    Command,
    Subcommand,
    Option,
    /// A value from a fixed list.
    Value,
    File,
    Dir,
    /// A value from a generator, labelled with its kind (`branch`, `unit`…).
    Generated(String),
}

/// What follows the inserted text.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Suffix {
    Space,
    Nothing,
}

#[derive(Debug, Clone)]
pub struct Candidate {
    /// Raw text (already escaped for the shell) that replaces the word.
    pub insert: String,
    pub suffix: Suffix,
    /// What the list shows, e.g. `-x, --extract` or `src/`.
    pub display: String,
    /// Char indices in `display` to highlight.
    pub highlights: Vec<usize>,
    pub description: String,
    /// Extra muted text on the right, e.g. a file size.
    pub meta: String,
    pub kind: Kind,
    pub match_kind: MatchKind,
    pub priority: i32,
    /// After inserting it, values are expected right away (`-f `, `src/`).
    pub opens_more: bool,
}

#[derive(Debug, Default)]
pub struct Suggestions {
    /// The list's top-border label: `option`, `branch`, `‹FILE› archive to read`…
    pub label: String,
    pub items: Vec<Candidate>,
    /// Generators whose results aren't available yet.
    pub pending: Vec<Generator>,
}

/// Everything outside the line that suggestions depend on.
pub struct Env<'a> {
    pub specs: &'a SpecSet,
    /// Executables on `PATH`, builtins and aliases.
    pub commands: &'a [String],
    /// The shell's working folder.
    pub cwd: &'a Path,
    pub home: Option<&'a Path>,
    /// Cached generator output; `None` means it hasn't run yet.
    pub generated: &'a dyn Fn(&Generator) -> Option<Arc<Vec<String>>>,
}

/// The most entries a folder listing contributes.
pub const MAX_PATHS: usize = 200;

/// Suggestions for the word under the cursor, best first.
#[must_use]
pub fn complete(ctx: &Context<'_>, env: &Env<'_>) -> Suggestions {
    let word = &ctx.word;
    let mut out = match &ctx.expect {
        Expect::FreeText => Suggestions::default(),
        Expect::Command => commands(word, env),
        Expect::Unknown { .. } | Expect::RedirectTarget => {
            if ctx.word.prefix.is_empty() && matches!(ctx.expect, Expect::Unknown { .. }) {
                Suggestions::default()
            } else {
                let mut s = Suggestions {
                    label: "file".into(),
                    ..Suggestions::default()
                };
                s.items = paths(word, env, Template::Filepaths, &[]);
                s
            }
        }
        Expect::Spec(spec) => in_spec(spec, word, env),
    };
    rank(&mut out.items);
    out
}

/// Whether the list should open on its own for these suggestions.
#[must_use]
pub fn worth_showing(ctx: &Context<'_>, s: &Suggestions) -> bool {
    let Some(first) = s.items.first() else {
        return false;
    };
    if matches!(ctx.expect, Expect::Command) && ctx.word.prefix.is_empty() {
        return false;
    }
    // A finished word with nothing longer to offer.
    let complete = first.match_kind == MatchKind::Exact
        && first.insert == ctx.word.raw_prefix
        && s.items[1..]
            .iter()
            .all(|c| c.match_kind > MatchKind::PrefixNoCase);
    !complete
}

fn rank(items: &mut [Candidate]) {
    items.sort_by(|a, b| {
        a.match_kind
            .cmp(&b.match_kind)
            .then(b.priority.cmp(&a.priority))
            .then_with(|| a.display.to_lowercase().cmp(&b.display.to_lowercase()))
    });
}

fn commands(word: &Word, env: &Env<'_>) -> Suggestions {
    let mut s = Suggestions {
        label: "command".into(),
        ..Suggestions::default()
    };
    if word.prefix.is_empty() {
        return s;
    }
    if word.prefix.contains('/') {
        s.label = "file".into();
        s.items = paths(word, env, Template::Filepaths, &[]);
        return s;
    }
    let mut seen = std::collections::HashSet::new();
    for spec in env.specs.iter() {
        if let Some(m) = match_name(&word.prefix, spec.name()) {
            seen.insert(spec.name().to_string());
            s.items.push(simple(
                spec.name(),
                &spec.description,
                Kind::Command,
                m,
                100,
                word,
            ));
        }
    }
    for name in env.commands {
        if seen.contains(name) {
            continue;
        }
        if let Some(m) = match_name(&word.prefix, name) {
            if m.kind <= MatchKind::PrefixNoCase {
                seen.insert(name.clone());
                s.items.push(simple(name, "", Kind::Command, m, 0, word));
            }
        }
    }
    s
}

fn simple(
    name: &str,
    description: &str,
    kind: Kind,
    m: Match,
    priority: i32,
    word: &Word,
) -> Candidate {
    Candidate {
        insert: escape(name, word.open_quote, true),
        suffix: Suffix::Space,
        display: name.to_string(),
        highlights: m.positions,
        description: description.to_string(),
        meta: String::new(),
        kind,
        match_kind: m.kind,
        priority,
        opens_more: false,
    }
}

fn in_spec(spec: &SpecContext<'_>, word: &Word, env: &Env<'_>) -> Suggestions {
    match &spec.slot {
        Slot::Option {
            cluster: Some(cluster),
        } => cluster_additions(spec, cluster),
        Slot::Option { cluster: None } => options(spec, word),
        Slot::OptionValue { arg, .. } => {
            let mut s = Suggestions {
                label: arg_label(arg),
                ..Suggestions::default()
            };
            arg_values(arg, word, env, &mut s);
            s
        }
        Slot::Positional { subcommands, arg } => {
            let mut s = Suggestions::default();
            if *subcommands {
                s.label = "subcommand".into();
                for sub in &spec.command.subcommands {
                    if let Some((m, name)) = best_name(&word.prefix, &sub.names) {
                        s.items.push(simple(
                            name,
                            &sub.description,
                            Kind::Subcommand,
                            m,
                            sub.priority,
                            word,
                        ));
                    }
                }
            }
            if let Some(arg) = arg {
                if s.items.is_empty() {
                    s.label = arg_label(arg);
                }
                arg_values(arg, word, env, &mut s);
            }
            s
        }
    }
}

fn best_name<'n>(query: &str, names: &'n [String]) -> Option<(Match, &'n str)> {
    names
        .iter()
        .filter_map(|n| match_name(query, n).map(|m| (m, n.as_str())))
        .min_by_key(|(m, _)| m.kind)
}

fn arg_label(arg: &Arg) -> String {
    if let Some(generator) = &arg.generator {
        if arg.template.is_none() {
            return generator.kind.clone();
        }
    }
    if arg.description.is_empty() {
        format!("‹{}›", arg.name)
    } else {
        format!("‹{}› {}", arg.name, arg.description)
    }
}

fn options(spec: &SpecContext<'_>, word: &Word) -> Suggestions {
    let prefix = word.prefix.as_str();
    let typing_long = prefix.starts_with("--");
    let mut s = Suggestions {
        label: "option".into(),
        ..Suggestions::default()
    };
    let available: Vec<&Opt> = spec
        .command
        .options
        .iter()
        .filter(|o| o.repeatable || !spec.used.iter().any(|u| std::ptr::eq(*u, *o)))
        .filter(|o| !spec.used.iter().any(|u| u.conflicts_with(o)))
        .collect();
    for opt in &available {
        let names: Vec<String> = opt
            .names
            .iter()
            .filter(|n| !typing_long || n.starts_with("--"))
            .cloned()
            .collect();
        let Some((m, matched)) = best_name(prefix, &names) else {
            continue;
        };
        s.items
            .push(option_candidate(opt, matched, m, typing_long, word));
    }
    if s.items.is_empty() && prefix.trim_start_matches('-').chars().count() >= 3 {
        s.label = "option · matched in docs".into();
        for opt in available {
            if match_description(prefix, &opt.description) {
                let name = if typing_long { opt.long() } else { opt.short() }
                    .or_else(|| opt.names.first().map(String::as_str))
                    .unwrap_or_default()
                    .to_string();
                let m = Match {
                    kind: MatchKind::Description,
                    positions: Vec::new(),
                };
                s.items
                    .push(option_candidate(opt, &name, m, typing_long, word));
            }
        }
    }
    s
}

fn option_candidate(
    opt: &Opt,
    matched: &str,
    m: Match,
    typing_long: bool,
    word: &Word,
) -> Candidate {
    let placeholder = opt.arg.as_ref().map(|a| format!("‹{}›", a.name));
    let (display, offset) = option_display(opt, matched, typing_long, placeholder.as_deref());
    // Insert the spelling the user is typing: long if they typed `--`.
    let insert_name = if typing_long || matched.starts_with("--") || opt.short().is_none() {
        matched.to_string()
    } else {
        opt.short().unwrap_or(matched).to_string()
    };
    let (insert, suffix) = match &opt.arg {
        Some(_) if insert_name.starts_with("--") && opt.requires_equals => {
            (format!("{insert_name}="), Suffix::Nothing)
        }
        _ => (insert_name, Suffix::Space),
    };
    Candidate {
        insert: escape(&insert, word.open_quote, false),
        suffix,
        display,
        highlights: m.positions.iter().map(|p| p + offset).collect(),
        description: opt.description.clone(),
        meta: if opt.deprecated {
            "deprecated".into()
        } else {
            String::new()
        },
        kind: Kind::Option,
        match_kind: m.kind,
        priority: opt.priority - if opt.deprecated { 1000 } else { 0 },
        opens_more: opt.arg.is_some(),
    }
}

/// The list text for an option, and where `matched` starts inside it.
fn option_display(
    opt: &Opt,
    matched: &str,
    typing_long: bool,
    placeholder: Option<&str>,
) -> (String, usize) {
    let long = opt
        .long()
        .or_else(|| opt.names.iter().find(|n| n.len() > 2).map(String::as_str));
    let short = opt.short();
    let value = |name: &str| match placeholder {
        Some(p) if name.starts_with("--") => format!("{name}={p}"),
        Some(p) => format!("{name} {p}"),
        None => name.to_string(),
    };
    if typing_long {
        // `--all  -a`: the long name leads.
        let mut d = value(matched);
        if let Some(short) = short {
            d.push_str("  ");
            d.push_str(short);
        }
        return (d, 0);
    }
    match (short, long) {
        (Some(short), Some(long)) => {
            let d = format!("{short}, {}", value(long));
            let offset = if matched == short {
                0
            } else {
                short.chars().count() + 2
            };
            (d, offset)
        }
        (Some(short), None) => (value(short), 0),
        (None, Some(long)) => (format!("    {}", value(long)), 4),
        (None, None) => (value(matched), 0),
    }
}

fn cluster_additions(spec: &SpecContext<'_>, cluster: &str) -> Suggestions {
    let mut s = Suggestions {
        label: format!("add to {cluster}"),
        ..Suggestions::default()
    };
    // If the last flag takes a value, the value comes next, not more flags.
    let last = cluster.chars().last().map(|c| format!("-{c}"));
    if let Some(opt) = last.and_then(|l| spec.command.find_option(&l)) {
        if opt.takes_value() {
            return s;
        }
    }
    let in_cluster: Vec<&Opt> = cluster[1..]
        .chars()
        .filter_map(|c| spec.command.find_option(&format!("-{c}")))
        .collect();
    for opt in &spec.command.options {
        let Some(short) = opt.short() else { continue };
        let letter = &short[1..];
        let used = cluster[1..].contains(letter) || spec.used.iter().any(|u| std::ptr::eq(*u, opt));
        if used && !opt.repeatable {
            continue;
        }
        if in_cluster
            .iter()
            .chain(&spec.used)
            .any(|u| u.conflicts_with(opt))
        {
            continue;
        }
        let needs = opt
            .arg
            .as_ref()
            .map(|a| format!("needs ‹{}›", a.name))
            .unwrap_or_default();
        s.items.push(Candidate {
            insert: format!("{cluster}{letter}"),
            suffix: if opt.takes_value() {
                Suffix::Space
            } else {
                Suffix::Nothing
            },
            display: letter.to_string(),
            highlights: Vec::new(),
            description: opt.description.clone(),
            meta: needs,
            kind: Kind::Option,
            match_kind: MatchKind::Prefix,
            priority: opt.priority,
            opens_more: opt.takes_value(),
        });
    }
    s
}

fn arg_values(arg: &Arg, word: &Word, env: &Env<'_>, s: &mut Suggestions) {
    if arg.new_path && word.prefix.is_empty() {
        return;
    }
    for sug in &arg.suggestions {
        if let Some(m) = match_name(&word.prefix, &sug.name) {
            let mut c = simple(
                &sug.name,
                &sug.description,
                Kind::Value,
                m,
                sug.priority,
                word,
            );
            c.priority += 50;
            s.items.push(c);
        }
    }
    if let Some(generator) = &arg.generator {
        match (env.generated)(generator) {
            Some(values) => {
                // Huge lists (all apt packages) only make sense once filtered.
                let limit = if word.prefix.is_empty() {
                    MAX_PATHS
                } else {
                    2000
                };
                let matches = values
                    .iter()
                    .filter_map(|v| match_name(&word.prefix, v).map(|m| (v, m)))
                    .filter(|(_, m)| m.kind <= MatchKind::Subsequence)
                    .take(limit);
                for (value, m) in matches {
                    let mut c = simple(
                        value,
                        "",
                        Kind::Generated(generator.kind.clone()),
                        m,
                        60,
                        word,
                    );
                    c.meta.clone_from(&generator.kind);
                    s.items.push(c);
                }
            }
            None => s.pending.push(generator.clone()),
        }
    }
    if let Some(template) = arg.template {
        s.items.extend(paths(word, env, template, &arg.extensions));
    }
}

/// Files and folders matching the word.
fn paths(word: &Word, env: &Env<'_>, template: Template, extensions: &[String]) -> Vec<Candidate> {
    let value = word.prefix.as_str();
    let (dir_value, partial) = match value.rfind('/') {
        Some(i) => (&value[..=i], &value[i + 1..]),
        None => ("", value),
    };
    let raw_dir = match word.raw_prefix.rfind('/') {
        Some(i) => &word.raw_prefix[..=i],
        None => word
            .raw_prefix
            .get(..usize::from(word.open_quote.is_some()))
            .unwrap_or(""),
    };
    let dir = resolve_dir(dir_value, env);
    let Ok(entries) = std::fs::read_dir(&dir) else {
        return Vec::new();
    };
    let mut out = Vec::new();
    for entry in entries.flatten() {
        let name = entry.file_name().to_string_lossy().into_owned();
        if name.starts_with('.') && !partial.starts_with('.') {
            continue;
        }
        let Some(m) = match_name(partial, &name) else {
            continue;
        };
        if m.kind > MatchKind::Subsequence {
            continue;
        }
        let meta = std::fs::metadata(entry.path()).ok();
        let is_dir = meta.as_ref().is_some_and(std::fs::Metadata::is_dir);
        if template == Template::Folders && !is_dir {
            continue;
        }
        let wanted_ext = !is_dir && extensions.iter().any(|e| name.ends_with(e.as_str()));
        let escaped = escape_in(&name, word.open_quote);
        let (insert, suffix) = if is_dir {
            (format!("{raw_dir}{escaped}/"), Suffix::Nothing)
        } else {
            let close = word.open_quote.map(String::from).unwrap_or_default();
            (format!("{raw_dir}{escaped}{close}"), Suffix::Space)
        };
        let display = if is_dir {
            format!("{name}/")
        } else {
            name.clone()
        };
        out.push(Candidate {
            insert,
            suffix,
            display,
            highlights: m.positions,
            description: String::new(),
            meta: match (&meta, is_dir) {
                (Some(md), false) => human_size(md.len()),
                _ => String::new(),
            },
            kind: if is_dir { Kind::Dir } else { Kind::File },
            match_kind: m.kind,
            priority: if wanted_ext {
                40
            } else if is_dir {
                10
            } else {
                0
            },
            opens_more: is_dir,
        });
        if out.len() >= MAX_PATHS {
            break;
        }
    }
    out
}

fn resolve_dir(dir_value: &str, env: &Env<'_>) -> PathBuf {
    if dir_value.is_empty() {
        return env.cwd.to_path_buf();
    }
    if let Some(rest) = dir_value.strip_prefix('~') {
        if let Some(home) = env.home {
            return home.join(rest.trim_start_matches('/'));
        }
    }
    let p = Path::new(dir_value);
    if p.is_absolute() {
        p.to_path_buf()
    } else {
        env.cwd.join(p)
    }
}

fn human_size(bytes: u64) -> String {
    const UNITS: [&str; 5] = ["B", "KB", "MB", "GB", "TB"];
    #[allow(clippy::cast_precision_loss)]
    let mut size = bytes as f64;
    let mut unit = 0;
    while size >= 1024.0 && unit < UNITS.len() - 1 {
        size /= 1024.0;
        unit += 1;
    }
    if unit == 0 || size >= 10.0 {
        format!("{size:.0} {}", UNITS[unit])
    } else {
        format!("{size:.1} {}", UNITS[unit])
    }
}

/// Escape a value for insertion as a whole word. Inside an open quote, the
/// value continues that quote; otherwise special characters get a backslash.
#[must_use]
pub fn escape(value: &str, open_quote: Option<char>, whole_word: bool) -> String {
    match open_quote {
        Some(q) => {
            let close = if whole_word {
                q.to_string()
            } else {
                String::new()
            };
            format!("{q}{}{close}", escape_in(value, Some(q)))
        }
        None => escape_in(value, None),
    }
}

fn escape_in(value: &str, open_quote: Option<char>) -> String {
    let mut out = String::with_capacity(value.len());
    for (i, c) in value.chars().enumerate() {
        match open_quote {
            Some('\'') => {
                if c == '\'' {
                    out.push_str("'\\''");
                } else {
                    out.push(c);
                }
            }
            Some(_) => {
                if matches!(c, '"' | '\\' | '$' | '`') {
                    out.push('\\');
                }
                out.push(c);
            }
            None => {
                let special = matches!(
                    c,
                    ' ' | '\t'
                        | '\''
                        | '"'
                        | '\\'
                        | '$'
                        | '`'
                        | '&'
                        | '|'
                        | ';'
                        | '<'
                        | '>'
                        | '('
                        | ')'
                        | '*'
                        | '?'
                        | '['
                        | ']'
                        | '{'
                        | '}'
                        | '!'
                ) || (i == 0 && matches!(c, '#' | '~'));
                if special {
                    out.push('\\');
                }
                out.push(c);
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::context::analyze;

    struct Fixture {
        specs: SpecSet,
        commands: Vec<String>,
        dir: tempfile_dir::Dir,
    }

    /// A tiny temp-dir helper, to avoid a dependency for tests.
    mod tempfile_dir {
        pub struct Dir(pub std::path::PathBuf);
        impl Dir {
            pub fn new(tag: &str) -> Self {
                let p =
                    std::env::temp_dir().join(format!("terme-test-{tag}-{}", std::process::id()));
                let _ = std::fs::remove_dir_all(&p);
                std::fs::create_dir_all(&p).unwrap();
                Self(p)
            }
        }
        impl Drop for Dir {
            fn drop(&mut self) {
                let _ = std::fs::remove_dir_all(&self.0);
            }
        }
    }

    impl Fixture {
        fn new(tag: &str) -> Self {
            let dir = tempfile_dir::Dir::new(tag);
            for f in ["backup.tar.gz", "notes.txt", "site.tgz", "my file.txt"] {
                std::fs::write(dir.0.join(f), b"x").unwrap();
            }
            std::fs::create_dir(dir.0.join("old")).unwrap();
            std::fs::create_dir(dir.0.join(".hidden")).unwrap();
            Self {
                specs: SpecSet::builtin(),
                commands: ["gcc", "gzip", "git", "grep", "echo"]
                    .map(String::from)
                    .to_vec(),
                dir,
            }
        }

        fn suggest(&self, line_with_cursor: &str) -> Suggestions {
            self.suggest_with(line_with_cursor, &|_| None)
        }

        fn suggest_with(
            &self,
            line_with_cursor: &str,
            generated: &dyn Fn(&Generator) -> Option<Arc<Vec<String>>>,
        ) -> Suggestions {
            let cursor = line_with_cursor.chars().position(|c| c == '█').unwrap();
            let line = line_with_cursor.replacen('█', "", 1);
            let ctx = analyze(&line, cursor, &self.specs);
            let env = Env {
                specs: &self.specs,
                commands: &self.commands,
                cwd: &self.dir.0,
                home: None,
                generated,
            };
            complete(&ctx, &env)
        }
    }

    fn displays(s: &Suggestions) -> Vec<&str> {
        s.items.iter().map(|c| c.display.as_str()).collect()
    }

    #[test]
    fn commands_put_known_ones_first() {
        let f = Fixture::new("cmd");
        let s = f.suggest("g█");
        assert_eq!(s.label, "command");
        assert_eq!(displays(&s)[..2], ["git", "grep"]);
        assert!(displays(&s).contains(&"gzip"));
    }

    #[test]
    fn git_subcommands_rank_by_priority() {
        let f = Fixture::new("sub");
        let s = f.suggest("git c█");
        assert_eq!(
            displays(&s)[..4],
            ["commit", "checkout", "clone", "cherry-pick"]
        );
        assert_eq!(s.items[0].insert, "commit");
        assert_eq!(s.items[0].suffix, Suffix::Space);
    }

    #[test]
    fn long_options_lead_with_the_long_name() {
        let f = Fixture::new("long");
        let s = f.suggest("git commit --a█");
        assert_eq!(
            displays(&s),
            ["--amend", "--all  -a", "--author=‹who›", "--allow-empty"]
        );
        assert_eq!(s.items[1].insert, "--all");
        assert_eq!(s.items[2].insert, "--author=");
        assert_eq!(s.items[2].suffix, Suffix::Nothing);
    }

    #[test]
    fn short_options_pair_like_man_pages() {
        let f = Fixture::new("short");
        let s = f.suggest("tar -█");
        assert_eq!(
            displays(&s)[..4],
            [
                "-x, --extract",
                "-c, --create",
                "-t, --list",
                "-f, --file=‹FILE›"
            ]
        );
        assert_eq!(s.items[0].insert, "-x");
        assert!(displays(&s).contains(&"    --exclude=‹PATTERN›"));
    }

    #[test]
    fn used_options_are_hidden() {
        let f = Fixture::new("used");
        let s = f.suggest("git commit --amend --█");
        assert!(!displays(&s).iter().any(|d| d.starts_with("--amend")));
    }

    #[test]
    fn clusters_offer_letters() {
        let f = Fixture::new("cluster");
        let s = f.suggest("tar -xz█");
        assert_eq!(s.label, "add to -xz");
        assert_eq!(displays(&s)[..2], ["f", "v"]);
        assert_eq!(s.items[0].insert, "-xzf");
        assert_eq!(s.items[0].suffix, Suffix::Space);
        assert_eq!(s.items[0].meta, "needs ‹FILE›");
        assert!(f.suggest("tar -xzf█").items.is_empty());
    }

    #[test]
    fn archives_come_first_for_tar_file() {
        let f = Fixture::new("tarfile");
        let s = f.suggest("tar -xzf █");
        assert_eq!(s.label, "‹FILE› archive to read");
        assert_eq!(
            displays(&s),
            [
                "backup.tar.gz",
                "site.tgz",
                "old/",
                "my file.txt",
                "notes.txt"
            ]
        );
        assert_eq!(s.items[2].insert, "old/");
        assert_eq!(s.items[2].suffix, Suffix::Nothing);
        assert_eq!(s.items[3].insert, r"my\ file.txt");
    }

    #[test]
    fn hidden_files_only_when_asked() {
        let f = Fixture::new("hidden");
        assert!(!displays(&f.suggest("cd █")).contains(&".hidden/"));
        assert_eq!(displays(&f.suggest("cd .█")), [".hidden/"]);
        assert_eq!(displays(&f.suggest("cd █")), ["old/"]);
    }

    #[test]
    fn open_quotes_are_continued() {
        let f = Fixture::new("quote");
        let s = f.suggest("cat \"my█");
        assert_eq!(s.items[0].insert, "\"my file.txt\"");
    }

    #[test]
    fn description_search() {
        let f = Fixture::new("desc");
        let s = f.suggest("docker run --port█");
        assert_eq!(s.label, "option · matched in docs");
        assert_eq!(
            displays(&s)[..3],
            [
                "--publish=‹host:ctr›  -p",
                "--publish-all  -P",
                "--expose=‹port›"
            ]
        );
        assert_eq!(s.items[0].insert, "--publish");
    }

    #[test]
    fn generators_are_requested_then_used() {
        let f = Fixture::new("gen");
        let s = f.suggest("git checkout fe█");
        assert_eq!(s.pending.len(), 1);
        let s = f.suggest_with("git checkout fe█", &|_| {
            Some(Arc::new(vec![
                "feature/login".into(),
                "main".into(),
                "feature/search".into(),
            ]))
        });
        assert!(s.pending.is_empty());
        assert_eq!(displays(&s)[..2], ["feature/login", "feature/search"]);
        assert_eq!(s.items[0].meta, "branch");
    }

    #[test]
    fn quiet_when_the_word_is_finished() {
        let f = Fixture::new("quiet");
        let specs = &f.specs;
        let check = |line: &str| {
            let cursor = line.chars().count();
            let ctx = analyze(line, cursor, specs);
            let env = Env {
                specs,
                commands: &f.commands,
                cwd: &f.dir.0,
                home: None,
                generated: &|_| None,
            };
            worth_showing(&ctx, &complete(&ctx, &env))
        };
        assert!(!check("git status"));
        assert!(check("git sta"));
        assert!(!check(""));
        assert!(!check("git commit -m \"fix"));
        assert!(!check("ln -s a "));
    }

    #[test]
    fn escaping() {
        assert_eq!(escape("a b$c", None, true), r"a\ b\$c");
        assert_eq!(escape("#x~", None, true), r"\#x~");
        assert_eq!(escape("it's", Some('\''), true), r"'it'\''s'");
        assert_eq!(escape("say \"hi\"", Some('"'), true), r#""say \"hi\"""#);
    }
}
