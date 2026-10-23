//! The state machine between the user, the shell and the terminal (UI spec §7).
//!
//! `App` does no I/O. It takes what arrived (keys from the user, output from
//! the shell, the time) and queues what to send: bytes for the real terminal
//! and keystrokes for the shell. That keeps it testable without a terminal.

use std::collections::VecDeque;
use std::path::PathBuf;
use std::sync::{Arc, OnceLock};
use std::time::{Duration, Instant};

use terme_core::context::Word;
use terme_core::edit;
use terme_core::{Candidate, Env, Kind, SpecSet, analyze, complete, worth_showing};
use terme_ui::{Anchor, Item, KindColor, Overlay, PopupView, Theme};

use crate::keys::{Key, KeyEvent, KeyParser};
use crate::osc::{Filter, Piece};
use crate::shadow::{Mark, Shadow};
use crate::shell::ShellKind;
use crate::sources::Generators;

#[derive(Debug, Clone)]
pub struct Config {
    /// Pause before the list opens on its own; once open it updates at once.
    pub open_delay: Duration,
    pub max_rows: usize,
    pub theme: Theme,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            open_delay: Duration::from_millis(100),
            max_rows: 8,
            theme: Theme::Ansi16,
        }
    }
}

/// How long a lone Esc waits to see whether it starts an Alt+key sequence.
const ESC_WAIT: Duration = Duration::from_millis(25);
/// How long held-back output waits for the rest of a possible message.
const OUTPUT_WAIT: Duration = Duration::from_millis(20);
/// How long keys that need an up-to-date line wait for the shell's echo.
const ECHO_WAIT: Duration = Duration::from_millis(150);

struct Popup {
    items: Vec<Candidate>,
    label: String,
    selected: usize,
    /// The user moved into the list with ↓, so Enter inserts.
    focused: bool,
    /// Char index of the word the suggestions are for.
    word_start: usize,
}

pub struct App {
    shell: ShellKind,
    shadow: Shadow,
    filter: Filter,
    keys: KeyParser,
    specs: SpecSet,
    commands: Arc<OnceLock<Vec<String>>>,
    home: Option<PathBuf>,
    generators: Generators,
    cwd: Box<dyn Fn() -> Option<PathBuf> + Send>,
    config: Config,

    /// Between a prompt's end mark and the next command start.
    at_prompt: bool,
    /// Screen cell where the command line starts.
    input_start: Option<(u16, u16)>,
    /// The line and cursor zsh reported (bash's are read from the screen).
    reported: Option<(String, usize)>,
    vi_command_mode: bool,

    popup: Option<Popup>,
    drawn: Option<Overlay>,
    /// Esc hid the list for the word starting here.
    suppressed_word: Option<usize>,
    open_at: Option<Instant>,
    /// The last change to the line came from typing.
    typed: bool,
    /// Show the next list as soon as the shell echoes an insert.
    open_after_echo: bool,
    awaiting_echo: Option<Instant>,
    deferred: VecDeque<KeyEvent>,
    esc_deadline: Option<Instant>,
    output_deadline: Option<Instant>,

    to_terminal: Vec<u8>,
    to_shell: Vec<u8>,
}

