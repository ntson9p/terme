# terme: UI/UX design spec

Suggestions and docs as you type, inside the terminal you already use.

Draft 0.1 · 2026-10-23 · Scope: v1 on Linux, with notes for macOS and Windows.

**Contents:** 1 Product · 2 Decisions · 3 Principles · 4 Where terme lives · 5 Surfaces · 6 Scenarios · 7 Behaviour · 8 Ghost text · 9 Explain the line · 10 Danger warnings · 11 Docs and sources · 12 AI · 13 Visual design · 14 Onboarding and settings · 15 Compatibility and edge cases · 16 Performance, privacy, accessibility · 17 Platforms · 18 Roadmap · 19 Open questions · A Default config · B Implementation notes

**Reading the mockups**

| Mark | Meaning |
|---|---|
| `█` | the cursor |
| text after `█` on the input line | ghost text: dim, never executed |
| `▏` | passive selection: what Tab would insert (thin accent bar) |
| `▌` | focused selection: you moved into the list, so Enter inserts too (the whole row is in the accent colour) |
| `‹FILE›` | a value placeholder (muted italic on screen) |

---

## 1. Product

**One line.** As you type a command, terme shows what you can type next and what it means (subcommands, flags, values, examples, warnings), right at the cursor, inside the terminal you already use.

**Primary user (v1).** Developers who know the command line but keep forgetting flags, subcommands and argument order: `tar`, `git`, `docker`, `systemctl`, `find`, `ln`, `ssh`, `kubectl`. Defaults are tuned for them: terse, fast, with more detail one key away.

**Jobs terme does**

1. *"What was the flag for…?"* Find it without leaving the prompt, even when you only remember what it does.
2. *"What does this do?"* Understand a flag, a whole line, or a pasted command before pressing Enter.
3. *"Run that long thing again."* Ghost text from your own history.
4. *"Is this safe?"* Warnings for destructive commands, with a safer alternative.

**Not in v1:** its own terminal emulator · fish and PowerShell · asking bash-completion or zsh completion functions for candidates (Tab still falls back to them) · live validation and typo underlines · automatic hints after a failed command · installing itself on SSH servers · mouse support.

## 2. Decisions

Decided in a Q&A on 2026-10-23. Options that weren't chosen appear under "Not in v1" above or "Later" in §18.

| Area | Decision | Why |
|---|---|---|
| Form factor | A PTY wrapper that runs inside the user's existing terminal | Nobody switches terminals; one engine for every shell; ports to macOS and Windows |
| Primary user | Developers who forget flags | Largest daily-use audience; terse defaults |
| Layout | Popup anchored at the current word, plus a docs pane | Eyes stay at the cursor; familiar from IDEs |
| Trigger | Automatic, with smart quieting | Real "docs as you type" without noise |
| Keyboard | Safe hybrid | IDE feel without taking over Enter or ↑ |
| Docs sources | man pages, curated completion specs (Fig import), tldr | Version-correct, structured, with examples; all offline |
| AI | Opt-in, only for actions the user triggers | The core stays private and predictable |
| Helpers | Explain the line; danger warnings | Learn and stay safe; cheap to build well |
| Shells (v1) | bash, zsh | Ubuntu's default and macOS's default |
| Ghost text | From history, coordinated with the popup | Re-running long commands; new for bash users |
| Visual style | Rounded box, Unicode kind marks, colours from the terminal theme | Clearly separate from output; works with every default font |
| Stack | Rust | It sits under every shell, so startup, latency and memory matter; its crates already handle Windows ConPTY |
| Platforms | One codebase: a `unix` build (Linux, then macOS), Windows later | macOS comes almost free after Linux; Windows reuses the core |

## 3. Principles

Ranked: when two conflict, the higher one wins.

1. **The line you see is the line that runs.** Enter never inserts or runs anything you haven't explicitly selected. Dim text is never executed.
2. **Never slower than typing.** Keystrokes reach the shell with no added delay, and overlays update within one frame (16 ms or less). If data isn't ready, show less; never lag.
3. **When unsure, show nothing.** A missing suggestion costs a second; a wrong one costs trust.
4. **Eyes stay at the cursor.** Information appears next to the word being typed.
5. **Quiet unless useful.** No popup for free text, finished words or unknown commands. Esc always hides it.
6. **One key deeper.** One-line description, then the docs pane, then the full man page (Alt+H).
7. **Truthful, sourced docs.** The installed tool's own docs win. Every pane shows its source and version, and AI text is labelled.
8. **Native to the user's terminal.** Their colours, their font, their scrollback. terme leaves nothing behind in scrollback.
9. **Private by default.** Everything stays local, there's no telemetry, and AI runs only when it's enabled and asked.

## 4. Where terme lives

```
 Your terminal  (Ptyxis, GNOME Terminal, Konsole, Kitty, WezTerm, VS Code, tmux…)
        ▲  shell output + terme's overlays                 │  your keystrokes
        │                                                  ▼
 ┌──────────────────────────────── terme ────────────────────────────────┐
 │  key router ─► keymap ─► nearly every key goes straight to the shell  │
 │  line model ◄─ shell hooks: prompt marks, line, cursor, exit code     │
 │  engine: parse ─► context ─► candidates ─► rank ─► docs               │
 │           (specs · man pages · tldr · history · files · git)          │
 │  compositor: shadow screen + overlay layer ─► minimal redraw          │
 └───────────────────────────────────────────────────────────────────────┘
        ▲  PTY
        │
   bash or zsh   (+ two marked blocks in ~/.bashrc or ~/.zshrc, added by `terme setup`)
```

What users notice because of this design:

- **terme is active only at the shell prompt.** While a command runs (`vim`, `htop`, `python`, `ssh`, a `sudo` password prompt), everything passes through untouched.
- **Overlays never become text.** They're drawn on top of the screen, never typed into the shell, and erased before any command output, so they never land in scrollback.
- **SSH.** Local terme stays passive, because the local shell is busy running `ssh`. If terme is installed on the server, the server's copy draws the suggestions; an inner terme announces itself, so two copies never draw at once.
- **Failure is safe.** If the suggestion engine fails, the shell keeps running: terme drops to plain passthrough and says so once.

## 5. Surfaces

The **popup** is the list plus, when there's room, the docs pane. Every other surface is lighter.

```
~/shop $ git commit --a█mend --no-edit
                  ╭─ option ───────────────────────────╮╭─ --amend ────────────────────────╮
                  │▏--amend          redo last commit  ││ Replace the tip of the current   │
                  │ --all  -a        stage all changes ││ branch by creating a new commit. │
                  │ --author=‹who›   set the author    ││                                  │
                  │ --allow-empty    allow no changes  ││ ⚠ Rewrites history: don't amend  │
                  ╰───────────────────────────── 1/4 ──╯│   commits you've already pushed. │
                                                        │                                  │
                                                        │ Example                          │
                                                        │   git commit --amend --no-edit   │
                                                        ╰─ man git-commit · git 2.43 ──────╯
```

