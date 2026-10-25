//! The state the `orchbus-probe` Claude Code plugin reports from inside a
//! session, one file per tmux pane (contract v0 §5). It outranks the screen;
//! a pane without a usable file is read from the screen alone.

use crate::classify::{shows_approve_menu, State};
use crate::transcript::encode_cwd;
use serde::Deserialize;
use std::path::{Path, PathBuf};

#[derive(Deserialize, Copy, Clone, PartialEq, Eq, Debug)]
#[serde(rename_all = "lowercase")]
pub enum Reported {
    Running,
    Idle,
    Approve,
    Input,
    Ended,
}

#[derive(Deserialize, Clone, PartialEq, Eq, Debug)]
pub struct Probe {
    pub v: u32,
    pub state: Reported,
    #[serde(default)]
    pub question: String,
    #[serde(default)]
    pub tool: String,
    #[serde(default)]
    pub tool_use_id: String,
    #[serde(default)]
    pub session_id: String,
    pub server_pid: u32,
    #[serde(default)]
    pub pane: String,
    #[serde(default)]
    pub since: u64,
    #[serde(default)]
    pub updated: u64,
}

/// What `display-message -p` prints for [`Server::parse`].
pub const SERVER_FORMAT: &str = "#{socket_path}\t#{pid}";

/// The tmux server a pane id belongs to: ids restart with the server.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Server {
    pub socket: String,
    pub pid: u32,
}

impl Server {
    /// Parse a [`SERVER_FORMAT`] line.
    pub fn parse(line: &str) -> Option<Server> {
        let (socket, pid) = line.trim_end().rsplit_once('\t')?;
        Some(Server { socket: socket.into(), pid: pid.parse().ok()? })
    }
}

/// `${XDG_STATE_HOME:-~/.local/state}/orchbus/panes`.
pub fn root() -> Option<PathBuf> {
    let var = |k| std::env::var_os(k).filter(|v| !v.is_empty());
    let base = match var("XDG_STATE_HOME") {
        Some(dir) => PathBuf::from(dir),
        None => PathBuf::from(var("HOME")?).join(".local/state"),
    };
    Some(base.join("orchbus").join("panes"))
}

pub fn path(root: &Path, server: &Server, pane: &str) -> PathBuf {
    let n = pane.trim_start_matches('%');
    root.join(encode_cwd(&server.socket)).join(format!("{n}.json"))
}

/// A probe that still speaks for the pane: format 1, written under this very
/// server, and not ended. `None` sends the reader back to the screen.
pub fn parse(text: &str, server: &Server) -> Option<Probe> {
    let p: Probe = serde_json::from_str(text).ok()?;
    let live = p.v == 1 && p.server_pid == server.pid;
    (live && p.state != Reported::Ended).then_some(p)
}

pub fn read(server: &Server, pane: &str) -> Option<Probe> {
    let text = std::fs::read_to_string(path(&root()?, server, pane)).ok()?;
    parse(&text, server)
}

impl Probe {
    /// The pane's state, given what the screen shows. The screen only adds
    /// what no hook sees: the rating prompt over an idle session, and that an
    /// answered dialog is gone while its tool still runs.
    pub fn decide(&self, screen: State, has_menu: bool) -> State {
        match self.state {
            Reported::Approve if has_menu => State::Approve,
            Reported::Approve | Reported::Running => State::Running,
            Reported::Input => State::Input,
            Reported::Idle if screen == State::Rating => State::Rating,
            Reported::Idle | Reported::Ended => State::Idle,
        }
    }
}

/// May an approval keystroke go to this screen? Only while the menu shows
/// and, when the pane has a usable probe, the probe says it is asking.
pub fn approvable(screen: &str, probe: Option<&Probe>) -> bool {
    let menu = shows_approve_menu(screen);
    probe.map_or(menu, |p| p.decide(State::Unknown, menu) == State::Approve)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn server() -> Server {
        Server { socket: "/private/tmp/tmux-501/default".into(), pid: 4242 }
    }

    fn probe(state: &str) -> String {
        format!(
            r#"{{"v":1,"state":"{state}","question":"Bash: ls","tool":"Bash",
            "tool_use_id":"t1","session_id":"s","server_pid":4242,
            "pane":"%7","since":1,"updated":2,"future":true}}"#
        )
    }

    #[test]
    fn server_parses_display_message() {
        let line = "/private/tmp/tmux-501/default\t4242\n";
        assert_eq!(Server::parse(line), Some(server()));
        assert_eq!(Server::parse("/tmp/s\tnope"), None);
        assert_eq!(Server::parse(""), None);
    }

    #[test]
    fn path_encodes_the_socket_and_drops_the_percent() {
        let p = path(Path::new("/st"), &server(), "%7");
        assert_eq!(p, Path::new("/st/-private-tmp-tmux-501-default/7.json"));
    }

    #[test]
    fn parse_reads_a_live_probe_and_ignores_unknown_fields() {
        let p = parse(&probe("approve"), &server()).unwrap();
        assert_eq!(p.state, Reported::Approve);
        assert_eq!(p.tool_use_id, "t1");
        assert_eq!(p.question, "Bash: ls");
    }

    #[test]
    fn parse_rejects_what_no_longer_speaks_for_the_pane() {
        let other = Server { pid: 1, ..server() };
        assert_eq!(parse(&probe("idle"), &other), None, "older server");
        assert_eq!(parse(&probe("ended"), &server()), None, "ended");
        assert_eq!(parse(&probe("dreaming"), &server()), None, "unknown");
        let v2 = probe("idle").replace(r#""v":1"#, r#""v":2"#);
        assert_eq!(parse(&v2, &server()), None, "unknown version");
        assert_eq!(parse("{\"v\":1,", &server()), None, "torn write");
    }

    #[test]
    fn decide_trusts_the_probe_over_the_screen() {
        let p = |s| parse(&probe(s), &server()).unwrap();
        assert_eq!(p("approve").decide(State::Unknown, true), State::Approve);
        assert_eq!(p("input").decide(State::Approve, true), State::Input);
        assert_eq!(p("running").decide(State::Idle, false), State::Running);
        assert_eq!(p("idle").decide(State::Running, false), State::Idle);
    }

    #[test]
    fn decide_keeps_the_screen_only_signals() {
        let p = |s| parse(&probe(s), &server()).unwrap();
        assert_eq!(p("idle").decide(State::Rating, true), State::Rating);
        assert_eq!(p("approve").decide(State::Running, false), State::Running);
    }

    #[test]
    fn approvable_needs_the_menu_and_an_agreeing_probe() {
        let menu = "Do you want to proceed?\n ❯ 1. Yes\n   2. No";
        let p = |s| parse(&probe(s), &server()).unwrap();
        assert!(approvable(menu, None), "no probe: the screen decides");
        assert!(!approvable("  ❯ ", None));
        assert!(approvable(menu, Some(&p("approve"))));
        assert!(!approvable("  ❯ ", Some(&p("approve"))), "dialog answered");
        assert!(!approvable(menu, Some(&p("input"))), "a question's menu");
        assert!(!approvable(menu, Some(&p("idle"))), "the rating menu");
    }
}