impl App {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        shell: ShellKind,
        size: (u16, u16),
        specs: SpecSet,
        commands: Arc<OnceLock<Vec<String>>>,
        generators: Generators,
        cwd: Box<dyn Fn() -> Option<PathBuf> + Send>,
        home: Option<PathBuf>,
        config: Config,
    ) -> Self {
        Self {
            shell,
            shadow: Shadow::new(size.0, size.1),
            filter: Filter::default(),
            keys: KeyParser::default(),
            specs,
            commands,
            home,
            generators,
            cwd,
            config,
            at_prompt: false,
            input_start: None,
            reported: None,
            vi_command_mode: false,
            popup: None,
            drawn: None,
            suppressed_word: None,
            open_at: None,
            typed: false,
            open_after_echo: false,
            awaiting_echo: None,
            deferred: VecDeque::new(),
            esc_deadline: None,
            output_deadline: None,
            to_terminal: Vec::new(),
            to_shell: Vec::new(),
        }
    }

    /// Bytes to write to the real terminal.
    pub fn take_terminal_output(&mut self) -> Vec<u8> {
        std::mem::take(&mut self.to_terminal)
    }

    /// Bytes to write to the shell.
    pub fn take_shell_input(&mut self) -> Vec<u8> {
        std::mem::take(&mut self.to_shell)
    }

    /// The earliest time [`App::on_tick`] has something to do.
    #[must_use]
    pub fn next_deadline(&self) -> Option<Instant> {
        [
            self.esc_deadline,
            self.output_deadline,
            self.open_at,
            self.awaiting_echo.map(|t| t + ECHO_WAIT),
        ]
        .into_iter()
        .flatten()
        .min()
    }

    /// Whether suggestions apply right now.
    fn active(&self) -> bool {
        self.at_prompt && self.input_start.is_some() && !self.shadow.screen().alternate_screen()
    }

    // ----- output from the shell -------------------------------------------

    pub fn on_output(&mut self, bytes: &[u8], now: Instant) {
        let pieces = self.filter.feed(bytes);
        self.output_deadline = self.filter.has_pending().then(|| now + OUTPUT_WAIT);
        self.apply_output(pieces, now);
    }

    fn apply_output(&mut self, pieces: Vec<Piece>, now: Instant) {
        if pieces.is_empty() {
            return;
        }
        // Take the popup off the screen first, so the shell draws on a clean
        // copy and nothing of ours can scroll into the scrollback.
        self.erase_overlay();
        for piece in pieces {
            match piece {
                Piece::Text(text) => {
                    self.shadow.process(&text);
                    self.to_terminal.extend_from_slice(&text);
                    self.handle_marks();
                }
                Piece::Message(message) => self.handle_message(&message),
            }
        }
        self.awaiting_echo = None;
        self.after_line_change(now);
        self.run_deferred(now);
    }

    fn handle_marks(&mut self) {
        for mark in self.shadow.take_marks() {
            match mark {
                Mark::InputStart { row, col } => {
                    self.at_prompt = true;
                    self.input_start = Some((row, col));
                    self.reported = None;
                    self.popup = None;
                    self.suppressed_word = None;
                    self.open_at = None;
                    self.typed = false;
                }
                Mark::CommandStart => {
                    self.at_prompt = false;
                    self.input_start = None;
                    self.popup = None;
                    self.open_at = None;
                    self.typed = false;
                }
                Mark::Scrolled => {
                    self.input_start = self.input_start.and_then(|(r, c)| Some((r.checked_sub(1)?, c)));
                }
                Mark::PromptStart | Mark::CommandDone => {}
            }
        }
    }

    fn handle_message(&mut self, message: &[u8]) {
        let text = String::from_utf8_lossy(message);
        let mut parts = text.splitn(3, ';');
        match parts.next() {
            Some("line") => {
                let cursor = parts.next().and_then(|c| c.parse().ok());
                let line = parts.next().unwrap_or_default().to_string();
                if let Some(cursor) = cursor {
                    self.reported = Some((line, cursor));
                }
            }
            Some("keymap") => self.vi_command_mode = parts.next() == Some("vicmd"),
            _ => {}
        }
    }

    /// The command line and the cursor's char index in it.
    fn line(&self) -> Option<(String, usize)> {
        if !self.active() {
            return None;
        }
        match self.shell {
            ShellKind::Zsh => self.reported.clone(),
            ShellKind::Bash => self.shadow.read_line(self.input_start?),
        }
    }

    fn after_line_change(&mut self, now: Instant) {
        if !self.active() || self.vi_command_mode {
            self.close_popup();
            return;
        }
        if self.open_after_echo {
            self.open_after_echo = false;
            self.refresh(false);
            self.redraw();
        } else if self.popup.is_some() {
            self.refresh(false);
            self.redraw();
        } else if self.typed {
            self.open_at = Some(now + self.config.open_delay);
        }
    }

    // ----- input from the user ---------------------------------------------

    pub fn on_input(&mut self, bytes: &[u8], now: Instant) {
        let events = self.keys.feed(bytes);
        self.esc_deadline = self.keys.has_pending().then(|| now + ESC_WAIT);
        for event in events {
            self.on_key(event, now);
        }
    }

    fn needs_fresh_line(&self, key: Key) -> bool {
        matches!(key, Key::Tab | Key::CtrlSpace)
            || (key == Key::Enter && self.popup.as_ref().is_some_and(|p| p.focused))
    }

    fn on_key(&mut self, event: KeyEvent, now: Instant) {
        // Keep keys in order behind any that wait for the shell's echo.
        if !self.deferred.is_empty() || (self.awaiting_echo.is_some() && self.active() && self.needs_fresh_line(event.key))
        {
            self.deferred.push_back(event);
            return;
        }
        self.handle_key(event, now);
    }

    fn run_deferred(&mut self, now: Instant) {
        while let Some(event) = self.deferred.pop_front() {
            if self.awaiting_echo.is_some() && self.active() && self.needs_fresh_line(event.key) {
                self.deferred.push_front(event);
                return;
            }
            self.handle_key(event, now);
        }
    }

    fn send(&mut self, bytes: &[u8], now: Instant) {
        self.to_shell.extend_from_slice(bytes);
        if self.active() {
            self.awaiting_echo = Some(now);
        }
    }

    #[allow(clippy::too_many_lines)]
    fn handle_key(&mut self, event: KeyEvent, now: Instant) {
        if !self.active() || self.vi_command_mode {
            self.send(&event.raw, now);
            return;
        }
        let visible = self.popup.is_some();
        let focused = self.popup.as_ref().is_some_and(|p| p.focused);
        match event.key {
            Key::Paste => {
                self.close_popup();
                self.typed = false;
                self.send(&event.raw, now);
            }
            Key::Tab if visible => {
                let selected = self.popup.as_ref().map_or(0, |p| p.selected);
                self.accept(selected, now);
            }
            Key::Tab => {
                let suppressed = self.current_word().is_some_and(|w| self.suppressed_word == Some(w.start));
                if !suppressed {
                    self.refresh(true);
                }
                match self.popup.as_ref().map(|p| p.items.len()) {
                    Some(1) => self.accept(0, now),
                    Some(_) => self.redraw(),
                    // Nothing to offer, or Esc said "not now": the shell's own completion.
                    None => self.send(&event.raw, now),
                }
            }
            Key::Enter if focused => {
                let selected = self.popup.as_ref().map_or(0, |p| p.selected);
                self.accept(selected, now);
            }
            Key::Down if visible => {
                if let Some(p) = &mut self.popup {
                    if p.focused {
                        p.selected = (p.selected + 1).min(p.items.len() - 1);
                    } else {
                        p.focused = true;
                    }
                }
                self.redraw();
            }
            Key::Up if focused => {
                if let Some(p) = &mut self.popup {
                    if p.selected == 0 {
                        p.focused = false;
                    } else {
                        p.selected -= 1;
                    }
                }
                self.redraw();
            }
            Key::PageDown | Key::PageUp if focused => {
                let page = self.config.max_rows;
                if let Some(p) = &mut self.popup {
                    p.selected = if event.key == Key::PageDown {
                        (p.selected + page).min(p.items.len() - 1)
                    } else {
                        p.selected.saturating_sub(page)
                    };
                }
                self.redraw();
            }
            Key::Esc if visible => {
                self.suppressed_word = self.popup.as_ref().map(|p| p.word_start);
                self.close_popup();
            }
            Key::CtrlSpace => {
                if visible {
                    self.suppressed_word = self.popup.as_ref().map(|p| p.word_start);
                    self.close_popup();
                } else {
                    self.suppressed_word = None;
                    self.refresh(true);
                    self.redraw();
                }
            }
            key => {
                // Editing keys keep the list (it follows the line); anything
                // else closes it and passes through.
                let typing = matches!(key, Key::Char(_) | Key::Backspace | Key::Ctrl('w' | 'h'));
                let moving = matches!(
                    key,
                    Key::Left | Key::Right | Key::Home | Key::End | Key::Ctrl('a' | 'e' | 'b' | 'f') | Key::Alt('b' | 'f')
                );
                if let Some(p) = &mut self.popup {
                    p.focused = false;
                }
                if typing {
                    self.typed = true;
                } else if !moving {
                    self.typed = false;
                    self.close_popup();
                }
                self.send(&event.raw, now);
            }
        }
    }

    fn current_word(&self) -> Option<Word> {
        let (line, cursor) = self.line()?;
        Some(analyze(&line, cursor, &self.specs).word)
    }

    fn accept(&mut self, index: usize, now: Instant) {
        let Some(candidate) = self.popup.as_ref().and_then(|p| p.items.get(index).cloned()) else {
            return;
        };
        let Some((line, cursor)) = self.line() else { return };
        let word = analyze(&line, cursor, &self.specs).word;
        let plan = edit::plan(&line, &word, cursor, &candidate);
        self.close_popup();
        self.typed = true;
        self.open_after_echo = candidate.opens_more;
        if !plan.is_noop() {
            self.send(&plan.keystrokes(), now);
        }
    }

    // ----- timers ----------------------------------------------------------

    pub fn on_tick(&mut self, now: Instant) {
        if self.esc_deadline.is_some_and(|t| now >= t) {
            self.esc_deadline = None;
            for event in self.keys.flush() {
                self.on_key(event, now);
            }
        }
        if self.output_deadline.is_some_and(|t| now >= t) {
            self.output_deadline = None;
            let text = self.filter.flush();
            if !text.is_empty() {
                self.apply_output(vec![Piece::Text(text)], now);
            }
        }
        if self.awaiting_echo.is_some_and(|t| now >= t + ECHO_WAIT) {
            self.awaiting_echo = None;
            self.run_deferred(now);
        }
        if self.open_at.is_some_and(|t| now >= t) {
            self.open_at = None;
            if self.active() && self.typed && self.popup.is_none() {
                self.refresh(false);
                self.redraw();
            }
        }
    }

    /// Generator output arrived: update an open (or about to open) list.
    pub fn on_generated(&mut self) {
        if self.popup.is_some() && self.active() {
            self.refresh(false);
            self.redraw();
        }
    }

    pub fn on_resize(&mut self, rows: u16, cols: u16) {
        // Best effort: the terminal may already have reflowed the old popup.
        self.erase_overlay();
        self.popup = None;
        self.shadow.set_size(rows, cols);
    }

    /// Bytes that leave the screen as the shell drew it, for exiting.
    pub fn finish(&mut self) -> Vec<u8> {
        self.erase_overlay();
        self.take_terminal_output()
    }

    // ----- suggestions and drawing -----------------------------------------

    /// Recompute the list for the current line. `force` shows it even when the
    /// quiet rules would keep it closed (Tab, Ctrl+Space).
    fn refresh(&mut self, force: bool) {
        let Some((line, cursor)) = self.line() else {
            self.popup = None;
            return;
        };
        let ctx = analyze(&line, cursor, &self.specs);
        if self.suppressed_word.is_some_and(|s| s != ctx.word.start) {
            self.suppressed_word = None;
        }
        if !force && self.suppressed_word == Some(ctx.word.start) {
            self.popup = None;
            return;
        }
        let cwd = (self.cwd)().or_else(|| std::env::current_dir().ok()).unwrap_or_else(|| PathBuf::from("/"));
        let commands: &[String] = self.commands.get().map_or(&[], Vec::as_slice);
        let generators = &self.generators;
        let generated = |g: &terme_core::spec::Generator| generators.get(&cwd, g);
        let env = Env {
            specs: &self.specs,
            commands,
            cwd: &cwd,
            home: self.home.as_deref(),
            generated: &generated,
        };
        let suggestions = complete(&ctx, &env);
        for g in &suggestions.pending {
            generators.request(&cwd, g);
        }
        let show = !suggestions.items.is_empty() && (force || worth_showing(&ctx, &suggestions));
        if !show {
            self.popup = None;
            return;
        }
        // Keep the highlight on the same item while the list filters.
        let (selected, focused) = match &self.popup {
            Some(p) if p.word_start == ctx.word.start => {
                let current = p.items.get(p.selected).map(|c| c.display.as_str());
                match current.and_then(|d| suggestions.items.iter().position(|c| c.display == d)) {
                    Some(i) => (i, p.focused),
                    None => (0, false),
                }
            }
            _ => (0, false),
        };
        self.popup = Some(Popup {
            items: suggestions.items,
            label: suggestions.label,
            selected,
            focused,
            word_start: ctx.word.start,
        });
    }

    fn close_popup(&mut self) {
        self.popup = None;
        self.open_at = None;
        self.redraw();
    }

    fn erase_overlay(&mut self) {
        if let Some(old) = self.drawn.take() {
            let bytes = self.shadow.restore(old.top, old.left, old.height(), old.width());
            self.to_terminal.extend_from_slice(&bytes);
        }
    }

    /// Bring the screen in line with `self.popup`, in one synchronized frame.
    fn redraw(&mut self) {
        let overlay = self.build_overlay();
        if overlay.is_none() && self.drawn.is_none() {
            return;
        }
        if overlay.is_some() && overlay == self.drawn {
            return;
        }
        self.to_terminal.extend_from_slice(b"\x1b[?2026h");
        self.erase_overlay();
        if let Some(overlay) = overlay {
            self.to_terminal.extend_from_slice(b"\x1b[?25l");
            self.to_terminal.extend_from_slice(&overlay.to_ansi());
            self.to_terminal.extend_from_slice(self.shadow.cursor_and_style().as_bytes());
            self.drawn = Some(overlay);
        }
        self.to_terminal.extend_from_slice(b"\x1b[?2026l");
    }

    fn build_overlay(&self) -> Option<Overlay> {
        let popup = self.popup.as_ref()?;
        let (line, cursor) = self.line()?;
        let start = self.input_start?;
        let (word_row, word_col) = self.shadow.position_of(start, &line, popup.word_start);
        let (cursor_row, _) = self.shadow.position_of(start, &line, cursor);
        let anchor = Anchor {
            word_row,
            word_col,
            cursor_row: cursor_row.max(word_row),
        };
        let show_kinds = shows_kinds(&popup.items);
        let items: Vec<Item> = popup.items.iter().map(|c| to_item(c, show_kinds)).collect();
        let view = PopupView {
            label: &popup.label,
            items: &items,
            selected: popup.selected,
            focused: popup.focused,
            max_rows: self.config.max_rows,
        };
        terme_ui::render(&view, self.shadow.size(), anchor, self.config.theme)
    }
}

