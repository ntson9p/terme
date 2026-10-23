//! Things suggestions need from the system: command names on `PATH`, the
//! shell's folder, and generator output, run in the background with a cache.

use std::collections::{HashMap, HashSet};
use std::io::Read;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::mpsc::{Receiver, Sender, channel};
use std::sync::{Arc, Mutex, OnceLock};
use std::thread;
use std::time::{Duration, Instant};

use terme_core::man::{self, ManPage};
use terme_core::spec::Generator;

/// Everything outside the command line that suggestions draw on. The slow
/// parts fill in from background threads.
pub struct Sources {
    /// Executables on `PATH` and builtins.
    pub commands: Arc<OnceLock<Vec<String>>>,
    /// One-line descriptions of commands, from `apropos`.
    pub summaries: Arc<OnceLock<HashMap<String, String>>>,
    pub generators: Generators,
    pub man: ManPages,
    /// The shell's working folder.
    pub cwd: Box<dyn Fn() -> Option<PathBuf> + Send>,
    pub home: Option<PathBuf>,
}

const BUILTINS: &[&str] = &[
    "alias", "bg", "builtin", "cd", "command", "declare", "dirs", "disown", "echo", "eval", "exec",
    "exit", "export", "false", "fc", "fg", "hash", "help", "history", "jobs", "kill", "let",
    "local", "popd", "printf", "pushd", "pwd", "read", "readonly", "return", "set", "shift",
    "source", "test", "time", "trap", "true", "type", "typeset", "ulimit", "umask", "unalias",
    "unset", "wait",
];

/// Executables on `PATH` plus shell builtins, sorted and unique.
#[must_use]
pub fn command_names() -> Vec<String> {
    let mut names: HashSet<String> = BUILTINS.iter().map(|s| (*s).to_string()).collect();
    if let Some(path) = std::env::var_os("PATH") {
        for dir in std::env::split_paths(&path) {
            let Ok(entries) = std::fs::read_dir(&dir) else {
                continue;
            };
            for entry in entries.flatten() {
                // Follow symlinks: /usr/bin/sh is usually one.
                let executable = std::fs::metadata(entry.path())
                    .is_ok_and(|m| m.is_file() && m.permissions().mode() & 0o111 != 0);
                if executable {
                    names.insert(entry.file_name().to_string_lossy().into_owned());
                }
            }
        }
    }
    let mut v: Vec<String> = names.into_iter().collect();
    v.sort();
    v
}

/// The working folder of a process (Linux).
#[must_use]
pub fn cwd_of(pid: u32) -> Option<PathBuf> {
    std::fs::read_link(format!("/proc/{pid}/cwd")).ok()
}

/// How long generator output is reused.
const FRESH_FOR: Duration = Duration::from_secs(30);
/// Generators that take longer than this are killed.
const TIMEOUT: Duration = Duration::from_secs(2);
const MAX_VALUES: usize = 100_000;

type Key = (PathBuf, Generator);

struct Entry {
    values: Arc<Vec<String>>,
    at: Instant,
}

/// Runs generators on a worker thread and caches what they print.
pub struct Generators {
    cache: Arc<Mutex<HashMap<Key, Entry>>>,
    running: Arc<Mutex<HashSet<Key>>>,
    jobs: Sender<Key>,
}

impl Generators {
    /// `done` is called on the worker thread whenever new values are ready.
    pub fn new(done: impl Fn() + Send + 'static) -> Self {
        let (jobs, rx): (Sender<Key>, Receiver<Key>) = channel();
        let cache: Arc<Mutex<HashMap<Key, Entry>>> = Arc::default();
        let running: Arc<Mutex<HashSet<Key>>> = Arc::default();
        let (c, r) = (cache.clone(), running.clone());
        thread::Builder::new()
            .name("terme-generators".into())
            .spawn(move || {
                for key in rx {
                    let values = run(&key.0, &key.1);
                    c.lock().expect("cache lock").insert(
                        key.clone(),
                        Entry {
                            values: Arc::new(values),
                            at: Instant::now(),
                        },
                    );
                    r.lock().expect("running lock").remove(&key);
                    done();
                }
            })
            .expect("spawning the generator thread");
        Self {
            cache,
            running,
            jobs,
        }
    }

