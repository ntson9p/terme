//! terme: suggestions and docs as you type, in the terminal you already use.

mod app;
mod keys;
mod osc;
mod shadow;
mod shell;
mod sources;
mod term;

use std::io::{Read, Write};
use std::os::unix::process::CommandExt;
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::path::PathBuf;
use std::sync::mpsc::{RecvTimeoutError, Sender, channel};
use std::sync::{Arc, OnceLock};
use std::time::{Duration, Instant};

use anyhow::{Context, Result, bail};
use portable_pty::{PtySize, native_pty_system};
use signal_hook::consts::{SIGHUP, SIGTERM, SIGWINCH};
use signal_hook::iterator::Signals;

use crate::app::{App, Config};
use crate::shell::ShellKind;

enum Event {
    Input(Vec<u8>),
    Output(Vec<u8>),
    Closed,
    Resize,
    Generated,
}

const HELP: &str = "\
terme: suggestions and docs as you type, in the terminal you already use.

Usage:
  terme [--shell PATH]    start your shell with suggestions
  terme init bash|zsh     print the shell hooks (terme loads them itself)
  terme --version
  terme --help

Keys: Tab inserts · ↓ moves into the list (then Enter inserts too)
      Enter runs your line as typed · Esc hides until the next word
      Ctrl+Space opens or closes the list

Set TERME=off to start the shell without terme.";

fn main() {
    let code = match run() {
        Ok(code) => code,
        Err(e) => {
            eprintln!("terme: {e:#}");
            1
        }
    };
    std::process::exit(code);
}

fn run() -> Result<i32> {
    let mut args = std::env::args().skip(1);
    let mut shell_flag = None;
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--help" | "-h" => {
                println!("{HELP}");
                return Ok(0);
            }
            "--version" | "-V" => {
                println!("terme {}", env!("CARGO_PKG_VERSION"));
                return Ok(0);
            }
            "--shell" => shell_flag = Some(PathBuf::from(args.next().context("--shell needs a path")?)),
            "init" => {
                match args.next().as_deref() {
                    Some("bash") => print!("{}", shell::BASH_HOOKS),
                    Some("zsh") => print!("{}", shell::ZSH_HOOKS),
                    _ => bail!("usage: terme init bash|zsh"),
                }
                return Ok(0);
            }
            other => bail!("unknown argument {other:?} (see terme --help)"),
        }
    }

    let shell = shell::choose(shell_flag);
    let kind = ShellKind::detect(&shell);
    let tty = term_name();
    let nested = tty.is_some() && std::env::var_os("TERME_TTY").as_deref() == tty.as_deref().map(std::ffi::OsStr::new);
    if std::env::var("TERME").as_deref() == Ok("off") || !term::is_tty() || nested || kind.is_none() {
        if kind.is_none() {
            eprintln!("terme: {} isn't supported yet; starting it without suggestions", shell.display());
        }
        // Nothing to add: become the shell.
        let err = std::process::Command::new(&shell).exec();
        bail!("starting {}: {err}", shell.display());
    }
    wrap(&shell, kind.expect("checked above"))
}

fn term_name() -> Option<String> {
    std::fs::read_link("/proc/self/fd/0").ok().map(|p| p.display().to_string())
}