The mockup shows ① ghost text (`mend --no-edit`), ② the list and ③ the docs pane. The others appear in §6.

| | Surface | What it shows | When |
|---|---|---|---|
| ① | Ghost text | The rest of a matching line from your history, dim | The cursor is at the end of the line and history has a strong match |
| ② | List | Candidates for the word under the cursor, each with a one-line description | Opens on its own (§7.1) |
| ③ | Docs pane | The highlighted item's description, warning, example and source | Alongside the list, when it adds something |
| ④ | Hint line | One muted line under the input line: a warning, a slot hint, or a plain-English reading of the line | While the list is closed |
| ⑤ | Explain panel | One row per word of the line, saying what it does | Alt+E, and on its own after a paste |
| ⑥ | Doc viewer | A full-screen man, tldr or spec reader | Alt+H |
| ⑦ | Status note | Rare one-liners on the hint line ("building the docs index…") | On events |

### 5.1 List rows

A row is: gutter · kind · name · description · meta.

- **Gutter** (1 column): `▏` for the passive selection, `▌` for the focused selection, otherwise blank.
- **Kind mark** (1 column plus a space): shown only when the list mixes kinds or lists values (files, branches…). Options don't need one, because their dashes already say what they are.
- **Name:** bold, with the characters you typed in the accent colour. Options use man-page pairing (`-x, --extract`), and long-only options are indented to line up. While you're typing a long option (`--a`), the long name leads and the short alias trails (`--all  -a`).
- **Description:** muted, the first clause of the docs, cut with `…`.
- **Meta** (right edge, muted, optional): `needs ‹FILE›`, `2 days ago`, `remote`, `deprecated`, or a red `⚠` on dangerous items.
- **Top border label:** what's expected here (`command`, `option`, `branch`, `‹FILE› archive to read`, `add to -xz`), or a red danger label.
- **Bottom border:** the position (`3/42`). When the list is focused, `↵` appears there, meaning Enter now inserts.

| Kind | Mark | ASCII | Colour |
|---|---|---|---|
| Command or subcommand | `▸` | `>` | blue |
| Your alias or function | `ƒ` | `a` | cyan |
| Option | none | none | default |
| Value from a fixed set | `=` | `=` | green |
| File | `▤` | `-` | default |
| Directory (shown with a trailing `/`) | `▦` | `/` | blue |
| Git branch / tag | `⎇` / `#` | `b` / `#` | magenta |
| Changed file (`git add`) | its status: `M`, `A`, `??` | same | yellow |
| Host or remote | `@` | `@` | cyan |
| Environment variable | `$` | `$` | green |
| Running / stopped (container, service, process) | `●` / `○` | `*` / `o` | green / muted |
| History entry | `↺` | `h` | muted |

Nerd Font icons for the same kinds are opt-in (`glyphs = "nerd"`); `terme setup` shows them and asks whether they render.

### 5.2 Docs pane

Blocks, in this order, each only if it exists:

1. **Title** (in the top border): the item with its aliases, such as `-x, --extract, --get`.
2. **Description:** 1–6 plain lines.
3. **Value:** what the value looks like (`‹who›  "Name <email>"`).
4. **Warning:** `⚠`, what happens to your data, and a safer alternative.
5. **Example:** one or two from tldr or the spec that use this item.
6. **Footer:** the source and the installed version, such as `man tar(1) · GNU tar 1.35`, or `spec · from git 2.45, you have 2.43`.

Rules: 32–48 columns wide and at most 12 lines (then `Alt+H more`). Hidden when it would only repeat the list's one-liner. The one-liner shows at once and the rest fills in when it's parsed; a spinner appears only after 150 ms. Alt+↑ and Alt+↓ scroll it.

### 5.3 Hint line

One muted line directly under the input line, while the list is closed. It shows the first of these that applies:

1. **Danger** (red): what will happen and a safer alternative (§10).
2. **Slot hint**, inside a free-text or positional argument: `‹LINK_NAME› where to create the link`.
3. **Reading** of the line in plain English, ending with the next expected slot: `↳ extract · gunzip · from backup.tgz · into /srv`, with `Alt+E` at the right edge (§9).
4. Nothing.

**Making room without covering output.** When a new prompt lands on the last row of the screen, terme scrolls the screen up one line at that moment, so the row under the prompt is free. This happens while command output is still moving the screen, so nothing jumps while you type. Users who dislike the empty row can choose `hint_line = "right"` (right-aligned on the input line when it fits) or `"off"`.

## 6. Scenarios

### 6.1 "Which tar flags extract a .tgz?"

Type `-` and pause. The most common options come first (based on tldr and your own use), with docs for the highlighted one.

```
~ $ tar -█
      ╭─ option ─────────────────────────╮╭─ -x, --extract ─────────────────╮
      │▏-x, --extract      extract files ││ Extract files from an archive.  │
      │ -c, --create       new archive   ││                                 │
      │ -t, --list         list contents ││ Example                         │
      │ -f, --file=‹FILE›  archive file  ││   tar -xf archive.tar           │
      │ -z, --gzip         gzip (.gz)    │╰─ man tar(1) · GNU tar 1.35 ─────╯
      │ -v, --verbose      list files    │
      ╰────────────────────────── 1/86 ──╯
```

Type `xz`. Short flags can be bundled, so the list offers letters to add. Tab appends `f`, and because `f` takes a value, a space follows.

```
~ $ tar -xz█
      ╭─ add to -xz ─────────────────────╮
      │▏f   archive file    needs ‹FILE› │
      │ v   list each file               │
      │ k   don't overwrite files        │
      │ p   keep permissions             │
      ╰────────────────────────── 1/41 ──╯
```

Now a file is expected. Archives come first (matched by extension), then folders and other files.

```
~ $ tar -xzf █
         ╭─ ‹FILE› archive to read ─────────╮
         │▏▤ backup-2026-10.tar.gz   12 MB  │
         │ ▤ site.tgz                3.1 MB │
         │ ▦ old/                           │
         │ ▤ notes.txt                      │
         ╰────────────────────────── 1/9 ───╯
```

Once the folder is typed in full, there's nothing left to suggest. The list closes and the hint line reads the command back.

```
~ $ tar -xzf backup-2026-10.tar.gz -C /srv█
    ↳ extract · gunzip · from backup-2026-10.tar.gz · into /srv           Alt+E
```

