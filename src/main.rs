//! orchbus — a tmux cockpit for triaging/approving Claude Code sessions across
//! all panes without tabbing between windows.
//!
//!   orchbus ui [--fresh]   fzf cockpit (prefix o popup / prefix O window body)
//!   orchbus open           open/reuse the cockpit as a real window (prefix O)
//!   orchbus scan [--cache] [PANE]
//!                          emit the pane list (full / from-cache / splice one)
//!   orchbus approve PANE <enter|1-9>   approve the menu, guarded (no-op if gone)
//!   orchbus cancel PANE    send Escape to a pane
//!
//! The scan classifier and the approve guard share one PATTERN TABLE (classify).

mod agent;
mod classify;
mod format;
mod scan;
mod spawn;
mod target;
mod tmux;
mod ui;
mod wait;
mod fork;
mod git;
mod passthru;
mod plan;
mod review;
mod revise;
// state::remove is consumed by reap (deferred).
#[allow(dead_code)]
mod state;
mod tags;

use anyhow::{bail, Context, Result};
use clap::{Parser, Subcommand};

#[derive(Parser)]
#[command(name = "orchbus", about = "tmux cockpit for Claude Code sessions")]
struct Cli {
    #[command(subcommand)]
    cmd: Cmd,
}

#[derive(Subcommand)]
enum Cmd {
    /// Emit one row per Claude Code pane (importance-sorted).
    Scan {
        /// Print the last cached list instantly, without scanning.
        #[arg(long)]
        cache: bool,
        /// Emit a JSON array instead of the fzf TSV list.
        #[arg(long)]
        json: bool,
        /// Rescan only this pane and splice it into the cached list.
        pane: Option<String>,
    },
    /// Show or flip the cockpit's row order: attention rank, or grouped by
    /// directory with the current one first. The choice persists, so the
    /// cockpit's ~1s auto-refresh keeps whichever view you picked.
    Sort {
        /// Flip to the other order and print the new one.
        #[arg(long)]
        toggle: bool,
    },
    /// List Claude Code panes as a human-readable table.
    #[command(alias = "ls")]
    List {
        /// Emit a JSON array instead of the table.
        #[arg(long)]
        json: bool,
    },
    /// Summarize pane states in one line; exits non-zero if any need approval.
    /// With a SLUG, report just that spawned agent's state (gone => non-zero).
    Status {
        /// A spawn slug: report only that agent's live state instead of the tally.
        slug: Option<String>,
        /// Emit JSON instead of the one-liner.
        #[arg(long)]
        json: bool,
    },
    /// Approve the highlighted menu on PANE, only if it's still showing.
    Approve {
        /// %pane_id, session:window, or window name (omit with --all).
        pane: Option<String>,
        /// `enter` (accept default) or a digit 1-9 (ignored with --all).
        key: Option<String>,
        /// Approve every pane showing an approval menu (accepts each default).
        #[arg(long)]
        all: bool,
        /// Skip the confirmation prompt (required for --all without a TTY).
        #[arg(short = 'y', long)]
        yes: bool,
    },
    /// Cancel a pane's prompt (send Escape).
    Cancel {
        /// %pane_id, session:window, or window name (omit with --all).
        pane: Option<String>,
        /// Cancel every pane showing an approval menu.
        #[arg(long)]
        all: bool,
        /// Skip the confirmation prompt (required for --all without a TTY).
        #[arg(short = 'y', long)]
        yes: bool,
    },
    /// Run the fzf cockpit.
    Ui {
        /// Full scan on init (the persistent window) vs cached (the popup).
        #[arg(long)]
        fresh: bool,
    },
    /// Open the cockpit as a reusable tmux window.
    Open,
    /// Spawn a tagged agent in an isolated git worktree.
    Spawn {
        /// The task prompt for the agent.
        prompt: String,
        /// Role profile: plan (default), implement, review, or a custom tag.
        #[arg(long, default_value = "plan")]
        tag: String,
        /// Branch name (default: orchbus/<slug>).
        #[arg(long)]
        branch: Option<String>,
        /// Don't pass --dangerously-skip-permissions even though it's isolated.
        #[arg(long)]
        no_skip: bool,
    },
    /// Run `claude` natively with your own flags, in an isolated worktree.
    ///
    /// Everything after `claude` is forwarded verbatim, so the whole Claude Code CLI
    /// is available (`--model`, `--agent`, `--effort`, `-p`, …). orchbus adds only a
    /// pinned `--session-id` and `--dangerously-skip-permissions`, and drops either
    /// one if your args already cover it. Put orchbus's own flags first, then `--`:
    ///
    ///   orchbus claude --slug retry -- --model opus "add retry to the http client"
    ///
    /// Aliased to `c`, the verb you'll actually type: `orchbus c -p "…"`.
    #[command(alias = "c", trailing_var_arg = true)]
    Claude {
        /// Name for the spawn (default: from the trailing prompt, else `claude`).
        #[arg(long)]
        slug: Option<String>,
        /// Branch name (default: orchbus/<slug>).
        #[arg(long)]
        branch: Option<String>,
        /// Don't inject --dangerously-skip-permissions even though it's isolated.
        #[arg(long)]
        no_skip: bool,
        /// Flags and prompt handed straight to `claude`.
        #[arg(allow_hyphen_values = true)]
        args: Vec<String>,
    },
    /// Block until a spawned agent settles (interactive done / needs attention),
    /// so a driving session can sequence spawn → wait → review.
    Wait {
        /// The spawn slug.
        slug: String,
        /// Wait for one specific state (idle|approve|input|rating|running);
        /// default: return on any settled state.
        #[arg(long = "for")]
        target: Option<String>,
        /// Give up after this many seconds (non-zero exit).
        #[arg(long, default_value_t = 600)]
        timeout: u64,
    },
    /// Capture a spawned agent's plan to .orchbus/plans/<slug>.md.
    Capture {
        /// The spawn slug (see `orchbus list` / state.json).
        slug: String,
    },
    /// Review a spawned agent's diff (or plan) with a fresh headless agent.
    Review {
        /// The spawn slug.
        slug: String,
        /// Review the plan itself (pre-implement gate) instead of the diff.
        #[arg(long)]
        plan: bool,
    },
    /// Send the latest review back to a spawned agent to fix the findings.
    Revise {
        /// The spawn slug.
        slug: String,
    },
    /// Fork a spawned agent's session into a new divergent worktree.
    Fork {
        /// The parent spawn slug.
        slug: String,
        /// Role for the fork (default: inherit the parent's tag).
        #[arg(long)]
        tag: Option<String>,
        /// First instruction for the interactive fork pane.
        #[arg(long)]
        prompt: Option<String>,
    },
}

