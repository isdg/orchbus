//! The fzf cockpit (`prefix o` popup / `prefix O` window) and the window opener.
//!
//! fzf drives everything through `--bind`s that call back into this same binary
//! (resolved via current_exe): approve/cancel/refresh act on the highlighted
//! pane ({1} = pane_id, hidden from matching via --with-nth=2..), and a
//! load->sleep->reload self-loop refreshes ~every 1s so glyphs stay current.

use crate::scan;
use crate::tmux;
use anyhow::{Context, Result};

fn exe() -> Result<String> {
    Ok(std::env::current_exe()
        .context("cannot resolve own path")?
        .to_string_lossy()
        .into_owned())
}

/// Run the cockpit. `fresh` (the `prefix O` window) scans on init so its opening
/// view is guaranteed current; otherwise (the popup) paint instantly from cache.
pub fn run(fresh: bool) -> Result<()> {
    let exe = exe()?;
    let init = scan::dispatch(!fresh, None)?; // fresh -> full scan; else -> cache

    let scan_all = format!("{exe} scan");
    let scan_one = format!("{exe} scan {{1}}"); // {{1}} -> literal {1} for fzf
    let approve = format!("{exe} approve {{1}} enter");
    let cancel = format!("{exe} cancel {{1}}");

    let args: Vec<String> = vec![
        "--reverse".into(),
        "--delimiter=\t".into(),
        "--with-nth=2..".into(),
        "--prompt=orchbus> ".into(),
        "--header=ctrl-a approve · ctrl-x cancel · ctrl-r refresh · enter jump".into(),
        "--preview=tmux capture-pane -ep -t {1} | tail -n \"${FZF_PREVIEW_LINES:-40}\"".into(),
        "--preview-window=down,70%".into(),
        "--preview-label= pane ".into(),
        // Self-refresh loop: `load` fires once the list is read, then each
        // finished reload re-fires it — a fresh scan swaps in ~1s after open and
        // every ~1s after. Async `reload` (not reload-sync) so input never blocks.
        format!("--bind=load:reload(sleep 1; {scan_all})"),
        format!("--bind=ctrl-r:reload({scan_all})"),
        format!("--bind=ctrl-a:execute-silent({approve})+reload({scan_one})"),
        format!("--bind=ctrl-x:execute-silent({cancel})+reload({scan_one})"),
        "--bind=enter:execute-silent(tmux switch-client -t {1}; tmux select-window -t {1}; tmux select-pane -t {1})+abort".into(),
    ];

    tmux::fzf_interactive(&args, init)
}

/// Open the cockpit as a real tmux window, reusing an existing `orchbus` window
/// (in any session) instead of spawning a duplicate.
pub fn open() -> Result<()> {
    let existing = tmux::query([
        "list-windows",
        "-a",
        "-F",
        "#{window_name} #{session_name}:#{window_index}",
    ])?
    .lines()
    .find_map(|l| {
        let (name, target) = l.split_once(' ')?;
        (name == "orchbus").then(|| target.to_string())
    });

    match existing {
        Some(target) => tmux::run(["switch-client", "-t", &target]),
        None => tmux::run(["new-window", "-n", "orchbus", &format!("{} ui --fresh", exe()?)]),
    }
}
