//! End-to-end tests: the real terme binary, running real bash and zsh, in a
//! pseudo-terminal. The test plays the terminal: it keeps a vt100 screen of
//! everything terme prints, types keys, and waits for the screen to change.

use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use portable_pty::{Child, CommandBuilder, MasterPty, PtySize, native_pty_system};

const ROWS: u16 = 24;
const COLS: u16 = 80;
const TIMEOUT: Duration = Duration::from_secs(10);

struct Term {
    screen: Arc<Mutex<vt100::Parser>>,
    raw: Arc<Mutex<Vec<u8>>>,
    writer: Box<dyn Write + Send>,
    child: Box<dyn Child + Send + Sync>,
    _master: Box<dyn MasterPty + Send>,
    home: tempfile::TempDir,
    shell: String,
}

fn find(shell: &str) -> Option<PathBuf> {
    ["/bin", "/usr/bin", "/usr/local/bin"]
        .iter()
        .map(|d| Path::new(d).join(shell))
        .find(|p| p.exists())
}

impl Term {
    /// Start terme with `shell`, or `None` (test skipped) if it isn't installed.
    fn start(shell: &str) -> Option<Self> {
        let Some(shell_path) = find(shell) else {
            eprintln!("skipping: {shell} is not installed");
            return None;
        };
        let home = tempfile::tempdir().unwrap();
        let rc = match shell {
            "bash" => (
                ".bashrc",
                "PS1='$ '\nunset PROMPT_COMMAND\nHISTFILE=/dev/null\n",
            ),
            _ => (".zshrc", "PROMPT='$ '\nRPROMPT=\nunsetopt BEEP\n"),
        };
        std::fs::write(home.path().join(rc.0), rc.1).unwrap();
        for f in ["backup.tar.gz", "notes.txt"] {
            std::fs::write(home.path().join(f), "x").unwrap();
        }
        std::fs::create_dir(home.path().join("old")).unwrap();

        let pair = native_pty_system()
            .openpty(PtySize {
                rows: ROWS,
                cols: COLS,
                pixel_width: 0,
                pixel_height: 0,
            })
            .unwrap();
        let mut cmd = CommandBuilder::new(env!("CARGO_BIN_EXE_terme"));
        cmd.cwd(home.path());
        cmd.env("HOME", home.path());
        cmd.env("TERME_SHELL", &shell_path);
        cmd.env("TERM", "xterm-256color");
        cmd.env("XDG_RUNTIME_DIR", home.path());
        cmd.env_remove("TERME");
        cmd.env_remove("TERME_TTY");
        cmd.env_remove("ZDOTDIR");
        let child = pair.slave.spawn_command(cmd).unwrap();
        drop(pair.slave);

        let screen = Arc::new(Mutex::new(vt100::Parser::new(
            ROWS.try_into().unwrap(),
            COLS.try_into().unwrap(),
            1000,
        )));
        let raw = Arc::new(Mutex::new(Vec::new()));
        let mut reader = pair.master.try_clone_reader().unwrap();
        let (s, r) = (screen.clone(), raw.clone());
        std::thread::spawn(move || {
            let mut buf = [0u8; 8192];
            while let Ok(n) = reader.read(&mut buf) {
                if n == 0 {
                    break;
                }
                s.lock().unwrap().process(&buf[..n]);
                r.lock().unwrap().extend_from_slice(&buf[..n]);
            }
        });
        let writer = pair.master.take_writer().unwrap();
        let term = Self {
            screen,
            raw,
            writer,
            child,
            _master: pair.master,
            home,
            shell: shell.to_string(),
        };
        term.wait_for("the first prompt", |t| t.cursor_line().starts_with("$ "));
        Some(term)
    }

    fn keys(&mut self, bytes: &[u8]) {
        self.writer.write_all(bytes).unwrap();
        self.writer.flush().unwrap();
    }

    /// Type text one key at a time, like a fast typist.
    fn type_text(&mut self, text: &str) {
        for c in text.chars() {
            self.keys(c.to_string().as_bytes());
            std::thread::sleep(Duration::from_millis(15));
        }
    }

