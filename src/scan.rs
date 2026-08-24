//! Emit one row per Claude Code (CC) pane for the cockpit, and keep a cache so
//! a single-pane rescan (after approve/cancel) updates instantly without
//! re-scanning every pane.
//!
//! Cache row (9 fields): rank <TAB> pane_id <TAB> glyph <TAB> agent <TAB> session:win <TAB> window_name <TAB> dir <TAB> topic <TAB> question
//! List row  (2 fields): pane_id <TAB> "glyph dir session:win agent question" 
//!
//! The list row (what the fzf cockpit consumes) deliberately omits window_name so
//! its columns stay short; window_name rides in the cache + the `--json` view.
//!
//! `dir` is the pane's working directory, stored home-relative (`~/cosmos-stih`)
//! because that is both what it is displayed as and what it is grouped by — a
//! second, absolute copy would only be a chance for the two to disagree.
//!
//! pane_id (e.g. %23) is the sole tmux target the UI uses; fields 2.. are display
//! only (the UI hides field 1 from fzf matching with --with-nth=2..). `agent` is
//! the running-agent tag (e.g. CC = Claude Code) so mixed-agent fleets stay
//! legible as we scale beyond Claude Code.

use crate::agent;
use crate::classify::{classify, meta, state_from_rank, State};
use crate::tmux;
use anyhow::{Context, Result};

const TAIL_LINES: usize = 25;

fn cache_path() -> String {
    let tmp = std::env::var("TMPDIR")
        .ok()
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| "/tmp".into());
    format!("{}/orchbus.cache", tmp.trim_end_matches('/'))
}

pub(crate) struct Row {
    pub(crate) rank: u8,
    pub(crate) pid: String,
    pub(crate) agent: String,
    pub(crate) glyph: String,
    pub(crate) swin: String,
    /// tmux window name — equals the spawn slug for orchbus-launched agents, so a
    /// driving session can map `scan --json` rows back to a `spawn`.
    pub(crate) name: String,
    /// The pane's working directory, home-relative (`~/cosmos-stih`). The grouping
    /// key for the directory view, and the displayed column.
    pub(crate) cwd: String,
    pub(crate) title: String,
    pub(crate) question: String,
}

impl Row {
    /// The classifier state this row carries, recovered from its cached rank.
    pub(crate) fn state(&self) -> State {
        state_from_rank(self.rank)
    }

    fn cache_line(&self) -> String {
        format!(
            "{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}",
            self.rank,
            self.pid,
            self.glyph,
            self.agent,
            self.swin,
            self.name,
            self.cwd,
            self.title,
            self.question
        )
    }
    /// `pane_id <TAB> display` — exactly one tab. Everything after it is a single
    /// pre-padded field, because fzf reproduces the row verbatim and renders a
    /// literal tab at the next 8-column stop, so a multi-tab row cannot be made to
    /// line up. Padding here makes the columns exact.
    ///
    /// `dir` leads after the glyph (the grouping column is what the eye wants
    /// first, the way the directory heading does in `claude agents`), and `title`
    /// is gone: `question` already falls back to it, so printing both repeated the
    /// same text on every row that had no question.
    fn list_line(&self, dir_w: usize, win_w: usize) -> String {
        format!(
            "{}\t{}  {:dir_w$}  {:win_w$}  {}  {}",
            self.pid, self.glyph, self.cwd, self.swin, self.agent, self.question
        )
    }
    fn from_cache_line(line: &str) -> Option<Row> {
        let f: Vec<&str> = line.splitn(9, '\t').collect();
        if f.len() != 9 {
            return None;
        }
        Some(Row {
            rank: f[0].parse().unwrap_or(6),
            pid: f[1].into(),
            glyph: f[2].into(),
            agent: f[3].into(),
            swin: f[4].into(),
            name: f[5].into(),
            cwd: f[6].into(),
            title: f[7].into(),
            question: f[8].into(),
        })
    }
}