    /// Cached values, if fresh.
    #[must_use]
    pub fn get(&self, cwd: &Path, generator: &Generator) -> Option<Arc<Vec<String>>> {
        let cache = self.cache.lock().expect("cache lock");
        let entry = cache.get(&(cwd.to_path_buf(), generator.clone()))?;
        (entry.at.elapsed() < FRESH_FOR).then(|| entry.values.clone())
    }

    /// Run the generator in the background unless it's already running.
    pub fn request(&self, cwd: &Path, generator: &Generator) {
        let key = (cwd.to_path_buf(), generator.clone());
        if self
            .running
            .lock()
            .expect("running lock")
            .insert(key.clone())
        {
            let _ = self.jobs.send(key);
        }
    }
}

fn run(cwd: &Path, generator: &Generator) -> Vec<String> {
    let Some((program, args)) = generator.script.split_first() else {
        return Vec::new();
    };
    let child = Command::new(program)
        .args(args)
        .current_dir(cwd)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn();
    let Ok(mut child) = child else {
        return Vec::new();
    };
    let mut stdout = child.stdout.take().expect("piped stdout");
    let reader = thread::spawn(move || {
        let mut buf = Vec::new();
        let _ = stdout.read_to_end(&mut buf);
        buf
    });
    let started = Instant::now();
    loop {
        match child.try_wait() {
            Ok(Some(_)) => break,
            Ok(None) if started.elapsed() > TIMEOUT => {
                let _ = child.kill();
                let _ = child.wait();
                break;
            }
            Ok(None) => thread::sleep(Duration::from_millis(5)),
            Err(_) => break,
        }
    }
    let output = reader.join().unwrap_or_default();
    parse(&String::from_utf8_lossy(&output), generator.field)
}

fn parse(output: &str, field: Option<usize>) -> Vec<String> {
    let mut seen = HashSet::new();
    output
        .lines()
        .filter_map(|line| match field {
            Some(n) => line.split_whitespace().nth(n),
            None => Some(line.trim()),
        })
        .filter(|v| !v.is_empty() && seen.insert(v.to_string()))
        .take(MAX_VALUES)
        .map(String::from)
        .collect()
}

/// Rendered and parsed man pages, loaded on a worker thread.
pub struct ManPages {
    /// `None` inside: looked up, but there's no page.
    cache: Arc<Mutex<HashMap<String, Option<Arc<ManPage>>>>>,
    running: Arc<Mutex<HashSet<String>>>,
    jobs: Sender<String>,
}

impl ManPages {
    /// `done` is called on the worker thread whenever a page is ready.
    pub fn new(done: impl Fn() + Send + 'static) -> Self {
        let (jobs, rx): (Sender<String>, Receiver<String>) = channel();
        let cache: Arc<Mutex<HashMap<String, Option<Arc<ManPage>>>>> = Arc::default();
        let running: Arc<Mutex<HashSet<String>>> = Arc::default();
        let (c, r) = (cache.clone(), running.clone());
        thread::Builder::new()
            .name("terme-man".into())
            .spawn(move || {
                for name in rx {
                    let page = render_man(&name).map(|text| Arc::new(man::parse(&text)));
                    c.lock().expect("man cache lock").insert(name.clone(), page);
                    r.lock().expect("man running lock").remove(&name);
                    done();
                }
            })
            .expect("spawning the man page thread");
        Self {
            cache,
            running,
            jobs,
        }
    }

    /// A store that never runs `man`: it only knows the pages given to it
    /// (now or later with [`ManPages::add`]). For tests.
    #[cfg(test)]
    pub fn offline(pages: Vec<(&str, Option<ManPage>)>) -> Self {
        let (jobs, _rx) = channel();
        let s = Self {
            cache: Arc::default(),
            // Mark everything as already running so nothing is ever sent.
            running: Arc::default(),
            jobs,
        };
        for (name, page) in pages {
            s.add(name, page);
        }
        s
    }

    #[cfg(test)]
    pub fn add(&self, name: &str, page: Option<ManPage>) {
        self.cache
            .lock()
            .expect("man cache lock")
            .insert(name.to_string(), page.map(Arc::new));
    }