    fn text(&self) -> String {
        let p = self.screen.lock().unwrap();
        p.screen().contents()
    }

    fn cursor_line(&self) -> String {
        let p = self.screen.lock().unwrap();
        let (row, _) = p.screen().cursor_position();
        p.screen()
            .rows(0, COLS)
            .nth(usize::from(row))
            .unwrap_or_default()
    }

    fn popup_visible(&self) -> bool {
        self.text().contains('╭')
    }

    fn wait_for(&self, what: &str, check: impl Fn(&Self) -> bool) {
        let start = Instant::now();
        while start.elapsed() < TIMEOUT {
            if check(self) {
                return;
            }
            std::thread::sleep(Duration::from_millis(10));
        }
        let raw = self.raw.lock().unwrap();
        let tail = &raw[raw.len().saturating_sub(1500)..];
        panic!(
            "[{}] timed out waiting for {what}. Screen:\n{}\nLast bytes from terme ({} in all):\n{}",
            self.shell,
            self.text(),
            raw.len(),
            String::from_utf8_lossy(tail).escape_debug()
        );
    }

    /// Stay a while and check that something never happens.
    fn never(&self, what: &str, ms: u64, check: impl Fn(&Self) -> bool) {
        let start = Instant::now();
        while start.elapsed() < Duration::from_millis(ms) {
            assert!(
                !check(self),
                "[{}] {what}. Screen:\n{}",
                self.shell,
                self.text()
            );
            std::thread::sleep(Duration::from_millis(10));
        }
    }

    /// Every line that scrolled off the top, oldest first.
    fn scrollback(&self) -> Vec<String> {
        let mut p = self.screen.lock().unwrap();
        let mut lines = Vec::new();
        let mut offset = 0;
        loop {
            p.screen_mut().set_scrollback(offset + 1);
            if p.screen().scrollback() == offset {
                break;
            }
            offset = p.screen().scrollback();
            lines.push(p.screen().rows(0, COLS).next().unwrap_or_default());
        }
        p.screen_mut().set_scrollback(0);
        lines.reverse();
        lines
    }

    fn clear_line(&mut self) {
        self.keys(b"\x03");
        self.wait_for("a fresh prompt", |t| {
            t.cursor_line().trim_end() == "$" && !t.popup_visible()
        });
    }
}

impl Drop for Term {
    fn drop(&mut self) {
        let _ = self.child.kill();
    }
}

fn each_shell(test: impl Fn(&mut Term)) {
    let mut ran = 0;
    for shell in ["bash", "zsh"] {
        if let Some(mut t) = Term::start(shell) {
            test(&mut t);
            ran += 1;
        }
    }
    assert!(ran > 0, "neither bash nor zsh is installed");
}

#[test]
fn list_appears_after_a_pause_and_tab_inserts() {
    each_shell(|t| {
        t.type_text("git c");
        t.wait_for("the subcommand list", |t| {
            t.text().contains("╭─ subcommand") && t.text().contains("commit")
        });
        t.keys(b"\t");
        t.wait_for("the inserted subcommand", |t| {
            t.cursor_line().starts_with("$ git commit ")
        });
        t.clear_line();
    });
}

#[test]
fn enter_runs_exactly_what_was_typed() {
    each_shell(|t| {
        t.type_text("git sta");
        t.wait_for("the list", Term::popup_visible);
        t.keys(b"\r");
        t.wait_for("git's own error for 'sta'", |t| {
            t.text().contains("'sta' is not a git command")
        });
        assert!(!t.popup_visible());
    });
}

#[test]
fn down_then_enter_inserts_without_running() {
    each_shell(|t| {
        t.type_text("git sta");
        t.wait_for("the list", Term::popup_visible);
        t.keys(b"\x1b[B");
        t.wait_for("the focused list", |t| t.text().contains('↵'));
        t.keys(b"\r");
        t.wait_for("the inserted subcommand", |t| {
            t.cursor_line().starts_with("$ git status ")
        });
        t.never("the command ran", 300, |t| {
            t.text().contains("not a git repository")
        });
        t.clear_line();
    });
}