/// `$HOME/x` -> `~/x`, `$HOME` -> `~`, anything else unchanged. Pure over `home`
/// so it tests without touching the environment.
fn home_relative_in(path: &str, home: &str) -> String {
    if home.is_empty() {
        return path.to_string();
    }
    if path == home {
        return "~".into();
    }
    match path.strip_prefix(home) {
        Some(rest) if rest.starts_with('/') => format!("~{rest}"),
        _ => path.to_string(),
    }
}

fn home_relative(path: &str) -> String {
    home_relative_in(path, &std::env::var("HOME").unwrap_or_default())
}

/// Which order the cockpit is showing. Persisted in a file rather than held by
/// fzf, because the ~1s auto-reload re-runs `scan` as a fresh process — the mode
/// has to outlive the command that renders it or every refresh would snap back.
#[derive(Clone, Copy, PartialEq)]
pub(crate) enum Sort {
    /// Attention first: rank, then pane_id. The triage order.
    Rank,
    /// Directory first — the current one leading — then rank inside it.
    Dir,
}

fn sort_path() -> String {
    format!("{}.sort", cache_path())
}

/// The persisted word for a mode — also what `orchbus sort` prints.
pub(crate) fn sort_label(mode: Sort) -> &'static str {
    match mode {
        Sort::Dir => "dir",
        Sort::Rank => "rank",
    }
}

pub(crate) fn sort_mode() -> Sort {
    match std::fs::read_to_string(sort_path()).as_deref().map(str::trim) {
        Ok("dir") => Sort::Dir,
        _ => Sort::Rank,
    }
}

/// Flip the persisted mode and report the new one (for `orchbus sort --toggle`).
pub(crate) fn toggle_sort() -> Sort {
    let next = match sort_mode() {
        Sort::Rank => Sort::Dir,
        Sort::Dir => Sort::Rank,
    };
    let _ = std::fs::write(sort_path(), sort_label(next));
    next
}

/// Gather the sorted rows for one of the three scan modes — the single scan path
/// that every formatter (fzf TSV, human table, JSON, status) is built on:
///   collect(true, _)         -> the cached rows instantly (no scan)
///   collect(false, None)     -> full scan of every CC pane
///   collect(false, Some(id)) -> rescan only that pane, splice into the cache
pub(crate) fn collect(cache: bool, pane: Option<String>) -> Result<Vec<Row>> {
    if cache {
        return Ok(read_cache_rows());
    }
    match pane {
        Some(p) => splice(&p),
        None => full(),
    }
}

/// The fzf list rows: `pane_id <TAB> aligned display` for each.
pub fn dispatch(cache: bool, pane: Option<String>) -> Result<String> {
    Ok(format_list(&collect(cache, pane)?))
}

/// Pane ids of rows currently showing an approval menu — the `approve/cancel
/// --all` bulk target. Pure over the rows so the selection is unit-testable.
pub(crate) fn approvable(rows: &[Row]) -> Vec<String> {
    rows.iter()
        .filter(|r| r.state() == State::Approve)
        .map(|r| r.pid.clone())
        .collect()
}

/// Pad the dir column to the widest entry so the columns after it line up; fzf
/// renders the row verbatim, so alignment has to be baked in here.
fn format_list(rows: &[Row]) -> String {
    let dir_w = rows.iter().map(|r| r.cwd.chars().count()).max().unwrap_or(0);
    let win_w = rows.iter().map(|r| r.swin.chars().count()).max().unwrap_or(0);
    rows.iter()
        .map(|r| r.list_line(dir_w, win_w))
        .collect::<Vec<_>>()
        .join("\n")
}

/// Last N lines of `s`, rejoined.
fn last_lines(s: &str, n: usize) -> String {
    let lines: Vec<&str> = s.lines().collect();
    let start = lines.len().saturating_sub(n);
    lines[start..].join("\n")
}

