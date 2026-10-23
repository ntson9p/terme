# Developing terme

## Layout

| Crate | What's in it |
|---|---|
| `crates/terme-core` | Everything about commands, with no terminal or process handling: the line lexer, specs (`specs/*.json`), what's expected at the cursor, matching, ranking, the keystrokes that insert a suggestion, and reading options out of rendered man pages (`man.rs`). |
| `crates/terme-ui` | Popup sizing, placement and drawing into cells and escape sequences. Pure; never touches a terminal. |
| `crates/terme` | The binary: the pseudo-terminal wrapper, shell hooks (`src/hooks/`), the shadow screen, key parsing and the `App` state machine. |

`App` (in `crates/terme/src/app.rs`) does no I/O: it takes keys, shell output and the time, and returns bytes for the terminal and keystrokes for the shell. `main.rs` owns the threads and the real file descriptors.

The slow sources live in `crates/terme/src/sources.rs` and fill in from background threads: command names on `PATH`, one-line summaries from `apropos`, generator output, and man pages (`man -S 1:8:6 -P cat`, rendered 200 columns wide, then parsed). A command with no bundled spec gets one built from its man page the first time it's typed.

## Building and testing

terme targets Linux. On Windows, use WSL:

```sh
cargo build
cargo test --workspace                       # unit, snapshot and end-to-end tests
cargo test --release -p terme --bin terme -- --ignored latency --nocapture
cargo clippy --workspace --all-targets -- -D warnings
cargo fmt --all
```

The end-to-end tests need `bash` and `zsh` installed (each test runs once per shell) and `git` and `seq` on `PATH`.

If the repo lives on the Windows side (`/mnt/c/...`, `/mnt/d/...`), put the build output on the Linux filesystem; it's several times faster:

```sh
export CARGO_TARGET_DIR=$HOME/.cache/terme-target
```

### The test layers

1. **Unit tests** in each module: lexer, context, matching, completion, edits, key parsing, the output filter, the shadow screen.
2. **Snapshot tests** (`crates/terme-ui/src/snapshots/`): popups drawn as text. Review changes with `cargo insta review` (or rerun with `INSTA_UPDATE=always` and read the diff).
3. **Simulated sessions** (`app.rs` tests): the state machine driven by fake shell output, checking the keyboard model, delays, restoring, and keys that wait for the shell's echo.
4. **End-to-end tests** (`crates/terme/tests/e2e.rs`): the real binary running real bash and zsh in a pseudo-terminal, with a vt100 screen playing the terminal.
5. **Latency**: keystroke to frame must stay under 16 ms at p99.

CI runs all of these on Ubuntu 24.04 and 22.04.

### Looking at it by hand, headlessly

tmux makes a good scriptable terminal:

```sh
tmux new-session -d -x 100 -y 20 -s t "target/debug/terme"
tmux send-keys -t t "git commit --a"; sleep 0.5
tmux capture-pane -t t -p        # add -e to see the colours as escapes
tmux kill-session -t t
```

## Writing a spec

Specs are JSON files in `crates/terme-core/specs/`, listed in `BUILTIN` in `spec.rs`. A command has `names`, `description`, `subcommands`, `options` and `args`; options have `names`, `description`, an optional `arg`, and `priority` (higher comes first). Args can have fixed `suggestions`, a `template` (`filepaths` or `folders`) and a `generator` (a program whose output lines become values). `exclusive` lists options that can't be combined. The `bundled_specs_parse` and `option_names_are_well_formed` tests check every spec.

Descriptions are short, lowercase phrases ("extract files", "redo last commit"), following the voice in the UI spec (§13.4).
