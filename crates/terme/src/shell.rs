//! Starting the user's shell with terme's hooks loaded, without editing any of
//! the user's dotfiles: bash gets an `--rcfile`, zsh a temporary `ZDOTDIR`.

use std::fs;
use std::io::Write;
use std::os::unix::fs::{DirBuilderExt, OpenOptionsExt};
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use portable_pty::CommandBuilder;

pub const BASH_HOOKS: &str = include_str!("hooks/terme.bash");
pub const ZSH_HOOKS: &str = include_str!("hooks/terme.zsh");

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ShellKind {
    Bash,
    Zsh,
}

impl ShellKind {
    #[must_use]
    pub fn detect(path: &Path) -> Option<Self> {
        match path.file_name()?.to_str()? {
            "bash" => Some(Self::Bash),
            "zsh" => Some(Self::Zsh),
            _ => None,
        }
    }
}

/// The shell to run: `--shell`, then `$TERME_SHELL`, `$SHELL`, `/bin/bash`.
#[must_use]
pub fn choose(flag: Option<PathBuf>) -> PathBuf {
    flag.or_else(|| std::env::var_os("TERME_SHELL").map(PathBuf::from))
        .or_else(|| std::env::var_os("SHELL").map(PathBuf::from))
        .unwrap_or_else(|| PathBuf::from("/bin/bash"))
}

/// A private folder with the startup files; removed when dropped.
pub struct HookDir {
    pub path: PathBuf,
}

impl Drop for HookDir {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.path);
    }
}

fn write_file(dir: &Path, name: &str, contents: &str) -> Result<()> {
    let path = dir.join(name);
    let mut f = fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(&path)
        .with_context(|| format!("creating {}", path.display()))?;
    // Files written on Windows checkouts may have CRLF; shells choke on it.
    f.write_all(contents.replace("\r\n", "\n").as_bytes())?;
    Ok(())
}

fn quote(path: &Path) -> String {
    format!("'{}'", path.display().to_string().replace('\'', r"'\''"))
}

/// Create the startup files for `kind` and the command that runs the shell.
pub fn prepare(shell: &Path, kind: ShellKind) -> Result<(CommandBuilder, HookDir)> {
    let base = std::env::var_os("XDG_RUNTIME_DIR")
        .map(PathBuf::from)
        .filter(|p| p.is_dir())
        .unwrap_or_else(std::env::temp_dir);
    let path = base.join(format!("terme-{}", std::process::id()));
    let _ = fs::remove_dir_all(&path);
    fs::DirBuilder::new()
        .mode(0o700)
        .create(&path)
        .with_context(|| format!("creating {}", path.display()))?;
    let dir = HookDir { path };
    let p = &dir.path;

    let mut cmd = CommandBuilder::new(shell);
    match kind {
        ShellKind::Bash => {
            write_file(p, "terme.bash", BASH_HOOKS)?;
            write_file(
                p,
                "bashrc",
                &format!(
                    "# Written by terme: your usual bashrc, then terme's hooks.\n\
                     if [ -f \"$HOME/.bashrc\" ]; then . \"$HOME/.bashrc\"; fi\n\
                     . {}\n",
                    quote(&p.join("terme.bash"))
                ),
            )?;
            cmd.args(["--rcfile", &p.join("bashrc").to_string_lossy(), "-i"]);
        }
        ShellKind::Zsh => {
            write_file(p, "terme.zsh", ZSH_HOOKS)?;
            // zsh reads $ZDOTDIR/.zshenv, .zprofile, .zshrc and .zlogin. Each of
            // ours runs the user's own file from their real ZDOTDIR.
            let user = std::env::var_os("ZDOTDIR")
                .map_or_else(|| "$HOME".to_string(), |z| quote(Path::new(&z)));
            write_file(
                p,
                ".zshenv",
                &format!(
                    "# Written by terme.\n\
                     typeset -g __terme_dir=$ZDOTDIR\n\
                     ZDOTDIR={user}\n\
                     [[ -f $ZDOTDIR/.zshenv ]] && source $ZDOTDIR/.zshenv\n\
                     typeset -g __terme_user_zdotdir=$ZDOTDIR\n\
                     ZDOTDIR=$__terme_dir\n"
                ),
            )?;
            write_file(
                p,
                ".zprofile",
                "# Written by terme.\n\
                 ZDOTDIR=$__terme_user_zdotdir\n\
                 [[ -f $ZDOTDIR/.zprofile ]] && source $ZDOTDIR/.zprofile\n\
                 ZDOTDIR=$__terme_dir\n",
            )?;
            write_file(
                p,
                ".zshrc",
                "# Written by terme: your usual zshrc, then terme's hooks.\n\
                 ZDOTDIR=$__terme_user_zdotdir\n\
                 [[ -f $ZDOTDIR/.zshrc ]] && source $ZDOTDIR/.zshrc\n\
                 source $__terme_dir/terme.zsh\n",
            )?;
            cmd.env("ZDOTDIR", p.as_os_str());
            cmd.arg("-i");
        }
    }
    Ok((cmd, dir))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detects_supported_shells() {
        assert_eq!(
            ShellKind::detect(Path::new("/usr/bin/zsh")),
            Some(ShellKind::Zsh)
        );
        assert_eq!(
            ShellKind::detect(Path::new("/bin/bash")),
            Some(ShellKind::Bash)
        );
        assert_eq!(ShellKind::detect(Path::new("/usr/bin/fish")), None);
    }

    #[test]
    fn hooks_have_unix_line_endings_and_no_status_variable() {
        for hooks in [BASH_HOOKS, ZSH_HOOKS] {
            assert!(
                !hooks.contains("local status"),
                "zsh's $status is read-only"
            );
        }
        assert!(ZSH_HOOKS.contains("6973;line;"));
        assert!(BASH_HOOKS.contains("133;B"));
    }
}
