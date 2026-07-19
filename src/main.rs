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
mod git;
mod plan;
// Some store/registry helpers are consumed by later verbs (review/revise/fork).
#[allow(dead_code)]
mod state;
#[allow(dead_code)]
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
    /// List Claude Code panes as a human-readable table.
    #[command(alias = "ls")]
    List {
        /// Emit a JSON array instead of the table.
        #[arg(long)]
        json: bool,
    },
    /// Summarize pane states in one line; exits non-zero if any need approval.
    Status {
        /// Emit a JSON object instead of the one-liner.
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
    /// Capture a spawned agent's plan to .orchbus/plans/<slug>.md.
    Capture {
        /// The spawn slug (see `orchbus list` / state.json).
        slug: String,
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
        Cmd::List { json } => {
            tmux::require_server()?;
            let rows = scan::collect(false, None)?;
            let out = if json { format::json_rows(&rows) } else { format::human(&rows) };
            if !out.is_empty() {
                println!("{out}");
            }
        }
        Cmd::Status { json } => {
            tmux::require_server()?;
            let rows = scan::collect(false, None)?;
            let out = if json { format::status_json(&rows) } else { format::status(&rows) };
            println!("{out}");
            // Non-zero exit when a pane is blocked on approval, so `status` slots
            // into shell conditionals. Flush first — process::exit skips buffers.
            use std::io::Write;
            let _ = std::io::stdout().flush();
            if format::any_waiting(&rows) {
                std::process::exit(1);
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
        Cmd::Capture { slug } => {
            let path = plan::capture(&slug)?;
            println!("captured plan → {}", path.display());
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