    /// `None` while not loaded yet; `Some(None)` if there is no page.
    #[must_use]
    pub fn get(&self, name: &str) -> Option<Option<Arc<ManPage>>> {
        self.cache
            .lock()
            .expect("man cache lock")
            .get(name)
            .cloned()
    }

    /// Load the page in the background unless it's loaded or loading.
    pub fn request(&self, name: &str) {
        if !is_page_name(name)
            || self
                .cache
                .lock()
                .expect("man cache lock")
                .contains_key(name)
        {
            return;
        }
        if self
            .running
            .lock()
            .expect("man running lock")
            .insert(name.to_string())
        {
            let _ = self.jobs.send(name.to_string());
        }
    }
}

/// Only plain command names are looked up.
fn is_page_name(name: &str) -> bool {
    !name.is_empty()
        && name.len() < 100
        && !name.starts_with('-')
        && name
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.' | '+'))
}

/// The page as `man` renders it for a wide terminal, from the sections for
/// commands (1), admin commands (8) and games (6).
fn render_man(name: &str) -> Option<String> {
    let child = Command::new("man")
        .args(["-S", "1:8:6", "-P", "cat", name])
        .env("MANWIDTH", "200")
        .env("MANPAGER", "cat")
        .env_remove("MAN_KEEP_FORMATTING")
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .ok()?;
    let output = wait_with_timeout(child, Duration::from_secs(5))?;
    let text = String::from_utf8_lossy(&output).into_owned();
    (!text.trim().is_empty()).then_some(text)
}

fn wait_with_timeout(mut child: std::process::Child, timeout: Duration) -> Option<Vec<u8>> {
    let mut stdout = child.stdout.take()?;
    let reader = thread::spawn(move || {
        let mut buf = Vec::new();
        let _ = stdout.read_to_end(&mut buf);
        buf
    });
    let started = Instant::now();
    let ok = loop {
        match child.try_wait() {
            Ok(Some(status)) => break status.success(),
            Ok(None) if started.elapsed() > timeout => {
                let _ = child.kill();
                let _ = child.wait();
                break false;
            }
            Ok(None) => thread::sleep(Duration::from_millis(5)),
            Err(_) => break false,
        }
    };
    let output = reader.join().unwrap_or_default();
    ok.then_some(output)
}

/// One-line descriptions of every command that has a man page, from
/// `apropos` (about half a second; run it off the main thread).
#[must_use]
pub fn command_summaries() -> HashMap<String, String> {
    let child = Command::new("apropos")
        .args(["-s", "1,8,6", "."])
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn();
    let Some(output) = child
        .ok()
        .and_then(|c| wait_with_timeout(c, Duration::from_secs(10)))
    else {
        return HashMap::new();
    };
    let mut map = HashMap::new();
    for (name, summary) in man::parse_whatis(&String::from_utf8_lossy(&output)) {
        map.entry(name)
            .or_insert_with(|| man::short_phrase(&summary));
    }
    map
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_lines_and_fields() {
        assert_eq!(parse("a\n b \n\na\n", None), ["a", "b"]);
        assert_eq!(
            parse("nginx.service loaded active\nssh.service loaded\n", Some(0)),
            ["nginx.service", "ssh.service"]
        );
    }

    #[test]
    fn runs_in_the_background_and_caches() {
        let (tx, rx) = channel();
        let gens = Generators::new(move || tx.send(()).unwrap());
        let g = Generator {
            script: vec!["printf".into(), "one\\ntwo\\n".into()],
            kind: "test".into(),
            field: None,
        };
        let cwd = std::env::temp_dir();
        assert!(gens.get(&cwd, &g).is_none());
        gens.request(&cwd, &g);
        rx.recv_timeout(Duration::from_secs(5)).unwrap();
        assert_eq!(*gens.get(&cwd, &g).unwrap(), ["one", "two"]);
    }

    #[test]
    fn finds_some_commands() {
        let names = command_names();
        assert!(names.contains(&"cd".to_string()));
        assert!(names.contains(&"sh".to_string()));
    }
}
