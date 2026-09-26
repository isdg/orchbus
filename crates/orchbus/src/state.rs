//! Per-slug record of an orchbus-spawned agent, persisted to `.orchbus/state.json`.
//!
//! This is what makes fork/resume/review deterministic: spawn pins a `session_id`
//! and records where the agent lives (worktree, branch, base commit) under a short
//! `slug`, so later verbs (`review`/`revise`/`fork`) can find it by slug alone.

use crate::git;
use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::PathBuf;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Entry {
    pub slug: String,
    /// Absolute path of the agent's git worktree.
    pub worktree: String,
    /// Branch the worktree is on (`orchbus/<slug>`).
    pub branch: String,
    /// Commit the branch forked from — the review diff base.
    pub base: String,
    /// The tag (role) the agent was spawned with.
    pub tag: String,
    /// Agent command (e.g. `claude`).
    pub agent: String,
    /// Pinned session id, for resume/fork.
    pub session_id: String,
    /// Session id of the most recent headless review, if any.
    #[serde(default)]
    pub review_session_id: Option<String>,
}

fn path() -> Result<PathBuf> {
    Ok(git::orchbus_dir()?.join("state.json"))
}

/// Load the whole slug→Entry map (empty if the store doesn't exist yet).
pub fn load() -> Result<BTreeMap<String, Entry>> {
    let p = path()?;
    match std::fs::read_to_string(&p) {
        Ok(body) => serde_json::from_str(&body).with_context(|| format!("parsing {}", p.display())),
        Err(_) => Ok(BTreeMap::new()),
    }
}

/// Fetch one entry by slug, erroring if it's unknown.
pub fn get(slug: &str) -> Result<Entry> {
    load()?
        .remove(slug)
        .with_context(|| format!("no spawned agent '{slug}' (see `orchbus list` / .orchbus/state.json)"))
}

/// Insert or replace an entry, then persist the whole store atomically.
pub fn put(entry: Entry) -> Result<()> {
    let mut all = load()?;
    all.insert(entry.slug.clone(), entry);
    write_all(&all)
}

/// Remove an entry by slug (no-op if absent). Returns whether it existed.
pub fn remove(slug: &str) -> Result<bool> {
    let mut all = load()?;
    let existed = all.remove(slug).is_some();
    write_all(&all)?;
    Ok(existed)
}

/// Serialize + write `.orchbus/state.json` atomically (temp file + rename), the
/// same discipline `scan.rs` uses for its cache.
fn write_all(all: &BTreeMap<String, Entry>) -> Result<()> {
    let dir = git::orchbus_dir()?;
    std::fs::create_dir_all(&dir).with_context(|| format!("creating {}", dir.display()))?;
    let body = serde_json::to_string_pretty(all).context("serializing state")?;
    let final_path = dir.join("state.json");
    let tmp = dir.join(format!("state.json.{}", std::process::id()));
    std::fs::write(&tmp, body).with_context(|| format!("writing {}", tmp.display()))?;
    std::fs::rename(&tmp, &final_path).with_context(|| format!("renaming into {}", final_path.display()))?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn entry_json_round_trips() {
        let e = Entry {
            slug: "fix-flaky".into(),
            worktree: "/repo/.orchbus/worktrees/fix-flaky".into(),
            branch: "orchbus/fix-flaky".into(),
            base: "abc123".into(),
            tag: "plan".into(),
            agent: "claude".into(),
            session_id: "uuid-1".into(),
            review_session_id: None,
        };
        let map: BTreeMap<String, Entry> = [(e.slug.clone(), e.clone())].into();
        let json = serde_json::to_string(&map).unwrap();
        let back: BTreeMap<String, Entry> = serde_json::from_str(&json).unwrap();
        assert_eq!(back["fix-flaky"], e);
    }

    #[test]
    fn review_session_id_defaults_when_missing() {
        // Older records without the field still parse.
        let e: Entry = serde_json::from_str(
            r#"{"slug":"s","worktree":"w","branch":"b","base":"c","tag":"plan","agent":"claude","session_id":"u"}"#,
        )
        .unwrap();
        assert_eq!(e.review_session_id, None);
    }
}
