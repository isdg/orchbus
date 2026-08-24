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

/// The one header line: the active sort mode, then the keys. The mode leads
/// because it is the only part that changes — `ctrl-g` re-runs this through
/// fzf's `transform-header`, so the label always matches what you are looking at
/// instead of naming a fixed action.
pub fn header() -> String {
    format!(
        "{} · ctrl-a approve · ctrl-i interrupt · ctrl-x kill pane · ctrl-g sort · enter jump",
        scan::sort_label(scan::sort_mode())
    )
}

/// Run the cockpit. `fresh` (the `prefix O` window) scans on init so its opening
/// view is guaranteed current; otherwise (the popup) paint instantly from cache.
pub fn run(fresh: bool) -> Result<()> {
    let exe = exe()?;
    let init = scan::dispatch(!fresh, None)?; // fresh -> full scan; else -> cache

    let scan_all = format!("{exe} scan");
    let scan_one = format!("{exe} scan {{1}}"); // {{1}} -> literal {1} for fzf
    let approve = format!("{exe} approve {{1}} enter");
    // Escape is what interrupts a Claude Code turn, which is exactly what `cancel`
    // already sends — so interrupt is that verb under a name that says what it
    // does to the agent rather than to the prompt.
    let interrupt = format!("{exe} cancel {{1}}");
    // ctrl-g flips the persisted sort and reloads. The mode lives in a file, not
    // in fzf, because the 1s auto-reload runs `scan` as a new process — holding
    // it here would mean every refresh snapped back to the rank view.
    let toggle = format!("{exe} sort --toggle");

    // Layout: list on top, the input line under it, preview below that — the
    // shape of nvim's buffer picker. `reverse-list` is what puts the prompt at
    // the bottom of the list while keeping the rows top-down, which matters here
    // because the rows are meaningfully ordered (rank, or directory groups);
    // plain `default` would read them bottom-up. Everything else is stripped:
    // inline counter instead of its own line, no scrollbar, a bare prompt, and a
    // single rule between the input and the preview.
    let args: Vec<String> = vec![
        "--style=minimal".into(),
        "--layout=reverse-list".into(),
        "--delimiter=\t".into(),
        "--with-nth=2..".into(),
        // inline-right, not inline: the counter is pinned to the right edge, so it
        // holds one column instead of sliding rightward as the query grows.
        "--info=inline-right".into(),
        "--no-scrollbar".into(),
        "--pointer=›".into(),
        "--marker= ".into(),
        // fzf 0.70 paints a '▌' gutter down every non-current row; blank it so the
        // only mark in the list is the pointer on the row you are actually on.
        "--gutter= ".into(),
        "--prompt=› ".into(),
        // A rule between the list and the input, so the thing you type into is
        // visually its own row rather than the last line of the results.
        "--input-border=line".into(),
        format!("--header={}", header()),
        "--preview=tmux capture-pane -ep -t {1} | tail -n \"${FZF_PREVIEW_LINES:-40}\"".into(),
        "--preview-window=down,60%,border-top".into(),
        // Self-refresh loop: `load` fires once the list is read, then each
        // finished reload re-fires it — a fresh scan swaps in ~1s after open and
        // every ~1s after. Async `reload` (not reload-sync) so input never blocks.
        format!("--bind=load:reload(sleep 1; {scan_all})"),
        format!("--bind=ctrl-r:reload({scan_all})"),
        format!("--bind=ctrl-g:execute-silent({toggle})+reload({scan_all})+transform-header({exe} header)"),
        format!("--bind=ctrl-a:execute-silent({approve})+reload({scan_one})"),
        format!("--bind=ctrl-i:execute-silent({interrupt})+reload({scan_one})"),
        // Kills the pane outright — no confirm, and the agent in it dies with the
        // process. Reloads the whole list rather than the single pane, since the
        // pane that a splice would rescan no longer exists.
        format!("--bind=ctrl-x:execute-silent(tmux kill-pane -t {{1}})+reload(sleep 0.2; {scan_all})"),
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
