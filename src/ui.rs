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

/// The top line: the active sort mode, then the keys. It rides on the list's top
/// border as a label rather than in `--header`, because fzf anchors the header to
/// the prompt — in this bottom-up layout that lands it next to the input, and the
/// help belongs at the top. The mode leads because it is the only part that
/// changes; `ctrl-g` re-runs this through `transform-list-label` so the label
/// always names what you are looking at rather than a fixed action.
/// The outer spaces keep the text off the border glyphs.
pub fn header() -> String {
    format!(
        " {} · ctrl-a approve · ctrl-i interrupt · ctrl-x kill pane · ctrl-g sort · enter jump ",
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
    //
    // No `--style` preset: fzf's built-in default is what nvim's fzf pickers run
    // with, and it is what colors the `▌` gutter and the pointer. The gutter color
    // falls back to `bg+`, so the rule down the left of the list follows whatever
    // the shared fzf theme sets for the current mode; `minimal` resets that
    // fallback to the terminal's default foreground and paints the same bar in
    // flat white. The options below strip the preset's scrollbar and marker one at
    // a time instead, which doesn't touch the colors.
    let args: Vec<String> = vec![
        // Bottom-up, like nvim's Buffers: the first match sits against the prompt
        // and the list grows upward, so a short list stays under your cursor
        // instead of stranding it at the top of an empty pane.
        "--layout=default".into(),
        "--delimiter=\t".into(),
        "--with-nth=2..".into(),
        // The counter gets its own line above the prompt, left-aligned and trailed
        // by a rule: a fixed position that no query length can move, and the rule
        // doubles as the divider the input needs — which is why --input-border is
        // gone, it would have drawn a second one.
        "--info=default".into(),
        "--separator=─".into(),
        "--no-scrollbar".into(),
        "--marker= ".into(),
        // The gutter keeps fzf's default '▌' on every non-current row, so a rule
        // runs down the left of the list — the same edge nvim's fzf pickers draw
        // beside their options. The pointer is left at fzf's default too, so the
        // current row's mark is themed by the same colors rather than a bare `›`
        // in the foreground color.
        "--prompt=› ".into(),
        "--list-border=top".into(),
        "--list-label-pos=2".into(),
        format!("--list-label={}", header()),
        "--preview=tmux capture-pane -ep -t {1} | tail -n \"${FZF_PREVIEW_LINES:-40}\"".into(),
        "--preview-window=down,60%,border-top".into(),
        // Self-refresh loop: `load` fires once the list is read, then each
        // finished reload re-fires it — a fresh scan swaps in ~1s after open and
        // every ~1s after. Async `reload` (not reload-sync) so input never blocks.
        format!("--bind=load:reload(sleep 1; {scan_all})"),
        format!("--bind=ctrl-r:reload({scan_all})"),
        format!("--bind=ctrl-g:execute-silent({toggle})+reload({scan_all})+transform-list-label({exe} header)"),
        format!("--bind=ctrl-a:execute-silent({approve})+reload({scan_one})"),
        format!("--bind=ctrl-i:execute-silent({interrupt})+reload({scan_one})"),
        // Kills the pane outright — no confirm, and the agent in it dies with the
        // process. Reloads the whole list rather than the single pane, since the
        // pane that a splice would rescan no longer exists.
        format!("--bind=ctrl-x:execute-silent(tmux kill-pane -t {{1}})+reload(sleep 0.2; {scan_all})"),
        "--bind=enter:execute-silent(tmux switch-client -t {1}; tmux select-window -t {1}; tmux select-pane -t {1})+abort".into(),
        // The shared opts file binds M-j to jump mode and chains `jump` to accept.
        // accept means nothing here — enter runs its own execute-silent and stdout
        // is ignored — so it would close the cockpit doing nothing. Land, then act.
        "--bind=jump:ignore".into(),
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
