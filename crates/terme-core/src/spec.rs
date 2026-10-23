//! Completion specs: what a command accepts, in a small JSON format inspired
//! by the Fig specs.

use serde::Deserialize;

/// A command or subcommand.
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Command {
    /// The name first, then any aliases.
    pub names: Vec<String>,
    pub description: String,
    pub subcommands: Vec<Command>,
    pub options: Vec<Opt>,
    pub args: Vec<Arg>,
    /// Higher comes first among equally good matches. 0 by default.
    pub priority: i32,
}

/// An option such as `-x, --extract`.
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Opt {
    /// Every spelling: short ones like `-x` and long ones like `--extract`.
    pub names: Vec<String>,
    pub description: String,
    /// The value the option takes, if any.
    pub arg: Option<Arg>,
    /// Long form must be written `--opt=VALUE` rather than `--opt VALUE`.
    pub requires_equals: bool,
    /// May appear more than once (`-v -v`).
    pub repeatable: bool,
    pub priority: i32,
    pub deprecated: bool,
    /// Names of options that can't be used together with this one.
    pub exclusive: Vec<String>,
}

/// A positional argument or an option's value.
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Arg {
    pub name: String,
    pub description: String,
    pub optional: bool,
    /// Can repeat until the end of the command (`FILE...`).
    pub variadic: bool,
    /// Built-in value sources.
    pub template: Option<Template>,
    /// File extensions listed first when suggesting files (`.tgz`).
    pub extensions: Vec<String>,
    /// A fixed set of values.
    pub suggestions: Vec<Suggestion>,
    /// Values produced by running a program.
    pub generator: Option<Generator>,
    /// The path doesn't have to exist yet (`ln -s TARGET LINK_NAME`), so stay
    /// quiet until the user starts typing a path.
    pub new_path: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Template {
    /// Files and folders.
    Filepaths,
    /// Folders only.
    Folders,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Suggestion {
    pub name: String,
    pub description: String,
    pub priority: i32,
}

/// Runs `script` in the shell's folder and turns each output line into a value.
#[derive(Debug, Clone, Default, PartialEq, Eq, Hash, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Generator {
    pub script: Vec<String>,
    /// How the values are labelled in the list (`branch`, `unit`...).
    pub kind: String,
    /// Keep only this whitespace-separated column of each line (0-based).
    pub field: Option<usize>,
}

impl Command {
    #[must_use]
    pub fn name(&self) -> &str {
        self.names.first().map_or("", String::as_str)
    }

    #[must_use]
    pub fn find_subcommand(&self, word: &str) -> Option<&Command> {
        self.subcommands
            .iter()
            .find(|c| c.names.iter().any(|n| n == word))
    }

    /// Find an option by one of its exact names.
    #[must_use]
    pub fn find_option(&self, name: &str) -> Option<&Opt> {
        self.options
            .iter()
            .find(|o| o.names.iter().any(|n| n == name))
    }

    /// Whether every char after the dash is a known short flag, as in `-xzf`.
    #[must_use]
    pub fn is_short_cluster(&self, word: &str) -> bool {
        let Some(rest) = word.strip_prefix('-') else {
            return false;
        };
        !rest.is_empty()
            && !rest.starts_with('-')
            && rest
                .chars()
                .all(|c| self.find_option(&format!("-{c}")).is_some())
    }
}

impl Opt {
    #[must_use]
    pub fn short(&self) -> Option<&str> {
        self.names
            .iter()
            .map(String::as_str)
            .find(|n| n.len() == 2 && n.starts_with('-') && !n.starts_with("--"))
    }

    #[must_use]
    pub fn long(&self) -> Option<&str> {
        self.names
            .iter()
            .map(String::as_str)
            .find(|n| n.starts_with("--"))
    }

    #[must_use]
    pub fn takes_value(&self) -> bool {
        self.arg.is_some()
    }

    /// Whether this option can't be combined with `other`, in either direction.
    #[must_use]
    pub fn conflicts_with(&self, other: &Opt) -> bool {
        self.exclusive.iter().any(|n| other.names.contains(n))
            || other.exclusive.iter().any(|n| self.names.contains(n))
    }
}

/// The specs bundled with terme.
pub struct SpecSet {
    commands: Vec<Command>,
}

const BUILTIN: &[(&str, &str)] = &[
    ("apt", include_str!("../specs/apt.json")),
    ("cd", include_str!("../specs/cd.json")),
    ("cp", include_str!("../specs/cp.json")),
    ("docker", include_str!("../specs/docker.json")),
    ("git", include_str!("../specs/git.json")),
    ("grep", include_str!("../specs/grep.json")),
    ("ls", include_str!("../specs/ls.json")),
    ("ssh", include_str!("../specs/ssh.json")),
    ("systemctl", include_str!("../specs/systemctl.json")),
    ("tar", include_str!("../specs/tar.json")),
];

impl SpecSet {
    /// Load the bundled specs.
    ///
    /// # Panics
    /// If a bundled spec is malformed; the test suite checks they all parse.
    #[must_use]
    pub fn builtin() -> Self {
        let commands = BUILTIN
            .iter()
            .map(|(name, json)| {
                serde_json::from_str(json)
                    .unwrap_or_else(|e| panic!("bundled spec {name}.json is invalid: {e}"))
            })
            .collect();
        Self { commands }
    }

    #[must_use]
    pub fn from_commands(commands: Vec<Command>) -> Self {
        Self { commands }
    }

    /// Add a spec learned at run time (from a man page). Bundled specs win:
    /// a command that already has one is left alone.
    pub fn insert(&mut self, command: Command) {
        if self.get(command.name()).is_none() {
            self.commands.push(command);
        }
    }

    #[must_use]
    pub fn get(&self, name: &str) -> Option<&Command> {
        self.commands
            .iter()
            .find(|c| c.names.iter().any(|n| n == name))
    }

    pub fn iter(&self) -> impl Iterator<Item = &Command> {
        self.commands.iter()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bundled_specs_parse_and_are_named_after_their_file() {
        let set = SpecSet::builtin();
        for (name, _) in BUILTIN {
            let spec = set.get(name).unwrap_or_else(|| panic!("{name} missing"));
            assert_eq!(spec.name(), *name);
        }
    }

    #[test]
    fn option_names_are_well_formed() {
        fn check(cmd: &Command, path: &str) {
            for opt in &cmd.options {
                assert!(!opt.names.is_empty(), "{path}: option without names");
                for n in &opt.names {
                    assert!(n.starts_with('-'), "{path}: option name {n:?}");
                }
                assert!(
                    !opt.description.is_empty(),
                    "{path} {:?}: no description",
                    opt.names
                );
            }
            for sub in &cmd.subcommands {
                assert!(
                    !sub.description.is_empty(),
                    "{path} {}: no description",
                    sub.name()
                );
                check(sub, &format!("{path} {}", sub.name()));
            }
        }
        for cmd in SpecSet::builtin().iter() {
            check(cmd, cmd.name());
        }
    }

    #[test]
    fn short_clusters() {
        let tar = SpecSet::builtin();
        let tar = tar.get("tar").unwrap();
        assert!(tar.is_short_cluster("-xzf"));
        assert!(!tar.is_short_cluster("--xz"));
        assert!(!tar.is_short_cluster("-"));
        assert!(!tar.is_short_cluster("-x@"));
    }
}