/// Kind marks only help when the list holds values or mixes kinds (§5.1).
fn shows_kinds(items: &[Candidate]) -> bool {
    let valueish = |k: &Kind| matches!(k, Kind::File | Kind::Dir | Kind::Value | Kind::Generated(_));
    items.iter().any(|c| valueish(&c.kind))
        && items.iter().any(|c| c.kind != items[0].kind || !matches!(c.kind, Kind::Value))
}

fn to_item(c: &Candidate, show_kinds: bool) -> Item {
    let kind_mark = show_kinds.then(|| match &c.kind {
        Kind::Command | Kind::Subcommand => ('▸', KindColor::Blue),
        Kind::Option => (' ', KindColor::Plain),
        Kind::Value => ('=', KindColor::Green),
        Kind::File => ('▤', KindColor::Plain),
        Kind::Dir => ('▦', KindColor::Blue),
        Kind::Generated(kind) => match kind.as_str() {
            "branch" => ('⎇', KindColor::Magenta),
            "remote" | "host" => ('@', KindColor::Cyan),
            "container" | "unit" => ('●', KindColor::Green),
            "changed file" => ('±', KindColor::Yellow),
            _ => ('•', KindColor::Plain),
        },
    });
    // The kind mark already says what a generated value is.
    let meta = if matches!(c.kind, Kind::Generated(_)) && show_kinds { String::new() } else { c.meta.clone() };
    Item {
        kind_mark,
        name: c.display.clone(),
        highlights: c.highlights.clone(),
        description: c.description.clone(),
        meta,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const PROMPT: &[u8] = b"\x1b]133;A\x07$ \x1b]133;B\x07";

    struct Session {
        app: App,
        now: Instant,
        /// A copy of the screen the user would see.
        screen: vt100::Parser,
        sent: Vec<u8>,
    }

    impl Session {
        fn bash() -> Self {
            Self::new(ShellKind::Bash)
        }

        fn new(shell: ShellKind) -> Self {
            let commands = Arc::new(OnceLock::new());
            let _ = commands.set(vec!["git".to_string(), "grep".to_string()]);
            let app = App::new(
                shell,
                (24, 80),
                SpecSet::builtin(),
                commands,
                Generators::new(|| {}),
                Box::new(|| Some(std::env::temp_dir())),
                None,
                Config::default(),
            );
            let mut s = Self {
                app,
                now: Instant::now(),
                screen: vt100::Parser::new(24.try_into().unwrap(), 80.try_into().unwrap(), 100),
                sent: Vec::new(),
            };
            s.output(PROMPT);
            s
        }

        fn pump(&mut self) {
            let out = self.app.take_terminal_output();
            self.screen.process(&out);
            self.sent.extend(self.app.take_shell_input());
        }

        fn output(&mut self, bytes: &[u8]) {
            self.app.on_output(bytes, self.now);
            self.pump();
        }

        /// Type keys; the "shell" echoes the printable ones it receives, like
        /// bash would.
        fn type_keys(&mut self, bytes: &[u8]) {
            let before = self.sent.len();
            self.app.on_input(bytes, self.now);
            self.pump();
            let echo: Vec<u8> = self.sent[before..].iter().copied().filter(|b| *b >= 0x20 && *b != 0x7f).collect();
            if !echo.is_empty() {
                self.output(&echo);
            }
        }

        /// The screen's text with trailing blanks trimmed on each row.
        fn text(&self) -> String {
            self.screen
                .screen()
                .contents()
                .lines()
                .map(str::trim_end)
                .collect::<Vec<_>>()
                .join("\n")
                .trim_end()
                .to_string()
        }

        fn wait(&mut self, ms: u64) {
            self.now += Duration::from_millis(ms);
            self.app.on_tick(self.now);
            self.pump();
        }

        fn row(&self, row: u16) -> String {
            self.screen.screen().rows(0, 80).nth(usize::from(row)).unwrap_or_default()
        }

        fn popup_visible(&self) -> bool {
            self.screen.screen().contents().contains('╭')
        }
    }

    #[test]
    fn list_opens_after_a_pause_not_while_typing() {
        let mut s = Session::bash();
        s.type_keys(b"git c");
        assert!(!s.popup_visible());
        s.wait(50);
        assert!(!s.popup_visible());
        s.wait(60);
        assert!(s.popup_visible());
        assert!(s.row(1).contains("╭─ subcommand"), "{}", s.row(1));
        assert!(s.row(2).contains("▏commit"), "{}", s.row(2));
    }

    #[test]
    fn tab_inserts_the_rest_of_the_word() {
        let mut s = Session::bash();
        s.type_keys(b"git c");
        s.wait(200);
        s.sent.clear();
        s.type_keys(b"\t");
        assert_eq!(s.sent, b"ommit ");
        assert!(s.row(0).starts_with("$ git commit "));
    }

    #[test]
    fn enter_runs_the_line_while_the_list_is_passive() {
        let mut s = Session::bash();
        s.type_keys(b"git sta");
        s.wait(200);
        assert!(s.popup_visible());
        s.sent.clear();
        s.type_keys(b"\r");
        assert_eq!(s.sent, b"\r");
        assert!(!s.popup_visible());
    }

    #[test]
    fn down_then_enter_inserts_instead_of_running() {
        let mut s = Session::bash();
        s.type_keys(b"git sta");
        s.wait(200);
        s.type_keys(b"\x1b[B");
        assert!(s.screen.screen().contents().contains('↵'));
        s.sent.clear();
        s.type_keys(b"\r");
        assert_eq!(s.sent, b"tus ");
    }

    #[test]
    fn up_before_entering_the_list_is_history() {
        let mut s = Session::bash();
        s.type_keys(b"git sta");
        s.wait(200);
        s.sent.clear();
        s.type_keys(b"\x1b[A");
        assert_eq!(s.sent, b"\x1b[A");
        assert!(!s.popup_visible());
    }

    #[test]
    fn up_on_the_first_item_leaves_the_list() {
        let mut s = Session::bash();
        s.type_keys(b"git sta");
        s.wait(200);
        s.type_keys(b"\x1b[B\x1b[B\x1b[A\x1b[A");
        assert!(s.popup_visible());
        assert!(!s.screen.screen().contents().contains('↵'));
        s.sent.clear();
        s.type_keys(b"\r");
        assert_eq!(s.sent, b"\r");
    }

    #[test]
    fn esc_hides_until_the_next_word_and_tab_falls_back_to_the_shell() {
        let mut s = Session::bash();
        s.type_keys(b"git sta");
        s.wait(200);
        s.type_keys(b"\x1b");
        s.wait(30);
        assert!(!s.popup_visible());
        s.type_keys(b"t");
        s.wait(200);
        assert!(!s.popup_visible());
        s.sent.clear();
        s.type_keys(b"\t");
        assert_eq!(s.sent, b"\t");
        s.type_keys(b"us ");
        s.wait(200);
        // A new word: suggestions are back (status takes paths, the temp dir has some).
        let _ = s.popup_visible();
    }

    #[test]
    fn closing_restores_what_was_under_the_popup() {
        let mut s = Session::bash();
        let before = s.text();
        s.type_keys(b"git c");
        s.wait(200);
        assert!(s.popup_visible());
        s.type_keys(b"\x03");
        assert!(!s.popup_visible());
        assert_eq!(s.text(), format!("{before} git c"));
        // Restored blanks are erased, not overwritten with spaces.
        assert_eq!(s.screen.screen().contents(), format!("{before} git c"));
    }

    #[test]
    fn flips_above_and_restores_output_at_the_bottom() {
        let mut s = Session::bash();
        let lines: Vec<u8> = (1..=40).flat_map(|i| format!("\r\nline {i}").into_bytes()).collect();
        s.output(&lines);
        s.output(b"\r\n");
        s.output(PROMPT);
        let before = s.text();
        s.type_keys(b"git c");
        s.wait(200);
        assert!(s.popup_visible());
        assert!(s.row(23).starts_with("$ git c"));
        assert!(s.row(22).contains('╰'), "the popup sits right above the prompt");
        s.type_keys(b"\x1b");
        s.wait(30);
        assert_eq!(s.text(), format!("{before} git c"));
    }

    #[test]
    fn nothing_while_a_command_runs() {
        let mut s = Session::bash();
        s.output(b"\x1b]133;C\x07");
        s.type_keys(b"git c");
        s.wait(200);
        assert!(!s.popup_visible());
        assert_eq!(s.sent, b"git c");
    }

    #[test]
    fn keys_wait_for_the_echo_before_tab() {
        let mut s = Session::bash();
        // Typed and Tab in one read, before the shell echoes anything.
        // One match: Tab inserts it at once.
        s.app.on_input(b"git comm\t", s.now);
        assert_eq!(s.app.take_shell_input(), b"git comm");
        s.output(b"git comm");
        assert_eq!(s.sent, b"it ");
        // Several matches: Tab opens the list instead.
        let mut s = Session::bash();
        s.app.on_input(b"git c\t", s.now);
        assert_eq!(s.app.take_shell_input(), b"git c");
        s.output(b"git c");
        assert!(s.sent.is_empty());
        assert!(s.popup_visible());
    }

    #[test]
    fn zsh_uses_the_reported_line() {
        let mut s = Session::new(ShellKind::Zsh);
        s.output(b"\x1b]6973;line;0;\x07");
        s.app.on_input(b"git c", s.now);
        s.pump();
        s.output(b"\x1b]6973;line;5;git c\x07git c");
        s.wait(200);
        assert!(s.popup_visible());
        // The private messages never reach the terminal.
        assert!(!s.screen.screen().contents().contains("6973"));
    }

    #[test]
    fn options_flow_into_values() {
        let mut s = Session::bash();
        s.type_keys(b"tar -xz");
        s.wait(200);
        assert!(s.screen.screen().contents().contains("add to -xz"));
        s.sent.clear();
        s.type_keys(b"\t");
        assert_eq!(s.sent, b"f ");
        // The value list opens as soon as the shell echoes, without the pause.
        assert!(s.screen.screen().contents().contains("archive to read") || !s.popup_visible());
    }
}
