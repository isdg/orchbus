//! Thin wrappers around `tmux` and `fzf`.

use anyhow::{Context, Result};
use std::ffi::OsStr;
use std::io::Write;
use std::process::{Command, Stdio};

/// Run `tmux <args>` and return stdout (lossy UTF-8), trailing newline trimmed.
pub fn query<I, S>(args: I) -> Result<String>
where
    I: IntoIterator<Item = S>,
    S: AsRef<OsStr>,
{
    let out = Command::new("tmux")
        .args(args)
        .output()
        .context("failed to run tmux (is it on PATH?)")?;
    let mut s = String::from_utf8_lossy(&out.stdout).into_owned();
    if s.ends_with('\n') {
        s.pop();
    }
    Ok(s)
}

/// Run `tmux <args>` for its side effect.
pub fn run<I, S>(args: I) -> Result<()>
where
    I: IntoIterator<Item = S>,
    S: AsRef<OsStr>,
{
    Command::new("tmux")
        .args(args)
        .status()
        .context("failed to run tmux")?;
    Ok(())
}

/// Spawn an interactive `fzf <args>`, feeding `input` as the initial list. fzf
/// draws on /dev/tty and drives all actions through its own `--bind`s (which call
/// back into this binary), so we don't capture a selection — we just run to exit.
pub fn fzf_interactive(args: &[String], input: String) -> Result<()> {
    let mut child = Command::new("fzf")
        .args(args)
        .stdin(Stdio::piped())
        .spawn()
        .context("failed to spawn fzf (>= 0.36 required)")?;

    let mut stdin = child.stdin.take().expect("piped stdin");
    let writer = std::thread::spawn(move || {
        let _ = stdin.write_all(input.as_bytes());
    });
    child.wait().context("fzf wait failed")?;
    let _ = writer.join();
    Ok(())
}
