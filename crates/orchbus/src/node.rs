//! `orchbus node <verb>`: the helper orchbus-node calls for every tmux operation
//! (contract v0 §4). Each verb reads one JSON object on stdin and writes one JSON
//! value on stdout; failures exit 1 with `{"error": "..."}`.
//!
//! Panes are found by the `@orchbus_uid` pane option, never by window name.

use crate::scan;
use anyhow::{bail, Context, Result};
use clap::Subcommand;
use orchbus_agent::classify;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::BTreeMap;
use std::io::Read;
use std::process::{Command, ExitCode};

pub const CONTRACT: &str = "v0";
const UID_OPTION: &str = "@orchbus_uid";

#[derive(Subcommand, Clone, Copy, Debug, PartialEq, Eq)]
pub enum Verb {
    /// `{}` → `{"contract": "v0"}`
    Version,
    /// Start an agent in a new window, or return the pane already running `uid`.
    Spawn,
    /// Kill the pane running `uid`; `killed` is false when there was none.
    Kill,
    /// Every pane carrying an orchbus uid.
    ListPanes,
    /// The on-screen state of the pane running `uid`.
    State,
}

#[derive(Deserialize, Debug, PartialEq)]
struct SpawnReq {
    uid: String,
    /// tmux session to open the window in; created when missing.
    session: String,
    /// Window name, for display only.
    name: String,
    cwd: String,
    argv: Vec<String>,
    #[serde(default)]
    env: BTreeMap<String, String>,
}

#[derive(Deserialize, Debug, PartialEq)]
struct UidReq {
    uid: String,
}

#[derive(Serialize, Debug, PartialEq)]
struct PaneInfo {
    uid: String,
    pane: String,
    pid: i64,
    command: String,
}

pub fn main(verb: Verb) -> ExitCode {
    let mut input = String::new();
    let out = std::io::stdin()
        .read_to_string(&mut input)
        .context("reading stdin")
        .and_then(|_| dispatch(verb, if input.trim().is_empty() { "{}" } else { &input }));
    match out {
        Ok(v) => {
            println!("{v}");
            ExitCode::SUCCESS
        }
        Err(e) => {
            println!("{}", json!({ "error": format!("{e:#}") }));
            ExitCode::FAILURE
        }
    }
}

fn dispatch(verb: Verb, input: &str) -> Result<Value> {
    Ok(match verb {
        Verb::Version => json!({ "contract": CONTRACT }),
        Verb::Spawn => json!({ "pane": spawn(&parse(input)?)? }),
        Verb::Kill => json!({ "killed": kill(&parse::<UidReq>(input)?.uid)? }),
        Verb::ListPanes => serde_json::to_value(list_panes()?)?,
        Verb::State => state(&parse::<UidReq>(input)?.uid)?,
    })
}

fn parse<T: for<'de> Deserialize<'de>>(input: &str) -> Result<T> {
    serde_json::from_str(input).context("parsing request")
}

/// Run tmux and fail on a non-zero exit, returning trimmed stdout.
fn tmux<S: AsRef<str>>(args: &[S]) -> Result<String> {
    let args: Vec<&str> = args.iter().map(AsRef::as_ref).collect();
    let out = Command::new("tmux").args(&args).output().context("running tmux")?;
    if !out.status.success() {
        bail!("tmux {}: {}", args.first().unwrap_or(&""), String::from_utf8_lossy(&out.stderr).trim());
    }
    Ok(String::from_utf8_lossy(&out.stdout).trim_end().to_string())
}

fn spawn(r: &SpawnReq) -> Result<String> {
    if r.argv.is_empty() {
        bail!("argv is empty");
    }
    if let Some(p) = pane_of(&r.uid)? {
        return Ok(p.pane);
    }
    let has_session = Command::new("tmux")
        .args(["has-session", "-t", &format!("={}", r.session)])
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false);
    let pane = tmux(&spawn_args(r, has_session))?;
    tmux(&["set-option", "-p", "-t", &pane, UID_OPTION, &r.uid])?;
    Ok(pane)
}

/// The tmux argv that opens `r` detached and prints its pane id: a new window in an
/// existing session, or a new session whose first window is the agent.
fn spawn_args(r: &SpawnReq, has_session: bool) -> Vec<String> {
    let mut a: Vec<String> = if has_session {
        vec!["new-window".into(), "-d".into(), "-t".into(), format!("={}:", r.session)]
    } else {
        vec!["new-session".into(), "-d".into(), "-s".into(), r.session.clone()]
    };
    a.extend(["-n".into(), r.name.clone(), "-c".into(), r.cwd.clone(), "-P".into(), "-F".into(), "#{pane_id}".into()]);
    for (k, v) in &r.env {
        a.extend(["-e".into(), format!("{k}={v}")]);
    }
    a.push("--".into());
    a.extend(r.argv.iter().cloned());
    a
}

