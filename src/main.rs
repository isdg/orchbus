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
mod scan;
mod tmux;
mod ui;

use anyhow::Result;
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
        /// Rescan only this pane and splice it into the cached list.
        pane: Option<String>,
    },
    /// Approve the highlighted menu on PANE, only if it's still showing.
    Approve {
        pane: String,
        /// `enter` (accept default) or a digit 1-9 (pick that option).
        key: String,
    },
    /// Cancel a pane's prompt (send Escape).
    Cancel { pane: String },
    /// Run the fzf cockpit.
    Ui {
        /// Full scan on init (the persistent window) vs cached (the popup).
        #[arg(long)]
        fresh: bool,
    },
    /// Open the cockpit as a reusable tmux window.
    Open,
}

fn main() -> Result<()> {
    match Cli::parse().cmd {
        Cmd::Scan { cache, pane } => {
            let out = scan::dispatch(cache, pane)?;
            if !out.is_empty() {
                println!("{out}");
            }
        }
        Cmd::Approve { pane, key } => approve(&pane, &key)?,
        Cmd::Cancel { pane } => tmux::run(["send-keys", "-t", &pane, "Escape"])?,
        Cmd::Ui { fresh } => ui::run(fresh)?,
        Cmd::Open => ui::open()?,
    }
    Ok(())
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
