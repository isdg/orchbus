//! Emit one row per Claude Code (CC) pane for the cockpit, and keep a cache so
//! a single-pane rescan (after approve/cancel) updates instantly without
//! re-scanning every pane.
//!
//! Cache row (7 fields): rank <TAB> pane_id <TAB> agent <TAB> glyph <TAB> session:win <TAB> topic <TAB> question
//! List row  (6 fields): pane_id <TAB> agent <TAB> glyph <TAB> session:win <TAB> topic <TAB> question
//!
//! pane_id (e.g. %23) is the sole tmux target the UI uses; fields 2.. are display
//! only (the UI hides field 1 from fzf matching with --with-nth=2..). `agent` is
//! the running-agent tag (e.g. CC = Claude Code) so mixed-agent fleets stay
//! legible as we scale beyond Claude Code.

use crate::agent;
use crate::classify::{classify, meta};
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

struct Row {
    rank: u8,
    pid: String,
    agent: String,
    glyph: String,
    swin: String,
    title: String,
    question: String,
}

impl Row {
    fn cache_line(&self) -> String {
        format!(
            "{}\t{}\t{}\t{}\t{}\t{}\t{}",
            self.rank, self.pid, self.agent, self.glyph, self.swin, self.title, self.question
        )
    }
    fn list_line(&self) -> String {
        format!(
            "{}\t{}\t{}\t{}\t{}\t{}",
            self.pid, self.agent, self.glyph, self.swin, self.title, self.question
        )
    }
    fn from_cache_line(line: &str) -> Option<Row> {
        let f: Vec<&str> = line.splitn(7, '\t').collect();
        if f.len() != 7 {
            return None;
        }
        Some(Row {
            rank: f[0].parse().unwrap_or(6),
            pid: f[1].into(),
            agent: f[2].into(),
            glyph: f[3].into(),
            swin: f[4].into(),
            title: f[5].into(),
            question: f[6].into(),
        })
    }
}

/// Dispatch matching scan.sh:
///   dispatch(true, _)         -> print the cached list instantly (no scan)
///   dispatch(false, None)     -> full scan of every CC pane
///   dispatch(false, Some(id)) -> rescan only that pane, splice into the cache
pub fn dispatch(cache: bool, pane: Option<String>) -> Result<String> {
    if cache {
        return Ok(read_cache_list());
    }
    match pane {
        Some(p) => splice(&p),
        None => full(),
    }
}

/// Last N lines of `s`, rejoined.
fn last_lines(s: &str, n: usize) -> String {
    let lines: Vec<&str> = s.lines().collect();
    let start = lines.len().saturating_sub(n);
    lines[start..].join("\n")
}

/// Build a row for one pane, or `None` if it isn't a live agent pane worth
/// showing. `agent` is the already-detected agent tag (e.g. CC).
fn scan_pane(pid: &str, agent: &str, swin: &str, title: &str) -> Option<Row> {
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
        title: title.replace('\t', ""),
        question,
    })
}

/// `pane_id \t command \t session:win \t pane_title` for every pane, all sessions.
fn list_panes() -> Result<String> {
    tmux::query([
        "list-panes",
        "-a",
        "-F",
        "#{pane_id}\t#{pane_current_command}\t#{session_name}:#{window_index}\t#{pane_title}",
    ])
    .context("list-panes failed")
}

/// Scan every agent pane across all sessions.
fn full() -> Result<String> {
    let panes = list_panes()?;
    let rows: Vec<Row> = panes
        .lines()
        .filter_map(|line| {
            let f: Vec<&str> = line.splitn(4, '\t').collect();
            if f.len() == 4 {
                let tag = agent::detect(f[1])?;
                scan_pane(f[0], tag, f[2], f[3])
            } else {
                None
            }
        })
        .collect();
    Ok(finalize(rows))
}

/// Rescan only `pid`, splicing its fresh row into the cached list (falls back to
/// a full scan if there's no cache yet).
fn splice(pid: &str) -> Result<String> {
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
        let f: Vec<&str> = line.splitn(4, '\t').collect();
        if f.len() == 4 {
            if let Some(tag) = agent::detect(f[1]) {
                if let Some(row) = scan_pane(f[0], tag, f[2], f[3]) {
                    rows.push(row);
                }
            }
        }
    }
    Ok(finalize(rows))
}

/// Sort by importance (rank, then pane_id), cache atomically (WITH rank so a
/// later splice can re-sort), and return the rank-stripped 5-field list.
fn finalize(mut rows: Vec<Row>) -> String {
    rows.sort_by(|a, b| a.rank.cmp(&b.rank).then_with(|| a.pid.cmp(&b.pid)));

    let cache_body = rows
        .iter()
        .map(Row::cache_line)
        .collect::<Vec<_>>()
        .join("\n");
    write_cache_atomic(&cache_body);

    rows.iter().map(Row::list_line).collect::<Vec<_>>().join("\n")
}

fn write_cache_atomic(body: &str) {
    let path = cache_path();
    let tmp = format!("{path}.{}", std::process::id());
    // Best-effort: a failed cache write just means the next open does a full scan.
    if std::fs::write(&tmp, body).is_ok() {
        let _ = std::fs::rename(&tmp, &path);
    }
}

fn read_cache_list() -> String {
    match std::fs::read_to_string(cache_path()) {
        Ok(c) => c
            .lines()
            .filter_map(|l| l.splitn(2, '\t').nth(1)) // drop the rank column
            .collect::<Vec<_>>()
            .join("\n"),
        Err(_) => String::new(),
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
            title: "topic".into(),
            question: "proceed?".into(),
        };
        let back = Row::from_cache_line(&r.cache_line()).unwrap();
        assert_eq!(back.pid, "%3");
        assert_eq!(back.agent, "CC");
        assert_eq!(back.question, "proceed?");
        assert_eq!(r.list_line(), "%3\tCC\t[!]\ts:1\ttopic\tproceed?");
    }

    #[test]
    fn last_lines_takes_tail() {
        assert_eq!(last_lines("a\nb\nc\nd", 2), "c\nd");
        assert_eq!(last_lines("only", 25), "only");
    }
}