/// Build a row for one pane, or `None` if it isn't a live agent pane worth
/// showing. `agent` is the already-detected agent tag (e.g. CC).
fn scan_pane(pid: &str, agent: &str, swin: &str, name: &str, cwd: &str, title: &str) -> Option<Row> {
    let full = tmux::query(["capture-pane", "-p", "-t", pid]).ok()?;
    let text = last_lines(&full, TAIL_LINES);
    if text.trim().is_empty() {
        return None;
    }
    let (rank, glyph) = meta(classify(&text));

    // Prefer the on-screen question (first line ending in ?); else the CC topic
    // (pane_title). Collapse whitespace and drop tabs so the TSV stays clean.
    let question = text
        .lines()
        .find(|l| l.trim_end().ends_with('?'))
        .map(|l| l.split_whitespace().collect::<Vec<_>>().join(" "))
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| title.replace('\t', ""));

    Some(Row {
        rank,
        pid: pid.into(),
        agent: agent.into(),
        glyph: glyph.into(),
        swin: swin.into(),
        name: name.replace('\t', ""),
        cwd: home_relative(cwd).replace('\t', ""),
        title: title.replace('\t', ""),
        question,
    })
}

/// `pane_id \t command \t session:win \t window_name \t cwd \t pane_title` for every
/// pane, all sessions. `pane_title` stays last — it's the only free-text field, so
/// the `splitn(6)` parsers can't be tripped by a tab in it.
fn list_panes() -> Result<String> {
    tmux::query([
        "list-panes",
        "-a",
        "-F",
        "#{pane_id}\t#{pane_current_command}\t#{session_name}:#{window_index}\t#{window_name}\t#{pane_current_path}\t#{pane_title}",
    ])
    .context("list-panes failed")
}

/// The live pane id for a window named `name` running a known agent, or `None` if
/// the window/pane is gone. Used to resolve a spawn slug (`orchbus status <slug>`,
/// `revise`) back to its pane without guessing.
pub(crate) fn pane_for_window(name: &str) -> Result<Option<String>> {
    let panes = tmux::query([
        "list-panes",
        "-a",
        "-F",
        "#{pane_id}\t#{window_name}\t#{pane_current_command}",
    ])?;
    Ok(pane_for_window_in(&panes, name))
}

/// Pure core of [`pane_for_window`] over a `list-panes` listing — unit-testable.
fn pane_for_window_in(listing: &str, name: &str) -> Option<String> {
    listing.lines().find_map(|l| {
        let f: Vec<&str> = l.splitn(3, '\t').collect();
        (f.len() == 3 && f[1] == name && agent::detect(f[2]).is_some()).then(|| f[0].to_string())
    })
}

/// The live state of a spawned agent's window (by name), or `None` if it's gone.
/// The `orchbus status <slug>` / `wait` core: resolve the window, capture its tail,
/// classify it — reusing the same signals the cockpit does.
pub(crate) fn window_state(name: &str) -> Result<Option<(String, State)>> {
    let Some(pid) = pane_for_window(name)? else {
        return Ok(None);
    };
    let full = tmux::query(["capture-pane", "-p", "-t", &pid]).unwrap_or_default();
    let state = classify(&last_lines(&full, TAIL_LINES));
    Ok(Some((pid, state)))
}

/// Scan every agent pane across all sessions.
fn full() -> Result<Vec<Row>> {
    let panes = list_panes()?;
    let rows: Vec<Row> = panes
        .lines()
        .filter_map(|line| {
            let f: Vec<&str> = line.splitn(6, '\t').collect();
            if f.len() == 6 {
                let tag = agent::detect(f[1])?;
                scan_pane(f[0], tag, f[2], f[3], f[4], f[5])
            } else {
                None
            }
        })
        .collect();
    Ok(finalize(rows))
}

