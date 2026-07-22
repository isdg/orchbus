//! Human- and machine-readable renderings of the scanned pane list.
//!
//! The fzf cockpit consumes the raw TSV from `scan::dispatch`; these formatters
//! are for driving orchbus straight from a shell: `list` (an aligned, optionally
//! colored table), `status` (a one-line state tally), and `--json` (both, as
//! structured data). All are pure `&[Row] -> String` so they unit-test cleanly.

use crate::classify::{self, State};
use crate::scan::Row;
use serde::Serialize;
use std::io::IsTerminal;

/// The six states in importance order — the column order for `status`.
const ORDER: [State; 6] = [
    State::Approve,
    State::Input,
    State::Running,
    State::Idle,
    State::Rating,
    State::Unknown,
];

/// SGR color for a state's glyph (mirrors the cockpit's attention ranking):
/// approve red, input magenta, running yellow, idle green, rating blue, unknown dim.
fn color(state: State) -> &'static str {
    match state {
        State::Approve => "31",
        State::Input => "35",
        State::Running => "33",
        State::Idle => "32",
        State::Rating => "34",
        State::Unknown => "2",
    }
}

/// An aligned table: `glyph  session:win  agent  question`. Only the short ASCII
/// columns (`swin`, `agent`) are padded; `question` runs free at the end (it
/// already falls back to the pane topic, so no separate topic column is needed).
/// The glyph is colored when stdout is a terminal.
pub fn human(rows: &[Row]) -> String {
    human_inner(rows, std::io::stdout().is_terminal())
}

fn human_inner(rows: &[Row], tty: bool) -> String {
    let win_w = rows.iter().map(|r| r.swin.len()).max().unwrap_or(0);
    let agent_w = rows.iter().map(|r| r.agent.len()).max().unwrap_or(0);

    rows.iter()
        .map(|r| {
            let glyph = if tty {
                format!("\x1b[{}m{}\x1b[0m", color(r.state()), r.glyph)
            } else {
                r.glyph.clone()
            };
            format!("{glyph}  {:win_w$}  {:agent_w$}  {}", r.swin, r.agent, r.question)
        })
        .collect::<Vec<_>>()
        .join("\n")
}

/// One-line tally, e.g. `1 approve · 0 input · 1 running · 2 idle · 0 rating`
/// (the `unknown` bucket is appended only when non-empty).
pub fn status(rows: &[Row]) -> String {
    let counts = counts(rows);
    let mut parts: Vec<String> = ORDER
        .iter()
        .take(5) // approve..rating always shown
        .map(|s| format!("{} {}", counts[*s as usize], classify::label(*s)))
        .collect();
    let unknown = counts[State::Unknown as usize];
    if unknown > 0 {
        parts.push(format!("{unknown} unknown"));
    }
    parts.join(" · ")
}

/// Does any pane need approval? Drives `status`'s exit code.
pub fn any_waiting(rows: &[Row]) -> bool {
    rows.iter().any(|r| r.state() == State::Approve)
}

/// Per-state counts indexed by `State as usize`.
fn counts(rows: &[Row]) -> [usize; 6] {
    let mut c = [0usize; 6];
    for r in rows {
        c[r.state() as usize] += 1;
    }
    c
}

#[derive(Serialize)]
struct RowView<'a> {
    pane: &'a str,
    state: &'static str,
    glyph: &'a str,
    agent: &'a str,
    window: &'a str,
    /// tmux window name — the spawn slug for orchbus-launched agents, so a driving
    /// session can match a `scan --json` row back to its `spawn`.
    name: &'a str,
    topic: &'a str,
    question: &'a str,
}

/// The scanned rows as a JSON array (for `scan --json` / `list --json`).
pub fn json_rows(rows: &[Row]) -> String {
    let views: Vec<RowView> = rows
        .iter()
        .map(|r| RowView {
            pane: &r.pid,
            state: classify::label(r.state()),
            glyph: &r.glyph,
            agent: &r.agent,
            window: &r.swin,
            name: &r.name,
            topic: &r.title,
            question: &r.question,
        })
        .collect();
    serde_json::to_string(&views).unwrap_or_else(|_| "[]".into())
}

