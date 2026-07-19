//! `orchbus spawn` — launch a tagged agent in an isolated git worktree.
//!
//! This is where orchbus stops being observe-only. It creates a worktree on a
//! fresh branch, pins a session id, launches the agent (per its tag/role) in a new
//! tmux window, and records everything under a short slug so `review`/`revise`/
//! `fork` can find it later. Because the agent runs in an isolated worktree, we can
//! safely pass `--dangerously-skip-permissions` (sandcastle's isolation gate).

use crate::{agent, git, state, tags, tmux};
use anyhow::{Context, Result};
use std::collections::BTreeSet;

/// Spawn an agent for `prompt` under role `tag_name`. Returns the assigned slug.
pub fn run(prompt: &str, tag_name: &str, branch: Option<&str>, no_skip: bool) -> Result<String> {
    let root = git::root()?; // also asserts we're in a repo
    let tag = tags::resolve(tag_name)?;

    let existing: BTreeSet<String> = state::load()?.into_keys().collect();
    let slug = unique_slug(&slugify(prompt), &existing);
    let branch = branch.map(str::to_string).unwrap_or_else(|| format!("orchbus/{slug}"));
    let worktree = root.join(".orchbus/worktrees").join(&slug);
    let base = git::head()?;

    // Filesystem isolation via a worktree on a new branch.
    git::add_worktree(&worktree, &branch, &base)?;

    // Pin a session id up front so fork/resume is deterministic (spawned-only).
    let session_id = uuid::Uuid::new_v4().to_string();

    let ag = agent::for_command(&tag.agent)
        .with_context(|| format!("unknown agent '{}' for tag '{tag_name}'", tag.agent))?;
    let argv = ag
        .argv(&agent::Launch {
            session_id: Some(&session_id),
            permission_mode: opt(&tag.permission_mode),
            // Skip only because it's isolated; --no-skip opts out. (claude_argv
            // suppresses it anyway when a permission_mode like `plan` is set.)
            skip_perms: tag.skip_perms && !no_skip,
            role: opt(&tag.role),
            headless: tag.headless,
            prompt: Some(prompt),
            ..Default::default()
        })
        .with_context(|| format!("orchbus can't drive agent '{}' yet", tag.agent))?;

    // Open a tmux window in the worktree running the agent. tmux takes the trailing
    // arguments as the command's argv directly (no shell), so the prompt's spaces
    // need no quoting.
    let worktree_str = worktree.to_string_lossy().into_owned();
    let mut cmd: Vec<String> =
        vec!["new-window".into(), "-c".into(), worktree_str.clone(), "-n".into(), slug.clone()];
    cmd.extend(argv);
    tmux::run(cmd)?;

    state::put(state::Entry {
        slug: slug.clone(),
        worktree: worktree_str,
        branch,
        base,
        tag: tag_name.to_string(),
        agent: tag.agent.clone(),
        session_id,
        review_session_id: None,
    })?;

    Ok(slug)
}

/// `""` → `None`, else `Some(s)` — for optional agent flags.
fn opt(s: &str) -> Option<&str> {
    (!s.is_empty()).then_some(s)
}

/// A short, filesystem/branch-safe slug from free text: lowercase, non-alnum runs
/// collapse to a single `-`, trimmed, capped, with a fallback.
fn slugify(text: &str) -> String {
    let mut out = String::new();
    let mut prev_dash = false;
    for ch in text.chars() {
        if ch.is_ascii_alphanumeric() {
            out.extend(ch.to_lowercase());
            prev_dash = false;
        } else if !prev_dash {
            out.push('-');
            prev_dash = true;
        }
        if out.len() >= 32 {
            break;
        }
    }
    let s = out.trim_matches('-').to_string();
    if s.is_empty() {
        "agent".to_string()
    } else {
        s
    }
}

/// Ensure the slug doesn't collide with an existing one by suffixing `-2`, `-3`, …
fn unique_slug(base: &str, existing: &BTreeSet<String>) -> String {
    if !existing.contains(base) {
        return base.to_string();
    }
    (2..).map(|n| format!("{base}-{n}")).find(|s| !existing.contains(s)).unwrap()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn slugify_normalizes_and_caps() {
        assert_eq!(slugify("Fix the FLAKY test!"), "fix-the-flaky-test");
        assert_eq!(slugify("  spaces  "), "spaces");
        assert_eq!(slugify("!!!"), "agent");
        assert!(slugify(&"x".repeat(100)).len() <= 32);
    }

    #[test]
    fn unique_slug_suffixes_on_collision() {
        let mut set = BTreeSet::new();
        set.insert("fix".to_string());
        set.insert("fix-2".to_string());
        assert_eq!(unique_slug("fix", &set), "fix-3");
        assert_eq!(unique_slug("new", &set), "new");
    }
}