### 6.2 "Which way round does ln -s go?"

Positional arguments have names, and the hint line names the next one, so nobody has to remember that the target comes first.

```
~ $ ln -s ~/dotfiles/vimrc █
    ↳ make a symlink · pointing at ~/dotfiles/vimrc · next: ‹LINK_NAME›, where the link goes
```

### 6.3 Live values

Values come from generators: branches by recency, changed files with their status, hosts from `~/.ssh/config`, units from systemd, containers from Docker. Slow generators load in the background and never block typing.

```
~/shop $ git checkout fe█
                  ╭─ branch ──────────────────────────╮
                  │▏⎇ feature/login       2 days ago  │
                  │ ⎇ feature/search      5 days ago  │
                  │ ⎇ origin/feature/pay  remote      │
                  │ ▤ feedback.md         file        │
                  ╰─────────────────────────── 1/4 ───╯
```

```
~/shop $ git add █
            ╭─ changed file ─────────────────╮
            │▏M  src/cart.ts                 │
            │ M  src/api/order.ts            │
            │ ?? notes/todo.md    untracked  │
            ╰──────────────────────── 1/3 ───╯
```

### 6.4 Search by meaning

There's no `--port` option, so terme searches the descriptions instead. Tab replaces `--port` with `--publish `, and the hint line then shows `‹host:container› e.g. 8080:80`.

```
~ $ docker run --port█
             ╭─ option · matched in docs ───────────────╮
             │▏-p, --publish=‹host:ctr›  publish a port │
             │ -P, --publish-all         all exposed    │
             │     --expose=‹port›       expose a port  │
             ╰────────────────────────────────── 1/3 ───╯
```

### 6.5 Run it again

Ghost text comes from your history; this line was used 31 times in this folder. → takes all of it and Alt+→ takes one word. Enter runs only what you typed.

```
~/shop $ docker compose up█ -d --build
         ↳ start the services                        → take it all · Alt+→ one word
```

On an empty line, Ctrl+Space lists what you run most in this folder.

```
~/shop $ █
     ╭─ often run in ~/shop ─────────────────╮
     │▏↺ docker compose up -d --build    31× │
     │ ↺ npm run dev                     24× │
     │ ↺ git pull --rebase               12× │
     ╰──────────────────────────── 1/10 ─────╯
```

### 6.6 Danger

Warnings say what will happen, using real context where it's cheap to get, then offer a safer command. Only critical cases ask for a second Enter.

```
~/data $ sort users.csv > users.csv█
         ⚠ The shell empties users.csv before sort reads it, so the data is lost.
           Safer: sort -o users.csv users.csv
```

```
~/shop $ git push --force█
         ⚠ Replaces the remote branch; commits others pushed meanwhile are lost.
           Safer: git push --force-with-lease
```

```
~ $ sudo dd if=ubuntu.iso of=/dev/sda bs=4M█
    ⚠ critical · overwrites all of /dev/sda: 512 GB SSD, mounted at / (your system disk)
      Enter again to run · any other key to keep editing
```

### 6.7 Pasted command

A paste opens the explain panel on its own.

```
~ $ curl -fsSL https://example.com/install.sh | sudo bash█
    ╭─ pasted · review before running ──────────────────────────────────╮
    │ curl -fsSL https://example.com/install.sh   download a script     │
    │   -f fail on HTTP errors · -s quiet · -S show errors · -L follow  │
    │ |                                           pipe it into…         │
    │ sudo bash                                   a root shell  ⚠       │
    ├───────────────────────────────────────────────────────────────────┤
    │ ⚠ Runs code from the internet as root, without showing it first.  │
    │   Safer: curl -fsSLo install.sh <url>, read it, then run it.      │
    ╰─ Enter runs · Esc closes · Alt+H docs ────────────────────────────╯
```

### 6.8 Full docs

Alt+H opens the full page, scrolled to the highlighted option. Tab switches between man, tldr and spec; in the tldr tab, Enter puts the highlighted example on your line. q takes you back to exactly where you were.

```
╭─ tar(1) ──────────────────── [man]  tldr  spec ──────────────────── q close ─╮
│ OPTIONS · Operation mode                                                     │
│ ▸ -x, --extract, --get                                                       │
│       Extract files from an archive. Arguments are optional. When given,     │
│       they specify names of the archive members to be extracted.             │
│   -t, --list                                                                 │
│       List the contents of an archive. Arguments are optional. When given,   │
│       they specify the names of the members to list.                         │
│                                                                              │
╰─ 31% · ↑↓ PgUp PgDn · / search · n next · Tab source · Enter insert example ─╯
```

### 6.9 AI (opt-in)

A line that starts with `#` is a shell comment, so it's a safe place to type a request.

```
~ $ # find files over 1 GB in this folder█
    ↳ Tab or Alt+A: ask AI for a command (Ollama, on this machine)
```

```
~ $ find . -type f -size +1G -exec ls -lh {} +█
    ╭─ AI suggestion · explained from your man pages ──────────╮
    │ find .              search this folder and below         │
    │  -type f            regular files only                   │
    │  -size +1G          bigger than 1 GiB                    │
    │  -exec ls -lh {} +  list them with readable sizes        │
    ╰─ Enter runs · Esc restores your # line · Alt+A retry ────╯
```

The AI writes the command, and terme explains it from your installed man pages, so an invented flag shows up as `? unknown option`. Nothing runs until you press Enter.

## 7. Behaviour

### 7.1 When the list opens

The list opens on its own when all of these are true:

- the shell is at its prompt, in insert mode (for vi users), and not in a paste or a history search;
- you changed the line by typing, not with ↑, Ctrl+R or a paste;
- you paused for 100 ms (`open_delay_ms`), so fast typing never flickers. Once open, it updates on every keystroke with no delay;
- there's something useful to show for the word under the cursor.

It stays closed, or closes, in these cases:

| Situation | Example | Shown instead |
|---|---|---|
| Empty line | `$ █` | Nothing (Ctrl+Space lists what you often run in this folder) |
| Inside quotes, a heredoc or a comment | `git commit -m "fix█` | Slot hint: `‹msg› commit message` |
| The word is complete and nothing longer matches | `git status█` | The reading |
| A value nobody can guess | `--port █` | Slot hint: `‹port› TCP port, e.g. 8080` |
| A path that doesn't have to exist yet | `ln -s ~/dotfiles/vimrc █` | Slot hint, until you type part of a path |
| Unknown command (no spec, man page or tldr) | `./deploy.sh --█` | File names if the word looks like a path; hint: `no docs for deploy.sh · Alt+H to learn from --help` |
| You pressed Esc | | Nothing until the next word, or Ctrl+Space |
| History search (Ctrl+R) | | Nothing |
| Screen smaller than 40 columns or 8 rows | | Hint line only |
| Not at a prompt | `vim`, `less`, `ssh`, a password prompt, a REPL | terme is off |