/// One spawned agent's live state as JSON (for `status <slug> --json`):
/// `{"slug":…,"state":…,"pane":…}`, with `pane` null when the window is gone.
pub fn slug_status_json(slug: &str, state: &str, pane: Option<&str>) -> String {
    let mut map = serde_json::Map::new();
    map.insert("slug".into(), slug.into());
    map.insert("state".into(), state.into());
    map.insert("pane".into(), pane.map(serde_json::Value::from).unwrap_or(serde_json::Value::Null));
    serde_json::Value::Object(map).to_string()
}

/// The state tally as a JSON object (for `status --json`).
pub fn status_json(rows: &[Row]) -> String {
    let c = counts(rows);
    let mut map = serde_json::Map::new();
    for s in ORDER {
        map.insert(classify::label(s).into(), c[s as usize].into());
    }
    map.insert("total".into(), rows.len().into());
    map.insert("waiting".into(), any_waiting(rows).into());
    serde_json::Value::Object(map).to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn row(rank: u8, glyph: &str, swin: &str, title: &str, question: &str) -> Row {
        Row {
            rank,
            pid: "%1".into(),
            agent: "CC".into(),
            glyph: glyph.into(),
            swin: swin.into(),
            name: "slug".into(),
            title: title.into(),
            question: question.into(),
        }
    }

    #[test]
    fn human_pads_columns_and_omits_color_without_tty() {
        let rows = vec![
            row(1, "[!]", "s:1", "refactor", "proceed?"),
            row(4, "[=]", "session:10", "x", "(idle)"),
        ];
        let out = human_inner(&rows, false);
        assert!(!out.contains('\x1b'), "no ANSI when not a tty");
        // both window cells padded to width of "session:10" (10 chars)
        assert!(out.contains("[!]  s:1         CC  proceed?"));
        assert!(out.contains("[=]  session:10  CC  (idle)"));
    }

    #[test]
    fn status_counts_by_state_and_hides_zero_unknown() {
        let rows = vec![
            row(1, "[!]", "s:1", "a", "?"),
            row(1, "[!]", "s:2", "b", "?"),
            row(3, "[*]", "s:3", "c", "run"),
        ];
        assert_eq!(status(&rows), "2 approve · 0 input · 1 running · 0 idle · 0 rating");
        assert!(any_waiting(&rows));
    }

    #[test]
    fn status_appends_unknown_when_present() {
        let rows = vec![row(6, "[.]", "s:1", "a", "?")];
        assert_eq!(
            status(&rows),
            "0 approve · 0 input · 0 running · 0 idle · 0 rating · 1 unknown"
        );
        assert!(!any_waiting(&rows));
    }

    #[test]
    fn json_rows_shape() {
        let out = json_rows(&[row(1, "[!]", "s:1", "topic", "proceed?")]);
        assert!(out.starts_with('['));
        assert!(out.contains(r#""state":"approve""#));
        assert!(out.contains(r#""window":"s:1""#));
        assert!(out.contains(r#""name":"slug""#));
        assert!(out.contains(r#""question":"proceed?""#));
    }

    #[test]
    fn slug_status_json_shape() {
        let live = slug_status_json("fix-flaky", "idle", Some("%7"));
        assert!(live.contains(r#""slug":"fix-flaky""#));
        assert!(live.contains(r#""state":"idle""#));
        assert!(live.contains(r#""pane":"%7""#));
        let gone = slug_status_json("fix-flaky", "gone", None);
        assert!(gone.contains(r#""state":"gone""#));
        assert!(gone.contains(r#""pane":null"#));
    }

    #[test]
    fn status_json_shape() {
        let out = status_json(&[row(1, "[!]", "s:1", "a", "?")]);
        assert!(out.contains(r#""approve":1"#));
        assert!(out.contains(r#""total":1"#));
        assert!(out.contains(r#""waiting":true"#));
    }
}
