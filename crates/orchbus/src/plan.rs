//! Capture the plan a plan-mode agent produced, so the diff reviewer can check
//! the implementation against it (the "Spec" axis).
//!
//! The plan is read from the agent's session transcript (see
//! `orchbus_agent::transcript`) and written to `.orchbus/plans/<slug>.md`.

use crate::{git, state};
use anyhow::{Context, Result};
use orchbus_agent::transcript::{extract_plan, transcript_path};
use std::path::PathBuf;

/// Path of the captured plan artifact for a slug.
fn artifact_path(slug: &str) -> Result<PathBuf> {
    Ok(git::orchbus_dir()?.join("plans").join(format!("{slug}.md")))
}

/// Read the slug's transcript, extract the latest plan, and write it to
/// `.orchbus/plans/<slug>.md`. Returns the artifact path.
pub fn capture(slug: &str) -> Result<PathBuf> {
    let entry = state::get(slug)?;
    let transcript = transcript_path(&entry.worktree, &entry.session_id)?;
    let jsonl = std::fs::read_to_string(&transcript)
        .with_context(|| format!("reading transcript {}", transcript.display()))?;
    let plan = extract_plan(&jsonl)
        .with_context(|| format!("no plan found in session for '{slug}' (has the agent produced a plan yet?)"))?;

    let out = artifact_path(slug)?;
    if let Some(parent) = out.parent() {
        std::fs::create_dir_all(parent).with_context(|| format!("creating {}", parent.display()))?;
    }
    std::fs::write(&out, &plan).with_context(|| format!("writing {}", out.display()))?;
    Ok(out)
}

/// The plan artifact, capturing it first if it isn't on disk yet (used by review).
#[allow(dead_code)] // consumed by `review` (Track B5)
pub fn ensure(slug: &str) -> Result<PathBuf> {
    let out = artifact_path(slug)?;
    if out.exists() {
        return Ok(out);
    }
    capture(slug)
}

/// Read the captured plan text, erroring if it hasn't been captured.
#[allow(dead_code)] // consumed by `review` (Track B5)
pub fn read(slug: &str) -> Result<String> {
    let path = ensure(slug)?;
    std::fs::read_to_string(&path).with_context(|| format!("reading {}", path.display()))
}