fn main() -> Result<()> {
    match Cli::parse().cmd {
        Cmd::Scan { cache, json, pane } => {
            tmux::require_server()?;
            let out = if json {
                format::json_rows(&scan::collect(cache, pane)?)
            } else {
                scan::dispatch(cache, pane)?
            };
            if !out.is_empty() {
                println!("{out}");
            }
        }
        Cmd::Sort { toggle } => {
            let mode = if toggle { scan::toggle_sort() } else { scan::sort_mode() };
            println!("{}", scan::sort_label(mode));
        }
        Cmd::List { json } => {
            tmux::require_server()?;
            let rows = scan::collect(false, None)?;
            let out = if json { format::json_rows(&rows) } else { format::human(&rows) };
            if !out.is_empty() {
                println!("{out}");
            }
        }
        Cmd::Status { slug, json } => {
            tmux::require_server()?;
            use std::io::Write;
            match slug {
                // Per-slug: report one spawned agent's live state so a driving
                // session can sequence spawn -> (wait) -> review. `gone` exits
                // non-zero so scripts can detect a dead/closed pane.
                Some(slug) => {
                    state::get(&slug)?; // validate it's a known spawn
                    let found = scan::window_state(&slug)?;
                    let (state, pane) = match &found {
                        Some((pid, st)) => (classify::label(*st), Some(pid.as_str())),
                        None => ("gone", None),
                    };
                    let out = if json {
                        format::slug_status_json(&slug, state, pane)
                    } else {
                        state.to_string()
                    };
                    println!("{out}");
                    let _ = std::io::stdout().flush();
                    if found.is_none() {
                        std::process::exit(1);
                    }
                }
                // Fleet tally; non-zero when any pane is blocked on approval, so
                // `status` slots into shell conditionals. Flush before exit.
                None => {
                    let rows = scan::collect(false, None)?;
                    let out =
                        if json { format::status_json(&rows) } else { format::status(&rows) };
                    println!("{out}");
                    let _ = std::io::stdout().flush();
                    if format::any_waiting(&rows) {
                        std::process::exit(1);
                    }
                }
            }
        }
        Cmd::Approve { pane, key, all, yes } => {
            tmux::require_server()?;
            if all {
                let targets = scan::approvable(&scan::collect(false, None)?);
                if confirm("approve", targets.len(), yes)? {
                    for p in &targets {
                        approve(p, "enter")?;
                    }
                }
            } else {
                let pane = pane.context("approve needs a PANE (or --all)")?;
                let key = key.context("approve needs a key: enter or 1-9 (or --all)")?;
                approve(&target::resolve(&pane)?, &key)?;
            }
        }
        Cmd::Cancel { pane, all, yes } => {
            tmux::require_server()?;
            if all {
                let targets = scan::approvable(&scan::collect(false, None)?);
                if confirm("cancel", targets.len(), yes)? {
                    for p in &targets {
                        tmux::run(["send-keys", "-t", p, "Escape"])?;
                    }
                }
            } else {
                let pane = target::resolve(&pane.context("cancel needs a PANE (or --all)")?)?;
                tmux::run(["send-keys", "-t", &pane, "Escape"])?;
            }
        }
        Cmd::Ui { fresh } => {
            tmux::require_inside()?;
            ui::run(fresh)?
        }
        Cmd::Open => {
            tmux::require_inside()?;
            ui::open()?
        }
        Cmd::Spawn { prompt, tag, branch, no_skip } => {
            tmux::require_inside()?;
            let slug = spawn::run(&prompt, &tag, branch.as_deref(), no_skip)?;
            println!("spawned '{slug}' (tag {tag}) — jump with: orchbus approve {slug} … / list");
        }
        Cmd::Claude { slug, branch, no_skip, args } => {
            tmux::require_inside()?;
            let slug = passthru::run(&args, slug.as_deref(), branch.as_deref(), no_skip)?;
            println!("spawned '{slug}' (claude passthrough) — jump with: orchbus list / wait {slug}");
        }
        Cmd::Wait { slug, target, timeout } => {
            tmux::require_server()?;
            let target = match target {
                Some(ref s) => Some(classify::from_label(s).with_context(|| {
                    format!("unknown state '{s}' (use idle|approve|input|rating|running)")
                })?),
                None => None,
            };
            wait::run(&slug, target, timeout)?;
        }
        Cmd::Capture { slug } => {
            let path = plan::capture(&slug)?;
            println!("captured plan → {}", path.display());
        }
        Cmd::Review { slug, plan } => review::run(&slug, plan)?,
        Cmd::Revise { slug } => {
            tmux::require_inside()?;
            revise::run(&slug)?
        }
        Cmd::Fork { slug, tag, prompt } => {
            tmux::require_inside()?;
            fork::run(&slug, tag.as_deref(), prompt.as_deref())?
        }
    }
    Ok(())
}

