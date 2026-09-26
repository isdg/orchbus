//! Capture the plan a plan-mode agent produced, so the diff reviewer can check
//! the implementation against it (the "Spec" axis).
//!
//! Claude Code stores each session as JSONL at
//! `~/.claude/projects/<enc-cwd>/<session-id>.jsonl`, where the plan is the
//! `input.plan` of an `ExitPlanMode` tool_use in an assistant message. We pull the
//! latest one out and write it to `.orchbus/plans/<slug>.md`.

use crate::{git, state};
use anyhow::{Context, Result};
use std::path::PathBuf;

/// `~/.claude/projects` (honoring `CLAUDE_CONFIG_DIR`).
fn projects_dir() -> Result<PathBuf> {
    let cfg = std::env::var("CLAUDE_CONFIG_DIR").ok().filter(|s| !s.is_empty());
    let base = match cfg {
        Some(dir) => PathBuf::from(dir),
        None => {
            let home = std::env::var("HOME").context("HOME not set")?;
            PathBuf::from(home).join(".claude")
        }
    };
    Ok(base.join("projects"))
}

/// Claude's project-dir encoding: every non-alphanumeric char becomes `-`
/// (one-to-one, so `/Users/x/.orchbus` → `-Users-x--orchbus`).
fn encode_cwd(cwd: &str) -> String {
    cwd.chars().map(|c| if c.is_ascii_alphanumeric() { c } else { '-' }).collect()
}

fn transcript_path(worktree: &str, session_id: &str) -> Result<PathBuf> {
    Ok(projects_dir()?.join(encode_cwd(worktree)).join(format!("{session_id}.jsonl")))
}

/// The latest plan (`ExitPlanMode` tool_use `input.plan`) in a session transcript,
/// or `None` if the agent hasn't produced one yet. Pure over the JSONL text.
pub fn extract_plan(jsonl: &str) -> Option<String> {
    let mut latest = None;
    for line in jsonl.lines() {
        let Ok(v) = serde_json::from_str::<serde_json::Value>(line) else {
            continue;
        };
        let Some(content) = v.get("message").and_then(|m| m.get("content")).and_then(|c| c.as_array())
        else {
            continue;
        };
        for item in content {
            let is_exit = item.get("type").and_then(|t| t.as_str()) == Some("tool_use")
                && item.get("name").and_then(|n| n.as_str()) == Some("ExitPlanMode");
            if is_exit {
                if let Some(plan) = item.get("input").and_then(|i| i.get("plan")).and_then(|p| p.as_str()) {
                    latest = Some(plan.to_string());
                }
            }
        }
    }
    latest
}

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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn encode_cwd_maps_non_alnum_to_dash() {
        assert_eq!(encode_cwd("/Users/x/.orchbus/wt"), "-Users-x--orchbus-wt");
    }

    #[test]
    fn extract_plan_takes_latest_exit_plan_mode() {
        let jsonl = concat!(
            r#"{"type":"assistant","message":{"role":"assistant","content":[{"type":"text","text":"thinking"}]}}"#,
            "\n",
            r#"{"type":"assistant","message":{"role":"assistant","content":[{"type":"tool_use","name":"ExitPlanMode","input":{"plan":"first"}}]}}"#,
            "\n",
            r#"{"type":"user","message":{"role":"user","content":[{"type":"text","text":"revise"}]}}"#,
            "\n",
            r#"{"type":"assistant","message":{"role":"assistant","content":[{"type":"tool_use","name":"ExitPlanMode","input":{"plan":"second and final"}}]}}"#,
        );
        assert_eq!(extract_plan(jsonl).as_deref(), Some("second and final"));
    }

    #[test]
    fn extract_plan_none_when_no_plan() {
        let jsonl = r#"{"type":"assistant","message":{"role":"assistant","content":[{"type":"text","text":"no plan here"}]}}"#;
        assert_eq!(extract_plan(jsonl), None);
        assert_eq!(extract_plan("not json\n{}"), None);
    }
}
