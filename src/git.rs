//! Small `git` helpers. `spawn`/`review`/`fork` all anchor to the repo root so
//! `.orchbus/` (state, worktrees, plans, reviews) lives in one place regardless of
//! which subdirectory a command is run from.

use anyhow::{bail, Context, Result};
use std::path::PathBuf;
use std::process::Command;

/// Absolute path of the enclosing git repository's top level.
pub fn root() -> Result<PathBuf> {
    let out = Command::new("git")
        .args(["rev-parse", "--show-toplevel"])
        .output()
        .context("failed to run git (is it on PATH?)")?;
    if !out.status.success() {
        bail!("not inside a git repository");
    }
    let path = String::from_utf8_lossy(&out.stdout).trim().to_string();
    Ok(PathBuf::from(path))
}

/// The repo-root-anchored `.orchbus/` directory (created on demand by callers).
pub fn orchbus_dir() -> Result<PathBuf> {
    Ok(root()?.join(".orchbus"))
}

/// Full commit sha of `HEAD` — the base a spawned worktree forks from.
pub fn head() -> Result<String> {
    let out = Command::new("git")
        .args(["rev-parse", "HEAD"])
        .output()
        .context("failed to run git")?;
    if !out.status.success() {
        bail!("git rev-parse HEAD failed (no commits yet?)");
    }
    Ok(String::from_utf8_lossy(&out.stdout).trim().to_string())
}

/// The change a worktree has made since `base`: tracked diff vs the working tree,
/// plus a trailing list of new untracked files (which a plain `diff` omits).
pub fn diff(worktree: &std::path::Path, base: &str) -> Result<String> {
    let tracked = Command::new("git")
        .arg("-C")
        .arg(worktree)
        .args(["diff", base, "--"])
        .output()
        .context("failed to run git diff")?;
    if !tracked.status.success() {
        bail!("git diff {base} failed in {}", worktree.display());
    }
    let mut out = String::from_utf8_lossy(&tracked.stdout).into_owned();

    let untracked = Command::new("git")
        .arg("-C")
        .arg(worktree)
        .args(["ls-files", "--others", "--exclude-standard"])
        .output()
        .context("failed to list untracked files")?;
    let list = String::from_utf8_lossy(&untracked.stdout);
    if !list.trim().is_empty() {
        out.push_str("\n# untracked (new) files:\n");
        for f in list.lines() {
            out.push_str("#   ");
            out.push_str(f);
            out.push('\n');
        }
    }
    Ok(out)
}

/// `git worktree add -b <branch> <path> <base>` — a fresh branch in its own tree.
pub fn add_worktree(path: &std::path::Path, branch: &str, base: &str) -> Result<()> {
    let status = Command::new("git")
        .arg("worktree")
        .arg("add")
        .arg("-b")
        .arg(branch)
        .arg(path)
        .arg(base)
        .status()
        .context("failed to run git worktree add")?;
    if !status.success() {
        bail!("git worktree add failed for {}", path.display());
    }
    Ok(())
}
