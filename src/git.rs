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
