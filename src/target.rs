//! Resolve a user-friendly target into a concrete tmux `pane_id`.
//!
//! The cockpit always drives panes by `pane_id` (e.g. `%23`), but from the shell
//! it's nicer to say `orchbus approve stih:1` (a `session:window`) or
//! `orchbus approve deploy` (a window *name*). `resolve` accepts any of the three
//! and hands back a single `pane_id`, erroring clearly on no-match / ambiguity so
//! a stray keypress never lands on the wrong pane.

use crate::tmux;
use anyhow::{bail, Result};

/// `pane_id \t session:window \t window_name \t pane_active` for every pane.
fn list() -> Result<String> {
    tmux::query([
        "list-panes",
        "-a",
        "-F",
        "#{pane_id}\t#{session_name}:#{window_index}\t#{window_name}\t#{pane_active}",
    ])
}

/// Resolve `input` to a pane_id: a `%…` pane_id passes straight through; anything
/// else is matched against the live pane list by `session:window` or window name.
pub fn resolve(input: &str) -> Result<String> {
    if input.starts_with('%') {
        return Ok(input.to_string());
    }
    pick(input, &list()?)
}

/// Pure matcher over a `list-panes` TSV (see `list` for the field order), split
/// out so the match/ambiguity rules are unit-testable without a tmux server.
///
/// A `session:window` or `window_name` names a *window*, which may hold several
/// panes — we pick that window's active pane. If the name resolves to more than
/// one window (e.g. a window name reused across sessions), it's ambiguous.
fn pick(input: &str, panes: &str) -> Result<String> {
    struct Pane<'a> {
        id: &'a str,
        swin: &'a str,
        active: bool,
    }

    let matched: Vec<Pane> = panes
        .lines()
        .filter_map(|l| {
            let f: Vec<&str> = l.splitn(4, '\t').collect();
            if f.len() != 4 {
                return None;
            }
            let (id, swin, wname, active) = (f[0], f[1], f[2], f[3]);
            (swin == input || wname == input).then_some(Pane {
                id,
                swin,
                active: active == "1",
            })
        })
        .collect();

    if matched.is_empty() {
        bail!("no pane matches '{input}' (try a %pane_id, session:window, or window name)");
    }

    // Distinct windows the name landed on — >1 means it's ambiguous.
    let mut windows: Vec<&str> = matched.iter().map(|p| p.swin).collect();
    windows.sort_unstable();
    windows.dedup();
    if windows.len() > 1 {
        bail!("'{input}' is ambiguous — matches windows: {}", windows.join(", "));
    }

    // Single window: send to its active pane (fall back to the first listed).
    let pane = matched.iter().find(|p| p.active).unwrap_or(&matched[0]);
    Ok(pane.id.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    const PANES: &str = "\
%1\tstih:1\tdeploy\t1
%2\tstih:1\tdeploy\t0
%3\tplc:2\tscratch\t1
%4\toda:1\tdeploy\t1";

    #[test]
    fn pane_id_passes_through() {
        // resolve short-circuits on '%'; pick isn't even consulted.
        assert_eq!(resolve("%99").unwrap(), "%99");
    }

    #[test]
    fn session_window_picks_active_pane() {
        assert_eq!(pick("stih:1", PANES).unwrap(), "%1");
    }

    #[test]
    fn unique_window_name_resolves() {
        assert_eq!(pick("scratch", PANES).unwrap(), "%3");
    }

    #[test]
    fn ambiguous_window_name_errors() {
        // "deploy" exists in both stih:1 and oda:1.
        let err = pick("deploy", PANES).unwrap_err().to_string();
        assert!(err.contains("ambiguous"), "got: {err}");
    }

    #[test]
    fn no_match_errors() {
        let err = pick("nope", PANES).unwrap_err().to_string();
        assert!(err.contains("no pane matches"), "got: {err}");
    }
}
