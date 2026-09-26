//! `orchbus fork <slug>` — branch a divergent line of work from an existing agent
//! session, leaving the parent untouched.
//!
//! The code is branched from the parent's branch tip into a new worktree; the
//! *session* is forked with `--fork-session`, which mints a fresh session id. We
//! capture that new id via a short headless probe (so fork/resume stays
//! deterministic — decision: spawned-only, no newest-file guessing), record it,
//! then open an interactive pane resuming the fork for the user to steer.

use crate::{agent, git, spawn, state, tmux};
use anyhow::{Context, Result};
use std::collections::BTreeSet;

/// A benign first turn whose only job is to make `--fork-session` mint a new id we
/// can read back. It must not change files.
const PROBE: &str =
    "You are a fork of a previous session. Reply with the single word: forked. Do not modify any files.";

/// Fork `parent_slug`. Optional `tag` overrides the parent's role for the fork;
/// optional `prompt` is the first instruction for the interactive fork pane.
pub fn run(parent_slug: &str, tag: Option<&str>, prompt: Option<&str>) -> Result<()> {
    let parent = state::get(parent_slug)?;
    let ag = agent::for_command(&parent.agent)
        .with_context(|| format!("unknown agent '{}'", parent.agent))?;

    let root = git::root()?;
    let existing: BTreeSet<String> = state::load()?.into_keys().collect();
    let slug = spawn::unique_slug(&format!("{parent_slug}-fork"), &existing);
    let branch = format!("orchbus/{slug}");
    let worktree = root.join(".orchbus/worktrees").join(&slug);

    // Branch the code from the parent's branch tip.
    let base = git::rev_parse(&parent.branch)?;
    git::add_worktree(&worktree, &branch, &base)?;
    let worktree_str = worktree.to_string_lossy().into_owned();

    // Headless probe: resume + fork the parent session to mint a new id.
    let probe_argv = ag
        .argv(&agent::Launch {
            resume: Some(&parent.session_id),
            fork: true,
            skip_perms: true, // re-pass: not restored on resume
            headless: true,
            prompt: Some(PROBE),
            ..Default::default()
        })
        .with_context(|| format!("orchbus can't drive agent '{}' yet", parent.agent))?;
    let stdout = agent::run_capture(&probe_argv, &worktree, "")?;
    let (_, new_id) = agent::parse_result(&stdout)?;
    let new_id = new_id.context("fork did not report a new session id")?;

    state::put(state::Entry {
        slug: slug.clone(),
        worktree: worktree_str.clone(),
        branch,
        base,
        tag: tag.unwrap_or(&parent.tag).to_string(),
        agent: parent.agent.clone(),
        session_id: new_id.clone(),
        review_session_id: None,
    })?;

    // Open an interactive pane resuming the fork for the user to continue.
    let int_argv = ag
        .argv(&agent::Launch {
            resume: Some(&new_id),
            skip_perms: true,
            prompt,
            ..Default::default()
        })
        .with_context(|| format!("orchbus can't drive agent '{}' yet", parent.agent))?;
    let mut cmd: Vec<String> =
        vec!["new-window".into(), "-c".into(), worktree_str, "-n".into(), slug.clone()];
    cmd.extend(int_argv);
    tmux::run(cmd)?;

    println!("forked '{parent_slug}' → '{slug}' (session {new_id}) in a new window");
    Ok(())
}