fn wrap(shell: &std::path::Path, kind: ShellKind) -> Result<i32> {
    let (rows, cols) = term::size();
    let pty = native_pty_system();
    let pair = pty
        .openpty(PtySize {
            rows,
            cols,
            pixel_width: 0,
            pixel_height: 0,
        })
        .map_err(|e| anyhow::anyhow!("opening a pseudo-terminal: {e}"))?;
    let (mut cmd, _hooks) = shell::prepare(shell, kind)?;
    if let Ok(cwd) = std::env::current_dir() {
        cmd.cwd(cwd);
    }
    cmd.env("TERME_SESSION", std::process::id().to_string());
    if let Some(name) = pair.master.tty_name() {
        cmd.env("TERME_TTY", name);
    }
    let mut child = pair
        .slave
        .spawn_command(cmd)
        .map_err(|e| anyhow::anyhow!("starting {}: {e}", shell.display()))?;
    drop(pair.slave);

    let (tx, rx) = channel::<Event>();
    let mut reader = pair.master.try_clone_reader().map_err(|e| anyhow::anyhow!("{e}"))?;
    let mut writer = pair.master.take_writer().map_err(|e| anyhow::anyhow!("{e}"))?;
    let master = pair.master;

    spawn_reader("terme-pty", tx.clone(), move |buf| reader.read(buf), Event::Output);
    spawn_reader("terme-stdin", tx.clone(), |buf| std::io::stdin().read(buf), Event::Input);
    let signal_tx = tx.clone();
    let mut signals = Signals::new([SIGWINCH, SIGTERM, SIGHUP]).context("installing signal handlers")?;
    std::thread::Builder::new().name("terme-signals".into()).spawn(move || {
        for sig in signals.forever() {
            let event = if sig == SIGWINCH { Event::Resize } else { Event::Closed };
            if signal_tx.send(event).is_err() {
                break;
            }
        }
    })?;

    let commands: Arc<OnceLock<Vec<String>>> = Arc::new(OnceLock::new());
    let fill = commands.clone();
    std::thread::Builder::new().name("terme-path".into()).spawn(move || {
        let _ = fill.set(sources::command_names());
    })?;
    let generated_tx: Sender<Event> = tx.clone();
    let generators = sources::Generators::new(move || {
        let _ = generated_tx.send(Event::Generated);
    });
    let pid = child.process_id();
    let cwd = Box::new(move || pid.and_then(sources::cwd_of));
    let home = std::env::var_os("HOME").map(PathBuf::from);
    let mut app = App::new(
        kind,
        (rows, cols),
        terme_core::SpecSet::builtin(),
        commands,
        generators,
        cwd,
        home,
        Config::default(),
    );

    let raw = term::RawMode::enable().context("switching the terminal to raw mode")?;
    install_panic_hook();
    let mut stdout = std::io::stdout().lock();
    let mut passthrough = false;

    loop {
        let timeout = app
            .next_deadline()
            .map_or(Duration::from_secs(3600), |d| d.saturating_duration_since(Instant::now()));
        let event = match rx.recv_timeout(timeout) {
            Ok(event) => Some(event),
            Err(RecvTimeoutError::Timeout) => None,
            Err(RecvTimeoutError::Disconnected) => break,
        };
        if matches!(event, Some(Event::Closed)) {
            break;
        }
        if passthrough {
            match event {
                Some(Event::Input(b)) => writer.write_all(&b)?,
                Some(Event::Output(b)) => stdout.write_all(&b)?,
                Some(Event::Resize) => resize(&*master, term::size()),
                _ => {}
            }
            writer.flush()?;
            stdout.flush()?;
            continue;
        }
        let now = Instant::now();
        let step = catch_unwind(AssertUnwindSafe(|| {
            match event {
                Some(Event::Input(b)) => app.on_input(&b, now),
                Some(Event::Output(b)) => app.on_output(&b, now),
                Some(Event::Resize) => {
                    let size = term::size();
                    app.on_resize(size.0, size.1);
                    resize(&*master, size);
                }
                Some(Event::Generated) => app.on_generated(),
                Some(Event::Closed) | None => {}
            }
            app.on_tick(Instant::now());
        }));
        match step {
            Ok(()) => {
                writer.write_all(&app.take_shell_input())?;
                stdout.write_all(&app.take_terminal_output())?;
            }
            Err(_) => {
                // Never take the shell down with us: keep relaying bytes.
                passthrough = true;
                stdout.write_all(b"\x1b[0m\r\nterme: suggestions paused after an internal error\r\n")?;
            }
        }
        writer.flush()?;
        stdout.flush()?;
    }

    if !passthrough {
        stdout.write_all(&app.finish())?;
        stdout.flush()?;
    }
    drop(raw);
    drop(writer);
    let status = child.wait().ok();
    Ok(status.map_or(0, |s| i32::try_from(s.exit_code()).unwrap_or(1)))
}

fn resize(master: &dyn portable_pty::MasterPty, (rows, cols): (u16, u16)) {
    let _ = master.resize(PtySize {
        rows,
        cols,
        pixel_width: 0,
        pixel_height: 0,
    });
}

fn spawn_reader(
    name: &str,
    tx: Sender<Event>,
    mut read: impl FnMut(&mut [u8]) -> std::io::Result<usize> + Send + 'static,
    wrap: fn(Vec<u8>) -> Event,
) {
    let _ = std::thread::Builder::new().name(name.into()).spawn(move || {
        let mut buf = vec![0u8; 64 * 1024];
        loop {
            match read(&mut buf) {
                Ok(0) => break,
                Ok(n) => {
                    if tx.send(wrap(buf[..n].to_vec())).is_err() {
                        return;
                    }
                }
                Err(e) if e.kind() == std::io::ErrorKind::Interrupted => {}
                Err(_) => break,
            }
        }
        let _ = tx.send(Event::Closed);
    });
}

/// Panics are caught in the main loop; this keeps them off the screen and
/// writes the details to a file instead.
fn install_panic_hook() {
    std::panic::set_hook(Box::new(|info| {
        let dir = std::env::var_os("XDG_STATE_HOME")
            .map(PathBuf::from)
            .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".local/state")))
            .map(|d| d.join("terme"));
        if let Some(dir) = dir {
            let _ = std::fs::create_dir_all(&dir);
            let _ = std::fs::write(dir.join("crash.log"), format!("{info}\n{}", std::backtrace::Backtrace::force_capture()));
        }
    }));
}
