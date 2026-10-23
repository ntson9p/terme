# terme

IntelliSense for the terminal you already use: as you type a command, terme shows what you can type next and what it means (subcommands, flags, values, examples and warnings) right at the cursor.

> **Status:** early development. The first milestone (M0, a working tracer for bash and zsh on Linux) is being built now. Nothing is ready to install yet.

## How it works

terme runs your shell inside a pseudo-terminal and draws its suggestions on top of the screen, so it works in the terminal you already use (GNOME Terminal, Ptyxis, Konsole, Kitty, WezTerm, VS Code, tmux…) without replacing it.

```
~/shop $ git commit --a█
                  ╭─ option ───────────────────────────╮
                  │▏--amend          redo last commit  │
                  │ --all  -a        stage all changes │
                  │ --author=‹who›   set the author    │
                  │ --allow-empty    allow no changes  │
                  ╰───────────────────────────── 1/4 ──╯
```

- **Tab** inserts the highlighted suggestion.
- **↓** moves into the list; after that, **Enter** inserts too.
- Otherwise **Enter** runs your line exactly as typed.
- **Esc** hides suggestions until the next word.

The full design is in [docs/ui-ux-spec.md](docs/ui-ux-spec.md).

## Platforms

Linux first (bash and zsh), then macOS from the same code, then Windows.

## License

Licensed under either of

- Apache License, Version 2.0 ([LICENSE-APACHE](LICENSE-APACHE))
- MIT license ([LICENSE-MIT](LICENSE-MIT))

at your option.

Unless you explicitly state otherwise, any contribution intentionally submitted for inclusion in the work by you, as defined in the Apache-2.0 license, shall be dual licensed as above, without any additional terms or conditions.
