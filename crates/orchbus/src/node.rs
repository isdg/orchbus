//! `orchbus node <verb>`: the helper orchbus-node calls for every tmux operation
//! (contract v0 §4). Each verb reads one JSON object on stdin and writes one JSON
//! value on stdout; failures exit 1 with `{"error": "..."}`.
//!
//! Panes are found by the `@orchbus_uid` pane option, never by window name.

use crate::scan;
use anyhow::{bail, Context, Result};
use clap::Subcommand;
use orchbus_agent::{classify, keys};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::BTreeMap;
use std::io::Read;
use std::process::{Command, ExitCode};

pub const CONTRACT: &str = "v0";
const UID_OPTION: &str = "@orchbus_uid";
/// Exact tmux socket to use instead of the default server; passed as `-S`, which has no fallback.
const SOCKET_ENV: &str = "ORCHBUS_TMUX_SOCKET";

fn tmux_command() -> Command {
    let mut c = Command::new("tmux");
    if let Some(sock) = std::env::var_os(SOCKET_ENV).filter(|s| !s.is_empty()) {
        c.arg("-S").arg(sock);
    }
    c
}

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
    /// Pick a menu option, only if an approval menu is still showing.
    Approve,
    /// Send Escape: dismiss the prompt.
    Cancel,
    /// Send Escape: interrupt the running turn.
    Interrupt,
    /// Type one line and submit it.
    Send,
    /// The pane's screen, with `lines` of scrollback before it.
    Capture,
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

#[derive(Deserialize, Debug, PartialEq)]
struct ApproveReq {
    uid: String,
    /// 1-based menu option; the highlighted default when absent.
    #[serde(default)]
    choice: Option<u8>,
}

#[derive(Deserialize, Debug, PartialEq)]
struct SendReq {
    uid: String,
    text: String,
}

#[derive(Deserialize, Debug, PartialEq)]
struct CaptureReq {
    uid: String,
    #[serde(default)]
    lines: u32,
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
        Verb::Approve => json!({ "sent": approve(&parse(input)?)? }),
        Verb::Cancel | Verb::Interrupt => {
            press(&pane(&parse::<UidReq>(input)?.uid)?, &keys::escape())?;
            json!({ "sent": true })
        }
        Verb::Send => json!({ "sent": send(&parse(input)?)? }),
        Verb::Capture => json!({ "text": capture(&parse(input)?)? }),
    })
}

fn parse<T: for<'de> Deserialize<'de>>(input: &str) -> Result<T> {
    serde_json::from_str(input).context("parsing request")
}

/// Run tmux and fail on a non-zero exit, returning trimmed stdout.
fn tmux<S: AsRef<str>>(args: &[S]) -> Result<String> {
    let args: Vec<&str> = args.iter().map(AsRef::as_ref).collect();
    let out = tmux_command().args(&args).output().context("running tmux")?;
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
    let has_session = tmux_command()
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

fn pane(uid: &str) -> Result<String> {
    Ok(pane_of(uid)?.with_context(|| format!("no pane for uid {uid}"))?.pane)
}

fn press(pane: &str, seq: &[keys::Keys]) -> Result<()> {
    for k in seq {
        let mut args = vec!["send-keys", "-t", pane];
        args.extend(k.args());
        tmux(&args)?;
    }
    Ok(())
}

/// Re-read the screen right before sending, so a menu that closed since the caller looked
/// never receives a stray keystroke. `false` when there was no menu to answer.
fn approve(r: &ApproveReq) -> Result<bool> {
    if r.choice.is_some_and(|c| !(1..=9).contains(&c)) {
        bail!("choice must be 1-9");
    }
    let pane = pane(&r.uid)?;
    if !classify::shows_approve_menu(&tmux(&["capture-pane", "-p", "-t", &pane])?) {
        return Ok(false);
    }
    press(&pane, &keys::approve(r.choice))?;
    Ok(true)
}

fn send(r: &SendReq) -> Result<bool> {
    if r.text.contains(['\n', '\r']) {
        bail!("text must be a single line: a newline would submit it early");
    }
    if r.text.trim().is_empty() {
        bail!("text is empty");
    }
    press(&pane(&r.uid)?, &keys::send(&r.text))?;
    Ok(true)
}

fn capture(r: &CaptureReq) -> Result<String> {
    let pane = pane(&r.uid)?;
    let start = format!("-{}", r.lines);
    let mut args = vec!["capture-pane", "-p", "-t", pane.as_str()];
    if r.lines > 0 {
        args.extend(["-S", &start]);
    }
    tmux(&args)
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
    fn action_requests_validate_before_touching_tmux() {
        assert!(dispatch(Verb::Approve, r#"{"uid":"u","choice":12}"#).unwrap_err().to_string().contains("1-9"));
        let multi = dispatch(Verb::Send, r#"{"uid":"u","text":"a\nb"}"#).unwrap_err().to_string();
        assert!(multi.contains("single line"), "{multi}");
        assert!(dispatch(Verb::Send, r#"{"uid":"u","text":"  "}"#).is_err());
    }

    #[test]
    fn malformed_requests_are_errors() {
        assert!(dispatch(Verb::Kill, "{}").is_err());
        assert!(dispatch(Verb::Spawn, "not json").is_err());
    }
}
