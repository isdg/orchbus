//! `orchbus revise <slug>` — hand the latest review back to the implement agent so
//! it fixes the findings, continuing the SAME session.
//!
//! Primary path: the implement pane is still alive → type an instruction pointing
//! at the review file into it (the `send-keys` mechanism `approve` already uses).
//! Fallback: the pane is gone → resume the session in a fresh window, re-passing
//! `--dangerously-skip-permissions` (⚠️ not restored on resume).

use crate::{agent, git, scan, state, tmux};
use anyhow::{bail, Context, Result};

pub fn run(slug: &str) -> Result<()> {
    let entry = state::get(slug)?;
    let review = git::orchbus_dir()?.join("reviews").join(format!("{slug}.md"));
    if !review.exists() {
        bail!("no review for '{slug}' yet — run `orchbus review {slug}` first");
    }
    let instruction = instruction(&review.to_string_lossy());

    match scan::pane_for_window(slug)? {
        Some(pane) => {
            // Type the instruction into the live pane and submit it.
            tmux::run(["send-keys", "-t", &pane, "-l", &instruction])?;
            tmux::run(["send-keys", "-t", &pane, "Enter"])?;
            println!("sent review back to '{slug}' ({pane}) — it will fix the findings");
        }
        None => {
            // Pane gone: resume the session in a new window in the worktree.
            let ag = agent::for_command(&entry.agent)
                .with_context(|| format!("unknown agent '{}'", entry.agent))?;
            let argv = ag
                .argv(&agent::Launch {
                    resume: Some(&entry.session_id),
                    skip_perms: true, // re-pass: bypassPermissions isn't restored on resume
                    prompt: Some(&instruction),
                    ..Default::default()
                })
                .with_context(|| format!("orchbus can't drive agent '{}' yet", entry.agent))?;
            let mut cmd: Vec<String> =
                vec!["new-window".into(), "-c".into(), entry.worktree.clone(), "-n".into(), slug.into()];
            cmd.extend(argv);
            tmux::run(cmd)?;
            println!("'{slug}' pane was gone — resumed the session in a new window");
        }
    }
    Ok(())
}

/// The one-line instruction. Points at the review file by absolute path (the agent
/// runs in the worktree, so a repo-root-relative `.orchbus/...` path wouldn't
/// resolve) rather than pasting multi-line findings into the prompt.
fn instruction(review_path: &str) -> String {
    format!(
        "A code review flagged issues with your work. Read the review at {review_path} \
         and fix every finding (both spec and correctness), then stop."
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn instruction_points_at_review_and_is_single_line() {
        let i = instruction("/repo/.orchbus/reviews/x.md");
        assert!(i.contains("/repo/.orchbus/reviews/x.md"));
        assert!(i.contains("spec and correctness"));
        assert!(!i.contains('\n'), "must be one line so send-keys doesn't submit early");
    }
}