/// Rescan only `pid`, splicing its fresh row into the cached list (falls back to
/// a full scan if there's no cache yet).
fn splice(pid: &str) -> Result<Vec<Row>> {
    let cache = match std::fs::read_to_string(cache_path()) {
        Ok(c) => c,
        Err(_) => return full(),
    };

    // Keep cached rows for every OTHER pane.
    let mut rows: Vec<Row> = cache
        .lines()
        .filter_map(Row::from_cache_line)
        .filter(|r| r.pid != pid)
        .collect();

    // Add this pane's fresh row if it's still a live agent pane.
    let panes = list_panes()?;
    if let Some(line) = panes.lines().find(|l| l.starts_with(&format!("{pid}\t"))) {
        let f: Vec<&str> = line.splitn(6, '\t').collect();
        if f.len() == 6 {
            if let Some(tag) = agent::detect(f[1]) {
                if let Some(row) = scan_pane(f[0], tag, f[2], f[3], f[4], f[5]) {
                    rows.push(row);
                }
            }
        }
    }
    Ok(finalize(rows))
}

/// The directory the cockpit was opened from, home-relative — the group that
/// leads in `Sort::Dir`. Empty when it can't be resolved, which simply means no
/// group is privileged.
fn here() -> String {
    std::env::current_dir()
        .map(|p| home_relative(&p.to_string_lossy()))
        .unwrap_or_default()
}

/// Order rows for display. Pure over `mode`/`here` so both orders test without a
/// tmux server or a real working directory.
pub(crate) fn order(rows: &mut [Row], mode: Sort, here: &str) {
    match mode {
        Sort::Rank => rows.sort_by(|a, b| a.rank.cmp(&b.rank).then_with(|| a.pid.cmp(&b.pid))),
        // `cwd != here` is false (0) for the current directory, so it sorts first;
        // the rest fall alphabetically, and rank still decides inside a group.
        Sort::Dir => rows.sort_by(|a, b| {
            (a.cwd != here, &a.cwd, a.rank, &a.pid).cmp(&(b.cwd != here, &b.cwd, b.rank, &b.pid))
        }),
    }
}

/// Order the rows for the active view and cache atomically (WITH rank so a later
/// splice can re-sort), returning them for a formatter to render.
fn finalize(mut rows: Vec<Row>) -> Vec<Row> {
    order(&mut rows, sort_mode(), &here());

    let cache_body = rows
        .iter()
        .map(Row::cache_line)
        .collect::<Vec<_>>()
        .join("\n");
    write_cache_atomic(&cache_body);

    rows
}

fn write_cache_atomic(body: &str) {
    let path = cache_path();
    let tmp = format!("{path}.{}", std::process::id());
    // Best-effort: a failed cache write just means the next open does a full scan.
    if std::fs::write(&tmp, body).is_ok() {
        let _ = std::fs::rename(&tmp, &path);
    }
}

