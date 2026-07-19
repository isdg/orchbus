//! Thin wrappers around `tmux` and `fzf`.

use anyhow::{bail, Context, Result};
use std::ffi::OsStr;
use std::io::Write;
use std::process::{Command, Stdio};

/// Is a tmux *server* running? (server-level — true even when not attached, so a
/// shell outside tmux can still drive `scan`/`approve`/`cancel`/`list`/`status`
/// against a live server.) `tmux has-session` exits non-zero when no server runs.
pub fn server_running() -> bool {
    Command::new("tmux")
        .arg("has-session")
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
}

/// Are we *inside* a tmux client? (client-level — `$TMUX` is set only when
/// attached.) Required by commands that switch the client or open a window.
pub fn inside() -> bool {
    std::env::var_os("TMUX").is_some()
}

/// Guard for commands that read/act on panes: a tmux server must be running.
pub fn require_server() -> Result<()> {
    if !server_running() {
        bail!("no tmux server running (start tmux first)");
    }
    Ok(())
}

/// Guard for commands that switch the client or open a window: must be inside tmux.
pub fn require_inside() -> Result<()> {
    if !inside() {
        bail!("not inside a tmux session (run this from a tmux client)");
    }
    Ok(())
}

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