/// Gate a bulk `--all` action behind a confirmation: no-op message when there's
/// nothing to do, straight through with `--yes`, an interactive `[y/N]` prompt on
/// a TTY, and a hard error when neither (so scripts must opt in with `--yes`).
fn confirm(action: &str, n: usize, yes: bool) -> Result<bool> {
    use std::io::{IsTerminal, Write};
    if n == 0 {
        println!("nothing to {action}");
        return Ok(false);
    }
    if yes {
        return Ok(true);
    }
    if !std::io::stdin().is_terminal() {
        bail!("refusing to {action} {n} pane(s) without --yes (no TTY to confirm)");
    }
    print!("{action} {n} pane(s)? [y/N] ");
    let _ = std::io::stdout().flush();
    let mut line = String::new();
    std::io::stdin().read_line(&mut line)?;
    Ok(matches!(line.trim(), "y" | "Y" | "yes"))
}

/// Re-capture PANE and send the key ONLY if the approve menu (❯ N.) is still
/// showing — closing the race between orchbus's periodic scan and the keypress,
/// so a stray key never lands on an idle/running/interrupted pane.
fn approve(pane: &str, key: &str) -> Result<()> {
    let text = tmux::query(["capture-pane", "-p", "-t", pane]).unwrap_or_default();
    if !classify::shows_approve_menu(&text) {
        return Ok(()); // menu gone -> no-op
    }
    match key {
        "enter" => tmux::run(["send-keys", "-t", pane, "Enter"])?,
        // Digit pick: send the option number, then confirm with Enter as separate
        // send-keys calls (tmux flushes between them) to avoid the debounce race.
        d if d.len() == 1 && matches!(d.chars().next(), Some('1'..='9')) => {
            tmux::run(["send-keys", "-t", pane, "-l", d])?;
            tmux::run(["send-keys", "-t", pane, "Enter"])?;
        }
        _ => {}
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn claude_cmd(argv: &[&str]) -> (Option<String>, bool, Vec<String>) {
        match Cli::try_parse_from(argv).expect("should parse").cmd {
            Cmd::Claude { slug, no_skip, args, .. } => (slug, no_skip, args),
            other => panic!("expected Cmd::Claude, got {:?}", std::mem::discriminant(&other)),
        }
    }

    /// `--` separates orchbus's flags from claude's — the documented spelling.
    #[test]
    fn passthrough_after_double_dash_keeps_claude_flags_intact() {
        let (slug, no_skip, args) =
            claude_cmd(&["orchbus", "claude", "--slug", "retry", "--", "--model", "opus", "-p", "go"]);
        assert_eq!(slug.as_deref(), Some("retry"));
        assert!(!no_skip);
        assert_eq!(args, ["--model", "opus", "-p", "go"]);
    }

    /// …and it also works without `--`, since claude's flags aren't orchbus's.
    #[test]
    fn passthrough_without_double_dash_also_collects_flags() {
        let (slug, _, args) = claude_cmd(&["orchbus", "claude", "--model", "opus", "-p", "go"]);
        assert_eq!(slug, None);
        assert_eq!(args, ["--model", "opus", "-p", "go"]);
    }

    /// A bare `orchbus claude` is valid: a tracked, isolated interactive session.
    #[test]
    fn passthrough_accepts_no_args() {
        let (slug, no_skip, args) = claude_cmd(&["orchbus", "claude"]);
        assert_eq!(slug, None);
        assert!(!no_skip);
        assert!(args.is_empty());
    }

    /// `c` is the short spelling — same verb, flags still forwarded untouched.
    /// (clap matches aliases exactly, so it can't be confused with cancel/capture.)
    #[test]
    fn c_is_an_alias_for_claude() {
        let (slug, _, args) = claude_cmd(&["orchbus", "c", "-p", "go"]);
        assert_eq!(slug, None);
        assert_eq!(args, ["-p", "go"]);
        assert!(Cli::try_parse_from(["orchbus", "c"]).is_ok());
    }

    /// orchbus's own flags bind to orchbus, not to the passthrough.
    #[test]
    fn orchbus_flags_are_not_forwarded() {
        let (_, no_skip, args) = claude_cmd(&["orchbus", "claude", "--no-skip", "--", "-p", "go"]);
        assert!(no_skip);
        assert_eq!(args, ["-p", "go"]);
    }
}