fn kill(uid: &str) -> Result<bool> {
    match pane_of(uid)? {
        Some(p) => tmux(&["kill-pane", "-t", &p.pane]).map(|_| true),
        None => Ok(false),
    }
}

fn list_panes() -> Result<Vec<PaneInfo>> {
    let fmt = format!("#{{pane_id}}\t#{{{UID_OPTION}}}\t#{{pane_pid}}\t#{{pane_current_command}}");
    match tmux(&["list-panes", "-a", "-F", &fmt]) {
        Ok(listing) => Ok(parse_panes(&listing)),
        Err(e) if no_server(&e.to_string()) => Ok(Vec::new()),
        Err(e) => Err(e),
    }
}

/// tmux's two ways of saying there is no server: none started, or its socket is gone.
fn no_server(err: &str) -> bool {
    err.contains("no server running") || err.contains("error connecting to")
}

fn parse_panes(listing: &str) -> Vec<PaneInfo> {
    listing
        .lines()
        .filter_map(|l| {
            let f: Vec<&str> = l.splitn(4, '\t').collect();
            let [pane, uid, pid, command] = f[..] else { return None };
            (!uid.is_empty()).then(|| PaneInfo {
                uid: uid.into(),
                pane: pane.into(),
                pid: pid.parse().unwrap_or(0),
                command: command.into(),
            })
        })
        .collect()
}

fn pane_of(uid: &str) -> Result<Option<PaneInfo>> {
    Ok(list_panes()?.into_iter().find(|p| p.uid == uid))
}

fn state(uid: &str) -> Result<Value> {
    let p = pane_of(uid)?.with_context(|| format!("no pane for uid {uid}"))?;
    let screen = tmux(&["capture-pane", "-p", "-t", &p.pane])?;
    let text = scan::last_lines(&screen, scan::TAIL_LINES);
    let st = classify::classify(&text);
    Ok(json!({
        "pane": p.pane,
        "state": classify::label(st),
        "question": scan::live_question(&text, st),
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn req() -> SpawnReq {
        serde_json::from_str(
            r#"{"uid":"u1","session":"demo","name":"agent-a","cwd":"/w","argv":["claude","-p","go"],
                "env":{"ORCHBUS_POD_UID":"u1"},"futureField":true}"#,
        )
        .unwrap()
    }

    #[test]
    fn spawn_request_tolerates_unknown_fields_and_defaults_env() {
        let r: SpawnReq = serde_json::from_str(r#"{"uid":"u","session":"s","name":"n","cwd":"/","argv":["x"]}"#).unwrap();
        assert!(r.env.is_empty());
        assert_eq!(req().env["ORCHBUS_POD_UID"], "u1");
    }

    #[test]
    fn spawn_opens_a_window_in_an_existing_session() {
        let a = spawn_args(&req(), true);
        assert_eq!(
            a,
            ["new-window", "-d", "-t", "=demo:", "-n", "agent-a", "-c", "/w", "-P", "-F", "#{pane_id}",
             "-e", "ORCHBUS_POD_UID=u1", "--", "claude", "-p", "go"]
        );
    }

    #[test]
    fn spawn_creates_the_session_when_missing() {
        let a = spawn_args(&req(), false);
        assert_eq!(&a[..4], ["new-session", "-d", "-s", "demo"]);
        assert_eq!(a.last().unwrap(), "go");
    }

    #[test]
    fn list_panes_keeps_only_orchbus_panes() {
        let listing = "%1\tu1\t42\tclaude\n%2\t\t43\tzsh\n%3\tu3\tx\tcodex\nbroken";
        let panes = parse_panes(listing);
        assert_eq!(panes.len(), 2);
        assert_eq!(panes[0], PaneInfo { uid: "u1".into(), pane: "%1".into(), pid: 42, command: "claude".into() });
        assert_eq!(panes[1].pid, 0);
    }

    #[test]
    fn a_missing_server_is_recognised() {
        assert!(no_server("tmux list-panes: no server running on /tmp/tmux-501/default"));
        assert!(no_server("tmux list-panes: error connecting to /tmp/tmux-501/default (No such file or directory)"));
        assert!(!no_server("tmux list-panes: unknown option"));
    }

    #[test]
    fn version_needs_no_input() {
        assert_eq!(dispatch(Verb::Version, "{}").unwrap(), json!({"contract": "v0"}));
    }

    #[test]
    fn malformed_requests_are_errors() {
        assert!(dispatch(Verb::Kill, "{}").is_err());
        assert!(dispatch(Verb::Spawn, "not json").is_err());
    }
}
