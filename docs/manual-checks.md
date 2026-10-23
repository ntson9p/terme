# Manual checks

The automated tests run terme headlessly. These checks need a person at a real terminal window, because they're about how things look and feel: fonts, colours, keys the terminal or input method might take. They take about 10 minutes.

## Setup

In Ubuntu (or WSL):

```sh
cd terme && git pull && cargo build --release
./target/release/terme
```

Try it in as many of these as you have: **Ptyxis or GNOME Terminal**, **Konsole**, **Kitty**, **VS Code's terminal**, **tmux** inside any of them. Under WSL on Windows 11 you can install and open Linux terminals too (`sudo apt install gnome-terminal`, then run `gnome-terminal`), but a real Ubuntu install or VM gives the truest results.

## The checks

| # | Do this | Expect | OK? |
|---|---|---|---|
| 1 | Type `git c` and pause | A rounded box under `c` listing `commit`, `checkout`… The box lines are joined, not broken or offset | |
| 2 | Look at the box's symbols | `▏`, `╭─╮`, `‹›` render cleanly in your font, nothing shows as an empty box | |
| 3 | Press ↓ | The first row turns into a solid highlighted bar, and `↵` appears in the bottom border | |
| 4 | Press Enter | `git commit ` is on the line; nothing ran | |
| 5 | Type `--a`, pause, press Esc | The list closes; the screen looks exactly as before | |
| 6 | Press Ctrl+C, then type `ls -` and pause | The list shows `-l`, `-a`… Press Ctrl+Space: it closes. Press Ctrl+Space again: it opens | |
| 7 | Run `seq 1 100`, then type `git c` | The list appears **above** the prompt; Esc restores the numbers under it exactly | |
| 8 | Type `tar -xz`, pause, press Tab twice | `f` is added, then an archive name (if the folder has one) | |
| 9 | Run `vim` or `less /etc/hostname`, type `git c` inside | No list appears in full-screen apps | |
| 10 | Type fast: `git status` and Enter quickly | No list flashes on screen while you type | |
| 11 | Resize the window with the list open | Note what happens (known rough edge in M0) | |
| 12 | General feel | Is the 100 ms pause right? Too eager, too slow? | |

If you use an input method (fcitx5, IBus with Vietnamese or another language), also check that Ctrl+Space reaches terme: if pressing it does nothing in step 6, your input method is taking it.

## Reporting back

Put screenshots in a `feedback/` folder at the root of the repo (git ignores it), with a line or two about what felt off. Or just tell Claude what you saw; describing it is enough.