fn read_cache_rows() -> Vec<Row> {
    match std::fs::read_to_string(cache_path()) {
        Ok(c) => c.lines().filter_map(Row::from_cache_line).collect(),
        Err(_) => Vec::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cache_line_round_trips_and_list_drops_rank() {
        let r = Row {
            rank: 1,
            pid: "%3".into(),
            agent: "CC".into(),
            glyph: "[!]".into(),
            swin: "s:1".into(),
            name: "fix-flaky".into(),
            cwd: "~/rc".into(),
            title: "topic".into(),
            question: "proceed?".into(),
        };
        let back = Row::from_cache_line(&r.cache_line()).unwrap();
        assert_eq!(back.pid, "%3");
        assert_eq!(back.agent, "CC");
        assert_eq!(back.name, "fix-flaky");
        assert_eq!(back.question, "proceed?");
        // list_line (fzf display) still drops rank AND window_name — cockpit columns
        // are unchanged; the name only rides the cache + JSON.
        assert_eq!(r.list_line(0, 0), "%3\t[!]  ~/rc  s:1  CC  proceed?");
    }

    #[test]
    fn pane_for_window_matches_named_agent_pane_only() {
        let listing = "\
%1\tother\tvim
%2\tfix-flaky\tclaude
%3\tfix-flaky\tbash
%4\tbuild\tcodex";
        // Matches the agent pane in the window named `fix-flaky`, not the bash pane
        // that shares the name, nor a non-agent (vim) pane.
        assert_eq!(pane_for_window_in(listing, "fix-flaky"), Some("%2".into()));
        assert_eq!(pane_for_window_in(listing, "build"), Some("%4".into()));
        assert_eq!(pane_for_window_in(listing, "missing"), None);
    }

    #[test]
    fn last_lines_takes_tail() {
        assert_eq!(last_lines("a\nb\nc\nd", 2), "c\nd");
        assert_eq!(last_lines("only", 25), "only");
    }

    #[test]
    fn approvable_selects_only_approve_state() {
        let mk = |rank: u8, pid: &str| Row {
            cwd: "~/rc".into(),
            rank,
            pid: pid.into(),
            agent: "CC".into(),
            glyph: String::new(),
            swin: "s:1".into(),
            name: String::new(),
            title: String::new(),
            question: String::new(),
        };
        // rank 1 == Approve; everything else is excluded.
        let rows = vec![mk(1, "%1"), mk(3, "%2"), mk(1, "%3"), mk(2, "%4")];
        assert_eq!(approvable(&rows), vec!["%1", "%3"]);
    }

    fn drow(rank: u8, pid: &str, cwd: &str) -> Row {
        Row {
            rank,
            pid: pid.into(),
            agent: "CC".into(),
            glyph: "[=]".into(),
            swin: "s:1".into(),
            name: "n".into(),
            cwd: cwd.into(),
            title: "t".into(),
            question: "q".into(),
        }
    }

    #[test]
    fn home_relative_only_rewrites_a_real_home_prefix() {
        assert_eq!(home_relative_in("/Users/isg/rc", "/Users/isg"), "~/rc");
        assert_eq!(home_relative_in("/Users/isg", "/Users/isg"), "~");
        // a sibling that merely shares the prefix text must not be rewritten
        assert_eq!(home_relative_in("/Users/isgore/x", "/Users/isg"), "/Users/isgore/x");
        assert_eq!(home_relative_in("/etc", "/Users/isg"), "/etc");
        assert_eq!(home_relative_in("/Users/isg/rc", ""), "/Users/isg/rc");
    }

    #[test]
    fn rank_sort_ignores_directories() {
        let mut rows = vec![drow(4, "%2", "~/a"), drow(1, "%9", "~/z"), drow(1, "%3", "~/a")];
        order(&mut rows, Sort::Rank, "~/z");
        let got: Vec<&str> = rows.iter().map(|r| r.pid.as_str()).collect();
        assert_eq!(got, vec!["%3", "%9", "%2"], "rank then pane_id");
    }

    #[test]
    fn dir_sort_leads_with_here_then_rank_inside_each_group() {
        let mut rows = vec![
            drow(4, "%1", "~/a"),
            drow(1, "%2", "~/a"),
            drow(4, "%3", "~/here"),
            drow(1, "%4", "~/here"),
            drow(1, "%5", "~/b"),
        ];
        order(&mut rows, Sort::Dir, "~/here");
        let got: Vec<&str> = rows.iter().map(|r| r.pid.as_str()).collect();
        // ~/here first (rank inside it), then the rest alphabetically
        assert_eq!(got, vec!["%4", "%3", "%2", "%1", "%5"]);
    }

    #[test]
    fn dir_sort_without_a_current_dir_is_plain_alphabetical() {
        let mut rows = vec![drow(1, "%1", "~/z"), drow(1, "%2", "~/a")];
        order(&mut rows, Sort::Dir, "");
        let got: Vec<&str> = rows.iter().map(|r| r.pid.as_str()).collect();
        assert_eq!(got, vec!["%2", "%1"]);
    }
}
