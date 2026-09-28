//! Claude Code session transcripts on disk.
//!
//! Each session is JSONL at `~/.claude/projects/<enc-cwd>/<session-id>.jsonl`. The format is
//! internal to Claude Code and changes between releases, so every reader here is tolerant.

use anyhow::{Context, Result};
use std::path::PathBuf;

/// `~/.claude/projects` (honoring `CLAUDE_CONFIG_DIR`).
pub fn projects_dir() -> Result<PathBuf> {
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
pub fn encode_cwd(cwd: &str) -> String {
    cwd.chars().map(|c| if c.is_ascii_alphanumeric() { c } else { '-' }).collect()
}

pub fn transcript_path(worktree: &str, session_id: &str) -> Result<PathBuf> {
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
