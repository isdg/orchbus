//! `orchbus review <slug> [--plan]` — a fresh, headless, read-only agent that
//! checks the work against the plan.
//!
//! Default: diff review — feed the captured plan **and** the worktree diff to a
//! brand-new `claude -p` session (no prior context, so it can't rubber-stamp its
//! own reasoning) and ask for two axes, spec (does the diff match the plan?) and
//! correctness. `--plan` runs the same fresh agent over just the plan, the optional
//! pre-implement gate. Findings land in `.orchbus/reviews/<slug>.md`.

use crate::{agent, git, plan, state, tags};
use anyhow::{Context, Result};
use std::path::{Path, PathBuf};

/// Review a slug. `plan_only` runs the pre-implement plan gate instead of the diff.
pub fn run(slug: &str, plan_only: bool) -> Result<()> {
    let entry = state::get(slug)?;
    let plan_text = plan::read(slug)?; // captures lazily if needed
    let worktree = Path::new(&entry.worktree);

    let input = if plan_only {
        plan_gate_prompt(&plan_text)
    } else {
        let diff = git::diff(worktree, &entry.base)?;
        diff_review_prompt(&plan_text, &diff)
    };

    // Fresh (no --session-id/--resume), headless, read-only reviewer.
    let tag = tags::resolve("review")?;
    let ag = agent::for_command(&tag.agent)
        .with_context(|| format!("unknown agent '{}' for the review tag", tag.agent))?;
    let argv = ag
        .argv(&agent::Launch {
            role: (!tag.role.is_empty()).then_some(tag.role.as_str()),
            headless: true,
            ..Default::default()
        })
        .with_context(|| format!("orchbus can't drive agent '{}' yet", tag.agent))?;

    let stdout = agent::run_capture(&argv, worktree, &input)?;
    let (result, session_id) = agent::parse_result(&stdout)?;
    let findings = extract_findings(&result);

    let out = write_review(slug, &findings)?;
    if let Some(sid) = session_id {
        let mut e = entry;
        e.review_session_id = Some(sid);
        state::put(e)?;
    }

    println!("{findings}\n\n(review saved → {})", out.display());
    Ok(())
}

fn diff_review_prompt(plan: &str, diff: &str) -> String {
    format!(
        "Here is the PLAN the change was meant to follow:\n\n{plan}\n\n\
         Here is the DIFF that was produced:\n\n```diff\n{diff}\n```\n\n\
         Review it per your instructions (spec + correctness)."
    )
}

fn plan_gate_prompt(plan: &str) -> String {
    format!(
        "Only a PLAN exists so far (no diff yet). Sanity-check the plan itself: is it \
         complete, correct, and free of risky or ambiguous steps? Report problems per \
         your instructions.\n\nPLAN:\n\n{plan}"
    )
}

/// Inner text of the `<review>…</review>` block if present, else the whole result.
fn extract_findings(result: &str) -> String {
    const OPEN: &str = "<review>";
    const CLOSE: &str = "</review>";
    if let (Some(a), Some(b)) = (result.find(OPEN), result.rfind(CLOSE)) {
        if b >= a + OPEN.len() {
            let inner = result[a + OPEN.len()..b].trim();
            return if inner.is_empty() {
                "no findings — review is clean".to_string()
            } else {
                inner.to_string()
            };
        }
    }
    result.trim().to_string()
}

fn write_review(slug: &str, findings: &str) -> Result<PathBuf> {
    let path = git::orchbus_dir()?.join("reviews").join(format!("{slug}.md"));
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).with_context(|| format!("creating {}", parent.display()))?;
    }
    std::fs::write(&path, findings).with_context(|| format!("writing {}", path.display()))?;
    Ok(path)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extract_findings_unwraps_block() {
        let r = "prose\n<review>\n- [correctness] b.rs: leak\n</review>\ntrailing";
        assert_eq!(extract_findings(r), "- [correctness] b.rs: leak");
    }

    #[test]
    fn extract_findings_empty_block_is_clean() {
        assert_eq!(extract_findings("<review></review>"), "no findings — review is clean");
    }

    #[test]
    fn extract_findings_falls_back_to_whole_result() {
        assert_eq!(extract_findings("  no tags here  "), "no tags here");
    }

    #[test]
    fn diff_prompt_includes_plan_and_diff() {
        let p = diff_review_prompt("THE PLAN", "THE DIFF");
        assert!(p.contains("THE PLAN") && p.contains("THE DIFF") && p.contains("spec + correctness"));
    }
}
