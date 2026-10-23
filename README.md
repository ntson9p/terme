# terme

IntelliSense for the terminal you already use: as you type a command, terme shows what you can type next and what it means (subcommands, flags, values, examples and warnings) right at the cursor.

> **Status:** early development. Milestone M0 (a working tracer for bash and zsh on Linux) is done, and the first part of M1: options and docs read from your installed man pages. Hints, ghost text, explanations and danger warnings come next (see the [roadmap](docs/ui-ux-spec.md#18-roadmap-and-success-metrics)).

```
~/shop $ git commit --a█
                  ╭─ option ──────────────────────────╮
                  │▏--amend         redo last commit  │
                  │ --all  -a       stage all changes │
                  │ --author=‹who›  set the author    │
                  │ --allow-empty   allow no changes  │
                  ╰──────────────────────────── 1/4 ──╯
```

## Try it

You need Linux (or WSL), Rust, and bash or zsh.

```sh
git clone https://github.com/ntson9p/terme
cd terme
cargo build --release
./target/release/terme            # your $SHELL, with suggestions
./target/release/terme --shell /usr/bin/zsh
```

Type `exit` to leave. terme doesn't change any of your dotfiles; it loads its hooks itself.

## Keys

| Key | What it does |
|---|---|
| Tab | Insert the highlighted suggestion (or open the list now) |
| ↓ | Move into the list; after that, Enter inserts too |
| Enter | Otherwise: run your line exactly as typed |
| ↑ | Shell history, until you've moved into the list |
| Esc | Hide suggestions until the next word; Tab is then the shell's own completion |
| Ctrl+Space | Open or close the list |

The list opens on its own when you pause for 100 ms, then follows every keystroke. The thin bar `▏` marks what Tab would insert; once you press ↓ it becomes a solid highlighted row and `↵` appears, meaning Enter now inserts.

## What it knows

- **Every command with a man page.** terme reads the installed page in the background the first time you use a command, so its options come with descriptions, and the docs pane shows the page's full text for the highlighted option. Because it's the installed page, it matches your version.
- **Ten hand-written specs** for `apt`, `cd`, `cp`, `docker`, `git`, `grep`, `ls`, `ssh`, `systemctl` and `tar`, with subcommands and live values: git branches and changed files, Docker containers and images, systemd units, apt packages, ssh hosts.
- **Every command on your `PATH`**, with its one-line description, plus files and folders.

### Known rough edges

- You start it yourself (`terme`); starting it automatically in every shell comes with `terme setup` later.
- The first time you use a command, its options take about a second to appear while its man page is read.
- Resizing the window while a list is open can leave marks until the next redraw.
- bash in vi mode (`set -o vi`) isn't supported yet; zsh's vi mode is.
- Linux only for now.

## How it works

terme runs your shell inside a pseudo-terminal and draws its suggestions on top of the screen, so it works in the terminal you already use (GNOME Terminal, Ptyxis, Konsole, Kitty, WezTerm, VS Code, tmux…). Small hooks tell terme where each prompt ends; zsh also reports the exact command line. Popups never become text: whatever they covered is restored, and nothing of terme's reaches your scrollback.

- [docs/ui-ux-spec.md](docs/ui-ux-spec.md): the full design.
- [docs/development.md](docs/development.md): building, testing and the code layout.
- [docs/manual-checks.md](docs/manual-checks.md): the checks only a person at a real terminal can do.

## Platforms

Linux first (bash and zsh), then macOS from the same code, then Windows.

## License

Licensed under either of

- Apache License, Version 2.0 ([LICENSE-APACHE](LICENSE-APACHE))
- MIT license ([LICENSE-MIT](LICENSE-MIT))

at your option.

Unless you explicitly state otherwise, any contribution intentionally submitted for inclusion in the work by you, as defined in the Apache-2.0 license, shall be dual licensed as above, without any additional terms or conditions.