#[test]
fn esc_hides_the_list_until_the_next_word() {
    each_shell(|t| {
        t.type_text("git sta");
        t.wait_for("the list", Term::popup_visible);
        t.keys(b"\x1b");
        t.wait_for("the list to close", |t| !t.popup_visible());
        t.type_text("t");
        t.never(
            "the list came back within the same word",
            400,
            Term::popup_visible,
        );
        t.clear_line();
    });
}

#[test]
fn files_values_flow_from_options() {
    each_shell(|t| {
        t.type_text("tar -xz");
        t.wait_for("the cluster list", |t| t.text().contains("add to -xz"));
        t.keys(b"\t");
        t.wait_for("archives first", |t| {
            t.text().contains("archive to read") && t.text().contains("▏▤ backup.tar.gz")
        });
        t.keys(b"\t");
        t.wait_for("the archive inserted", |t| {
            t.cursor_line().starts_with("$ tar -xzf backup.tar.gz ")
        });
        t.clear_line();
    });
}

#[test]
fn popup_above_the_prompt_restores_the_output_under_it() {
    each_shell(|t| {
        t.type_text("seq 1 40\r");
        t.wait_for("the output and a prompt at the bottom", |t| {
            t.text().contains("40\n$") && t.cursor_line().trim_end() == "$"
        });
        let before = t.text();
        t.type_text("git c");
        t.wait_for("the list above the prompt", Term::popup_visible);
        t.keys(b"\x1b");
        t.wait_for("the list to close", |t| !t.popup_visible());
        let after = t.text();
        assert_eq!(
            after.replace("$ git c", "$"),
            before.replace("$ ", "$"),
            "[{}] output under the popup changed",
            t.shell
        );
        t.clear_line();
    });
}

#[test]
fn nothing_reaches_the_scrollback() {
    each_shell(|t| {
        for _ in 0..3 {
            t.type_text("git c");
            t.wait_for("the list", Term::popup_visible);
            t.keys(b"\x03");
            t.wait_for("the list to close", |t| !t.popup_visible());
            t.type_text("seq 1 30\r");
            t.wait_for("the output", |t| t.text().contains("30\n$"));
        }
        let scrollback = t.scrollback();
        assert!(scrollback.len() > 20, "expected scrolling");
        for line in &scrollback {
            assert!(
                !line.contains(['╭', '╰', '│', '▏']),
                "[{}] popup in the scrollback: {line:?}",
                t.shell
            );
        }
        let raw = t.raw.lock().unwrap();
        assert!(
            !raw.windows(5).any(|w| w == b"]6973"),
            "a private message reached the terminal"
        );
    });
}

#[test]
fn quiet_while_a_command_runs() {
    each_shell(|t| {
        t.type_text("cat\r");
        std::thread::sleep(Duration::from_millis(200));
        t.type_text("git c");
        t.never(
            "a list appeared while cat was running",
            500,
            Term::popup_visible,
        );
        t.keys(b"\x03");
        t.wait_for("the prompt again", |t| t.cursor_line().trim_end() == "$");
    });
}

#[test]
fn exit_status_is_passed_on() {
    each_shell(|t| {
        t.type_text("exit 3\r");
        let start = Instant::now();
        let status = loop {
            if let Some(status) = t.child.try_wait().unwrap() {
                break status;
            }
            assert!(start.elapsed() < TIMEOUT, "terme didn't exit");
            std::thread::sleep(Duration::from_millis(20));
        };
        assert_eq!(status.exit_code(), 3, "[{}]", t.shell);
        // The hook folder is cleaned up.
        let left: Vec<_> = std::fs::read_dir(t.home.path())
            .unwrap()
            .flatten()
            .filter(|e| e.file_name().to_string_lossy().starts_with("terme-"))
            .collect();
        assert!(left.is_empty(), "[{}] leftover {left:?}", t.shell);
    });
}