Other presets: `trigger = "calm"` opens the list only on Tab or Ctrl+Space (ghost text and the hint line stay live), and `"manual"` opens it only on Ctrl+Space.

### 7.2 Focus and keys (safe hybrid)

The list has two states, and the gutter shows which one you're in.

```
 Passive: the list just opened             Focused: after ↓
 ╭─ option ─────────────────────────╮      ╭─ option ─────────────────────────╮
 │▏--amend        redo last commit  │      │▌--amend        redo last commit  │
 │ --all  -a      stage all changes │      │ --all  -a      stage all changes │
 ╰─────────────────────────── 1/4 ──╯      ╰────────────────────────── 1/4 ↵ ─╯
 Tab inserts --amend                       Tab or Enter inserts --amend
 Enter runs the line as typed              ↑ on the first row: back to passive
 ↑ is shell history                        typing: back to passive
```

| Key | List closed | List passive (`▏`) | List focused (`▌`) |
|---|---|---|---|
| Tab | Open the list now (insert at once if there's exactly one match). After Esc, or with nothing to offer: the shell's own completion | Insert the `▏` item | Insert the `▌` item |
| Enter | Run the line | Run the line | Insert the `▌` item (doesn't run) |
| ↓ | Shell (history) | Move into the list, on the first item | Next item |
| ↑ | Shell (history) | Shell (history); the list closes | Previous item; from the first item, back to passive |
| → or End | Accept the ghost text if shown, else shell | Same | Shell |
| Alt+→ or Ctrl+→ | Accept one word of ghost text if shown, else shell | Same | Shell |
| Esc | Shell | Hide until the next word | Hide until the next word |
| Ctrl+Space | Open the list now (on an empty line: what you often run here) | Close the list | Close the list |
| PgUp / PgDn | Shell | Shell | Page through the list |
| Alt+↑ / Alt+↓ | | Scroll the docs pane | Scroll the docs pane |
| Alt+H | Full docs for the word at the cursor | Full docs for the `▏` item | Full docs for the `▌` item |
| Alt+E | Explain panel | Explain panel | Explain panel |
| Alt+A | AI action, if AI is on (§12) | Same | Same |
| Ctrl+C | Shell (cancels the line; overlays clear) | Same | Same |
| Anything else | Shell | Shell; the list filters | Shell; the list filters and goes back to passive |

Notes:

- In the focused state, Enter only inserts. A second Enter runs the line.
- Alt+H replaces zsh's `run-help`, which uses the same key. F1 isn't used because GNOME Terminal opens its help on F1.
- terme waits 25 ms to tell a lone Esc from an Alt+key sequence. In terminals that speak the kitty keyboard protocol (Kitty, Ghostty, foot…), there's no wait.
- If an input method (fcitx5, IBus) takes Ctrl+Space, Tab still opens the list. `terme doctor --keys` asks you to press each key and reports which ones the terminal or input method swallows.
- Other presets: `ide` (↑/↓ always move in the list; Enter and Tab both insert) and `shell` (Tab and Shift+Tab cycle; arrows and Enter keep their shell meaning). Every binding can be changed in the config.

### 7.3 Inserting

| Item | Inserted | Then |
|---|---|---|
| Command or subcommand | `name ` | The list for the next word opens after the usual pause |
| Option without a value | `--flag ` | Same |
| Option with a value, `--opt=VALUE` style | `--opt=` | Values (or a slot hint) show at once |
| Option with a separate value | `-o ` | Values (or a slot hint) show at once |
| Bundled short flag (`add to -xz`) | The letter, appended to the word; a space follows if it takes a value | The list updates in place, or values show |
| Directory | `dir/` | Its contents show at once |
| File or other value | `value ` | Next word |
| Ghost text | The rest of the line | Nothing else |

- The whole word under the cursor is replaced, even when the cursor is in the middle of it.
- The style you started is kept: type `--` and you get the long form; type `-` and you get the short one.
- Special characters are escaped (`my\ file.txt`). If you opened a quote, terme stays inside it and closes it.

### 7.4 Ranking and matching

1. **Match quality:** exact, then prefix, then word starts (`--ae` finds `--allow-empty`), then any subsequence, then a one-letter typo (`comit`). A word of 3 or more letters also searches descriptions, and those results appear under a `matched in docs` label (§6.4).
2. **Then usage:** how often you use it (this flag with this command, this branch, this folder), then how common it is (whether it appears in tldr examples, the spec's priority), then alphabetical order.
3. **Already used:** options already on the line are hidden unless they can repeat (`-v -v`). Options that conflict with ones you typed are muted with `conflicts with -c`. Deprecated ones go last, muted.
4. **Stability:** while you type within a word, the list filters but doesn't reshuffle, and the highlighted item keeps its place if it still matches.

### 7.5 Placement and size

1. **Size:** up to 8 rows (`max_rows`), 24–64 columns, and no more than 60% of the screen width.
2. **Vertical:** below the input line if it fits. Otherwise above it, covering older output that is restored exactly when the popup closes. Otherwise on the side with more room, shrinking the list down to 3 rows; below that, the hint line only.
3. **Horizontal:** the item names line up with the start of the word you're typing. If that would overflow the right edge, the popup shifts left.
4. **Docs pane:** to the right of the list if 32 or more columns remain; otherwise to the left; otherwise inside the list's box below a divider, up to 4 lines; otherwise hidden (Alt+H still works).
5. **Steadiness:** within one word the popup doesn't move unless it no longer fits.

| Screen width | Layout |
|---|---|
| 100 columns or more | List and docs side by side |
| 60–99 | Docs inside the list box (or beside it when the word starts near the left edge) |
| 40–59 | List only, with descriptions cut; Alt+H for docs |
| Under 40 columns, or under 8 rows | No popup; hint line only |

Narrow screen: the docs move inside the box.

```
~/shop $ git commit --a█
                  ╭─ option ─────────────────────────╮
                  │▏--amend        redo last commit  │
                  │ --all  -a      stage all changes │
                  │ --author=‹who› set the author    │
                  ├──────────────────────────────────┤
                  │ Replace the tip of the current   │
                  │ branch with a new commit.        │
                  │ ⚠ Rewrites history.  Alt+H more  │
                  ╰─────────────────────────── 1/4 ──╯
```

Prompt near the bottom: the popup flips above the input line and covers older output until it closes. The free row under the prompt stays empty while the list is open.

```
-rw-r--r--  1 you you 1.2K Oct 22 09:12 README.md
                  ╭─ option ─────────────────────────╮╭─ --amend ────────────────────────╮
                  │▏--amend        redo last commit  ││ Replace the tip of the current   │
                  │ --all  -a      stage all changes ││ branch by creating a new commit. │
                  ╰─────────────────────────── 1/4 ──╯╰─ man git-commit · git 2.43 ──────╯
~/shop $ git commit --a█

```

### 7.6 States

| State | Entered when | Shows | Leaves when |
|---|---|---|---|
| Off | A command is running, a full-screen app is open, a password prompt, no prompt marks, or `terme off` | Nothing | The shell reports a new prompt |
| Idle | At the prompt with an empty line | Nothing | You type, or press Ctrl+Space |
| Typing | The line isn't empty and the list is quiet | Ghost text, hint line | The open rules in §7.1 are met |
| Passive | The list opened on its own | List (`▏`), docs pane, ghost text | ↓ → Focused. Esc, a space or a quiet rule → Typing. Enter runs the line |
| Focused | ↓ was pressed in Passive | List (`▌` and `↵`), docs pane | Typing, or ↑ on the first item → Passive. Tab or Enter inserts → Typing. Esc → Typing |
| Explain | Alt+E, or a paste | Explain panel | Esc, Alt+E, typing (the reading stays), or Enter (runs) |
| Confirm | Enter on a critical command | Red hint: "Enter again to run" | Enter runs it. Any other key → Typing |
| Doc viewer | Alt+H | Full-screen docs | q or Esc → back to exactly where you were |

### 7.7 Teaching the model

- Once, under the first prompt after setup: `terme is on · Tab insert · ↓ browse · → ghost text · Alt+H docs · Esc hide · terme help`.
- Contextual tips on the hint line, at most one a day and six in total. For example, the first time Enter inserts: `Enter inserted because you were in the list (↓). Otherwise Enter runs the line.` Turn them off with `tips = false`.
- `terme tour`: a two-minute practice run in a sandbox shell.

## 8. Ghost text

- **Source:** your history. terme keeps its own log (command, folder, git repo, exit code, time), seeded from the shell's `HISTFILE` on first run.
- **Match:** what you typed must be the start of the suggestion (case-sensitive).
- **Ranking:** same folder first, then same repo, then frecency. Only commands that succeeded are suggested. Lines that start with a space (the `HISTCONTROL=ignorespace` convention) or that look like secrets (tokens, `password=`, `Authorization:`) are never suggested.
- **Display:** dim, only when the cursor is at the end of the line, cut at the screen edge rather than wrapped.
- **Accept:** → or End takes all of it; Alt+→ or Ctrl+→ takes one word.
- **With the list:** shown while the list is passive and hidden while it's focused, so only one thing previews at a time.
- **Coexistence:** it turns itself off when zsh-autosuggestions or ble.sh is active, and `terme doctor` says so.
- Enter never runs ghost text.

## 9. Explain the line

**Reading (live).** The hint line reads the line back in plain English: one short phrase per word, joined by `·`, with the values you typed filled in.

| Line | Reading |
|---|---|
| `tar -xzf backup.tgz -C /srv` | `extract · gunzip · from backup.tgz · into /srv` |
| `ps aux \| grep nginx` | `list all processes → keep lines matching nginx` |
| `find . -name "*.log" -mtime +7 -delete` | `in . · named *.log · modified over 7 days ago · ⚠ delete them` |
| `sudo apt update && sudo apt upgrade -y` | `as root: refresh package lists · then, if that worked, as root: upgrade everything · don't ask` |

- Phrases come from the specs' `phrase` field, hand-written for the top ~60 commands in v1. Otherwise they come from the first clause of the option's docs, cut to about four words.
- Operators: `|` becomes `→`, `&&` becomes `then, if that worked`, `||` becomes `otherwise`, `;` becomes `then`, `&` becomes `in the background`, `>` becomes `write to`, `>>` becomes `append to`, `2>&1` becomes `errors too`, and `sudo` becomes `as root:`.
- The reading covers only what you typed, never the ghost text. When it doesn't fit, it's cut with `…`, and Alt+E shows all of it.

**Explain panel (Alt+E).** One row per word. Bundled flags are split (`-xzvf` gives four rows), each command in a pipeline gets its own group, and operators get their own rows. Dangerous rows are red, and words with no docs show `?`. The footer names the sources. The panel follows the popup's placement rules. Esc or Alt+E closes it; so does typing, and the reading stays.

**On paste.** When pasted text contains a command terme recognizes, the panel opens on its own, titled `pasted · review before running` (§6.7). Set `explain.on_paste = false` to turn this off.

**Anywhere.** `terme explain '<command>'` prints the same breakdown, for use in scripts, docs and chats.

## 10. Danger warnings

| Level | What you see | Examples |
|---|---|---|
| Caution | A yellow `⚠` on the item and on the hint line | `git reset --hard`, `git clean -fd`, `docker system prune` |
| Danger | A red hint line with a safer alternative, and a red `⚠` on list rows | `rm -rf <dir>`, `git push --force`, `chmod -R 777`, `curl … \| sh`, `sort f > f` |
| Critical | Red, and Enter must be pressed twice | `rm -rf /`, `rm -rf ~`, `dd of=/dev/<disk>`, `mkfs` on a mounted disk, `chown -R … /`, a fork bomb |

- **Wording:** say in plain words what happens to the user's data, using real context where it's cheap to get (`lsblk` for disks, `git` for unpushed commits), then give a safer command.
- **Confirm:** only for critical commands. The second Enter must come next; any other key returns to editing. Set with `danger.confirm = "critical" | "all" | "never"`.
- **Rules are data:** a pattern, a level, a message, a safer alternative and an optional context probe. Each rule ships with tests against false positives. A pattern can be muted with `terme danger mute <rule>`.
- A warning never changes the line, and it never blocks anything except a critical command's first Enter.

## 11. Docs and sources

| Source | Gives | Used for |
|---|---|---|
| Curated specs (the MIT-licensed Fig specs, imported, plus terme's own fields) | Subcommands, options, argument types, value generators, `phrase`, danger flags | Structure and live values |
| Installed man pages | Full option docs, synopsis, wording that matches the installed version | Descriptions, docs pane, doc viewer |
| tldr (offline, updated weekly) | Common examples | Docs pane examples, the tldr tab in Alt+H, how common an option is (for ranking) |
| Your history | What you actually use | Ranking, ghost text |
| Generators (files, git, ssh config, systemd, Docker, package.json…) | Live values | Value lists |
| `<cmd> --help`, only when you ask | Options for tools with no man page or spec | Alt+H on an unknown tool asks: `Run "foo --help" to learn its options? This runs foo. [y/N]` |
| AI, opt-in | Explanations and commands on request | §12, always labelled |

- **When sources disagree:** the installed man page wins on wording and on whether an option exists; the spec wins on structure (arguments, generators). Options that are in a spec but not in your installed man page are marked `not in your version?`.
- **Index:** built in the background on first run, usable at once and complete within about a minute. It's refreshed when packages change, weekly for tldr, and stored in `~/.cache/terme`.
- **Attribution:** the Fig specs (MIT) and tldr (CC BY 4.0) are credited in `terme about` and in the doc viewer's tldr tab.

## 12. AI (opt-in)

- **Off** until `terme ai setup`. Providers: Ollama on this machine, the Anthropic API, or any OpenAI-compatible endpoint. Keys are stored in the system keyring.
- **One key, Alt+A,** does the obvious thing for where you are:
  - on a line that starts with `#`, it turns the sentence into a command. Tab works here too, and the hint line says so. Because `#` starts a comment, pressing Enter by mistake runs nothing;
  - on a normal line, it adds an AI explanation to the explain panel, for tools with no docs or for "why would I…" questions;
  - on an empty line right after a failed command, it suggests a fixed command.
- **Checked and labelled.** AI output never runs by itself. It's shown with terme's own explain panel, built from your installed docs, so an invented flag shows up as `? unknown option`. AI text carries a magenta `AI` label.
- **Consent.** Before the first request, terme shows exactly what will be sent: the line, the OS and shell names, and the versions of the tools in the line. It never sends history, environment variables or file contents, and it redacts anything that looks like a secret.
- Esc restores your `#` line.

## 13. Visual design

### 13.1 Colour roles

Colours come from the user's terminal. At startup terme asks the terminal for its background, foreground and palette (OSC 10, 11 and 4, with a 50 ms timeout) and derives everything from them.

| Role | Used for | Truecolor (derived) | 16-colour fallback | Mono |
|---|---|---|---|---|
| `surface` | Popup background | mix(bg, fg, 6%) | Default background | Default |
| `border` | Box lines, dividers | mix(bg, fg, 30%) | Bright black | Default |
| `text` | Names | fg | Default | Default |
| `muted` | Descriptions, ghost text, meta | mix(bg, fg, 65%), raised until it reaches 4.5:1 against `surface` | Bright black, or dim | Dim |
| `accent` | Focused row, `▏` | mix(bg, palette blue, 35% on dark, 20% on light) behind fg | Blue background, bright white text | Reverse video |
| `match` | Typed characters in names | Palette blue, bold | Blue, bold | Bold |
| `warn` | Caution | Palette yellow | Yellow | `⚠` + bold |
| `danger` | Danger, critical | Palette red | Red, bold | `⚠` + bold; reverse video for critical |
| `ai` | AI label | Palette magenta | Magenta | `[AI]` |

Worked examples:

- **Dark** (bg `#1e1e1e`, fg `#d4d4d4`, blue `#2472c8`): surface `#292929`, border `#555555`, muted `#949494`, accent `#203b59`.
- **Light** (bg `#ffffff`, fg `#333333`, blue `#0451a5`): surface `#f3f3f3`, border `#c2c2c2`, muted `#6f6f6f`, accent `#cddced`.

In 16-colour mode, terme checks the contrast of the palette slots it uses (when the terminal reports them) and falls back to dim default text if bright black is unreadable, as it is in some Solarized setups.

### 13.2 Type and glyphs

- **Bold:** names. **Muted:** descriptions, ghost text, meta. **Italic:** placeholders like `‹FILE›` (muted if the terminal has no italics). **Underline:** the current slot in the hint line. **Strikethrough:** deprecated items (otherwise muted, plus the word `deprecated`).
- **Glyph sets:** `unicode` (default), `ascii` (automatic on the Linux console, `TERM=linux`, or when setup reports broken symbols), and `nerd` (opt-in).
- **Borders:** `rounded` `╭╮╰╯` (default), `square` `┌┐└┘`, `ascii` `+-|`, `none`.
- **Widths:** at startup terme measures how wide the terminal draws ambiguous-width characters and adjusts the layout. This matters for CJK setups.

### 13.3 Spacing and chrome

- **Row:** gutter (1) · kind (2, when shown) · name · at least 2 spaces · description · 1 space of padding.
- **Docs pane:** 1 space of padding on each side, and one blank line between blocks.
- **Labels:** lowercase and muted in the top border; the position and `↵` in the bottom border.
- **Motion:** none. Popups appear and disappear in a single frame. No spinner for waits under 150 ms; after that, a muted `loading…` row.
- **No flicker:** one write per frame, synchronized output (DEC mode 2026) where the terminal supports it, and the cursor hidden while drawing and put back exactly where it was.

### 13.4 Voice

Short, lowercase labels and plain verbs (`extract files`, not "Extracts files from the archive."). Warnings say what happens to your data, then give a safer command. No exclamation marks, no emoji, no jokes.

## 14. Onboarding and settings

### 14.1 Install

A `.deb` from an apt repository (Ubuntu and Debian), Homebrew (Linux and macOS), `cargo install terme`, or a static tarball. There's no `curl … | sh` installer as the main route, since terme warns about that pattern itself.

### 14.2 `terme setup`

Three short steps, all inside the terminal.

```
$ terme setup

  terme · suggestions and docs as you type                           1/3

  Do these symbols look right?    ▸ ▤ ▦ ⎇ ● ↺ ⚠   ╭─╮ ╰─╯

  ▏Yes, they look fine
   Some are empty boxes or overlap, so use plain ASCII

  ↑↓ choose · Enter confirm · q quit
```

```
  How the keys work                                                  2/3

  Tab     insert the highlighted suggestion
  ↓       move into the list; then Enter inserts too
  Enter   otherwise runs your line exactly as typed
  →       take the dim ghost text
  Esc     hide suggestions until the next word
  Alt+H   full docs        Alt+E   explain the line

  Enter continue · p try another preset (ide, shell)
```

```
  Start terme in every new bash session?                             3/3

  Adds two marked blocks to ~/.bashrc (backup: ~/.bashrc.terme-bak)
    top     runs bash inside terme, for interactive terminals only
    bottom  loads terme's prompt hooks

  ▏Yes, add them
   No, I'll run `terme` when I want it

  Afterwards, open a new tab and type:  git c
```

`terme setup --remove` takes the blocks out again.

### 14.3 Day to day

The first prompt after setup shows this tip once:

```
~ $ █
    terme is on · Tab insert · ↓ browse · → ghost text · Alt+H docs · Esc hide · terme help
```

`terme doctor` explains what terme sees:

```
$ terme doctor
  ✓ shell      bash 5.2 · hooks loaded · prompt marks seen
  ✓ terminal   Ptyxis (VTE) · truecolor · dark background
  ✓ keys       Alt sends an ESC prefix · Esc wait 25 ms
  ! conflict   ble.sh is active, so terme's ghost text is off
  ✓ docs       1,284 commands · 912 man pages · 4,620 tldr pages · updated 2 days ago
  ✓ speed      p50 2.1 ms · p99 9.8 ms over the last 1,000 keystrokes
  · ai         off · turn on with `terme ai setup`
```

| Command | What it does |
|---|---|
| `terme` | Start a shell inside terme (manual mode) |
| `terme setup` | The first-run wizard; `--remove` undoes the rc changes |
| `terme init bash` / `terme init zsh` | Print the hook snippet, for dotfile managers |
| `terme on` / `terme off` | Turn suggestions on or off for this session; `--everywhere` for all sessions |
| `terme doctor` | Check the terminal, keys, hooks, conflicts, index and latency; `--keys` tests each binding |
| `terme explain '<cmd>'` | Print the explain panel as plain text |
| `terme docs <cmd> [option]` | Open the doc viewer directly |
| `terme index [--rebuild]` | Refresh the docs index (normally automatic) |
| `terme config` | Open the config in `$EDITOR` (a settings screen can come later) |
| `terme ai setup` / `terme ai off` | Choose an AI provider, or turn AI off |
| `terme history forget <pattern>` | Remove matching entries from terme's history |
| `terme danger mute <rule>` | Stop one warning rule |
| `terme stats` | Local-only numbers: accept rate, top commands |
| `terme tour` | A two-minute practice run |

### 14.4 Config

`~/.config/terme/config.toml`. Every key is optional; Appendix A lists the defaults with comments.

## 15. Compatibility and edge cases

### 15.1 Terminals

| Tier | Terminals |
|---|---|
| 1 (tested every release) | Ptyxis and GNOME Terminal (both VTE), Konsole, Kitty, Alacritty, WezTerm, Ghostty, VS Code's terminal, tmux |
| 2 (best effort) | foot, Tilix, Terminator, xterm, GNU screen, the Linux console (ASCII glyphs, 16 colours) |

### 15.2 Living with other tools

- **VS Code:** it has its own terminal suggestions (`terminal.integrated.suggest.enabled`). Setup notices and asks which to keep. terme passes VS Code's shell-integration sequences through.
- **tmux, screen:** each pane gets its own terme through the rc blocks. Detection uses the TTY, not only an environment variable, because panes inherit the environment.
- **zsh:** zsh-autosuggestions turns terme's ghost text off. zsh-autocomplete conflicts, so setup warns. fzf, fzf-tab, oh-my-zsh, powerlevel10k and starship are fine; multi-line prompts work because the popup anchors to the input line.
- **bash:** ble.sh turns ghost text off, and setup warns about the popup. bash-completion is still used when Tab falls through to the shell. atuin and mcfly keep working, because ↑ and Ctrl+R reach them unchanged.
- **vi mode:** suggestions appear only in insert mode. `esc_passthrough = true` makes one Esc both hide the popup and switch to normal mode.
- **Terminal features:** terme passes through OSC 7 (working directory), OSC 8 (links), OSC 133 (prompt marks), window titles and bracketed paste, so the terminal's own features keep working.

### 15.3 Edge cases

| Case | Behaviour |
|---|---|
| Window resize | Overlays vanish at once and come back on the next keystroke |
| A long line wraps, or a multi-line command (`\`, an open quote) | The popup anchors at the word's real row and the whole command is parsed. In a multi-line command, ↓ enters the list only from the last line |
| A background job prints while you type | Overlays are erased before the output and redrawn after it |
| Password prompt (echo off) | Nothing shown, nothing recorded |
| A folder with 10,000+ entries | The first 200, then a `keep typing to filter` row |
| Slow generator (`kubectl get pods`) | `loading…` after 150 ms, a 2 s timeout, results cached for 30 s; typing is never blocked |
| Images (sixel, kitty graphics) in recent output | Covered cells can't be restored exactly, so the popup only opens below (scrolling if needed) or falls back to the hint line |
| A shell started from the shell (`bash`, `zsh`) | Works: its hooks report to the same terme |
| `sudo -i`, `su` | Root's rc has no hooks, so terme stays off |
| A shell started without hooks (`bash --norc`) | No prompt marks, so terme stays off |
| Scripts that send keystrokes (`expect`, `tmux send-keys`) | Set `TERME=off`, or `danger.confirm = "never"` |
| terme itself fails | The shell keeps running; terme drops to passthrough and shows `suggestions paused · terme doctor` once |

## 16. Performance, privacy, accessibility

| Budget | Target |
|---|---|
| Delay added to a keystroke reaching the shell | Under 1 ms |
| Overlay update after a keystroke | p50 ≤ 4 ms, p99 ≤ 16 ms |
| Deliberate pause before the list opens | 100 ms (configurable, 0–500) |
| Added shell startup time | ≤ 15 ms |
| Memory per session | ≤ 30 MB (the index is shared between sessions) |
| First-run indexing | In the background, never blocking |

**Privacy.** No network traffic except the weekly tldr update (which can be turned off) and AI requests when AI is enabled. No telemetry; `terme stats` shows local numbers only. History lives in `~/.local/share/terme/history.db` with mode 0600. It skips space-prefixed commands and redacts secret-looking text, and `terme history forget <pattern>` removes entries.

**Accessibility.** Colour is never the only signal: there are gutter marks, `⚠` with words, and `↵`. Muted text is raised to 4.5:1 contrast where the terminal reports its colours. There's a `high-contrast` theme, and `NO_COLOR` gives the mono style. Overlays can confuse screen readers, so `accessibility = true` turns them off and keeps the hint line as plain text. Everything works from the keyboard.

## 17. Platforms

- **Linux (v1):** Ubuntu 22.04 and newer first; Debian, Fedora and Arch also work. x86_64 and arm64.
- **macOS (v1.x, the same `unix` code):** zsh is the default shell. Terminal.app and iTerm2 need "Use Option as Meta" (iTerm2: "Esc+") for the Alt keys; `terme doctor` detects this and offers alternative bindings. BSD tools differ from GNU ones, but docs come from the installed man pages, so the flags are right, and specs mark GNU and BSD variants.
- **Windows (v2):** ConPTY and Windows Terminal. PowerShell 7, with docs from `Get-Help` and parameters from `Get-Command`. It must coexist with PSReadLine's predictions (setup picks one). cmd gets basic support, and WSL runs the Linux build. Avoid Alt+Enter, which toggles fullscreen in Windows Terminal.

**Shared:** the engine, specs, ranking, renderer, keymap and config. **Per platform:** PTY setup, terminal and font detection, key defaults, package paths and doc sources.

## 18. Roadmap and success metrics

| Milestone | What users get | Done when |
|---|---|---|
| M0 Tracer | Wrapper, bash and zsh hooks, the list for 10 hand-made specs, Tab/↓/Enter/Esc | The latency budget is met, and a day of dogfooding leaves nothing in scrollback |
| M1 Docs | Man pages for every installed command, docs pane, hint line, Fig spec import, ranking | The top 50 commands feel complete |
| M2 Understand | Reading, explain panel, paste review, danger rules and confirm, doc viewer | No false critical warnings on a test corpus of real commands |
| M3 Flow | Ghost text, setup, doctor, tips, config, themes, ASCII mode | A new user is comfortable within two minutes |
| M4 macOS | The same build on macOS (zsh, Terminal.app, iTerm2) | The same test suite passes |
| M5 AI | Opt-in providers and Alt+A | AI output is always checked and labelled |
| Later | fish; Windows and PowerShell; a `terme ssh` bootstrap; a native-completion bridge; validation and typo hints; help after failed commands | |

**Signals that it works** (from `terme stats`; telemetry only if it ever becomes opt-in):

- suggestions accepted per 100 commands: 15 or more;
- auto-opened popups dismissed with Esc: under 15% (above that, tune the quiet rules);
- inserts undone within 2 seconds (a proxy for wrong inserts): under 3%;
- still turned on after 14 days: 60% or more.

## 19. Open questions

1. Is "terme" the final name?
2. License and business model: an open-source core, paid extras, or something else? Either way, the Fig specs (MIT) and tldr (CC BY 4.0) need credit.
3. Who writes the plain-English phrases for the top ~60 commands? The reading line's quality depends on them.
4. Who approves new critical danger rules, and how are false positives reported?
5. Should Ctrl+Space on an empty line also offer project commands (package.json scripts, Makefile targets), not just history?
6. Should the native shell-completion bridge come in v1.x, for coverage?

## Appendix A. Default config

```toml
# ~/.config/terme/config.toml
# Every key is optional. The values below are the defaults.

accessibility = false      # true: no overlays; the hint line only, as plain text

[popup]
trigger       = "auto"     # auto | calm (list only on Tab) | manual (Ctrl+Space only)
open_delay_ms = 100        # pause before the list opens; updates are instant once open
max_rows      = 8
docs_pane     = "auto"     # auto | right | below | off
placement     = "auto"     # auto (below, else above) | below | above

[hint_line]
mode    = "below"          # below (keeps one row free under a new prompt) | right | off
reading = true             # plain-English reading of the line

[ghost]
enabled = "auto"           # auto (off if zsh-autosuggestions or ble.sh is active) | on | off

[keys]
preset          = "hybrid" # hybrid | ide | shell
toggle          = "ctrl+space"
docs            = "alt+h"
explain         = "alt+e"
ai              = "alt+a"
esc_passthrough = false    # true: Esc hides the popup and also reaches the shell (vi mode)

[look]
style  = "rounded"         # rounded | square | ascii | none
glyphs = "unicode"         # unicode | ascii | nerd
theme  = "auto"            # auto (from the terminal's colours) | dark | light | high-contrast | mono

[explain]
on_paste = true

[danger]
warnings = true
confirm  = "critical"      # critical | all | never

[sources]
man           = true
specs         = true
tldr          = true
tldr_update   = "weekly"   # weekly | never
run_help_flag = "ask"      # ask | never; terme never runs a program on its own to read its --help

[ai]
enabled  = false
provider = "ollama"        # ollama | anthropic | openai-compatible
model    = ""              # empty: the provider's recommended model

[privacy]
record_history        = true   # local only, mode 0600
ignore_space_prefixed = true

[tips]
enabled = true

[disable]
commands = []              # never suggest for these, e.g. ["mysql", "psql"]
```

## Appendix B. Implementation notes

| Concern | Plan |
|---|---|
| PTY | `portable-pty`: Unix PTYs now, ConPTY on Windows later |
| Shadow screen | `alacritty_terminal`, fed the same bytes as the real terminal, so covered cells can be restored and the cursor position is always known |
| Drawing | Escape sequences through `crossterm`, one write per frame, synchronized output where supported; `ratatui` only for full-screen views (doc viewer, setup) |
| Parsing the line | `tree-sitter-bash`; zsh-only syntax handled on a best-effort basis |
| Fuzzy matching | `nucleo` |
| Specs | Fig specs compiled to JSON at build time; their generator and post-processing functions run in embedded QuickJS (`rquickjs`) with time limits |
| Storage | SQLite (`rusqlite`) for the index and history, in XDG directories |
| Config, secrets | `serde` and `toml`; the `keyring` crate for AI keys |

**Shell hooks**

- **zsh:** `precmd` and `preexec` emit prompt marks (OSC 133 A–D), the exit code and the folder (OSC 7). `zle-line-pre-redraw` and `zle-keymap-select` report the line, cursor and keymap on every redraw through a private escape sequence, which terme removes before it reaches the terminal. The line model is exact.
- **bash:** `PROMPT_COMMAND` and `PS0` emit the same marks. Readline has no per-keystroke hook, so terme reads the line from the shadow screen between the end of the prompt and the cursor. When it matters (before an insert), it resyncs by sending a key bound with `bind -x` to a function that reports `READLINE_LINE` and `READLINE_POINT`, and hides the redraw that follows. vi mode is detected through `show-mode-in-prompt` mode strings that carry a private marker. If the screen read is ambiguous, terme shows nothing.
- **rc blocks:** the top block re-runs the shell inside terme only for interactive shells on a TTY, with `TERM` not set to `dumb`, and only if this TTY isn't already inside terme. The bottom block loads the hooks. `TERME=off` skips both. Non-interactive shells (scp, rsync, cron, editors resolving the environment) are never touched.
- **Inserting:** terme sends keystrokes to the shell (backspaces, then text), queued ahead of anything you type next, and only in emacs or vi insert mode.
- **Nesting:** an inner terme, for example on a server you SSH into, announces itself with a private sequence. The outer one stays passive until that session ends.
- **Failure isolation:** the PTY relay runs apart from the engine (on its own thread with panic recovery, or as a child process). If the engine fails, terme switches to passthrough and leaves one note.
