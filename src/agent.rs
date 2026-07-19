//! Registry of the terminal agents orchbus can observe and drive.
//!
//! `pane_current_command` tells us which agent runs in a pane; the registry maps
//! that to a short display tag (shown as a column in the cockpit) and — for agents
//! orchbus can *launch* — to a command-line builder used by `spawn`/`revise`/`fork`.
//!
//! Today only Claude Code is fully driveable; `codex`/`opencode` are registered so
//! they're *detected* (the tag column stays legible in a mixed fleet), but their
//! argv builder is not wired yet. Add a kind below to teach orchbus a new agent.

use anyhow::{bail, Context, Result};
use std::io::Write;
use std::path::Path;
use std::process::{Command, Stdio};

/// What the caller wants an agent invocation to do. Only the fields relevant to a
/// given verb are set (e.g. `spawn` sets `session_id`+`prompt`; `fork` sets
/// `resume`+`fork`). See `Agent::argv`.
// Consumed by spawn/revise/fork (Track B3+); tested here in the meantime.
#[allow(dead_code)]
#[derive(Default)]
pub struct Launch<'a> {
    /// Pin a new session id (`--session-id`) so we can resume/fork it later.
    pub session_id: Option<&'a str>,
    /// Resume an existing session (`--resume`).
    pub resume: Option<&'a str>,
    /// Fork the resumed session into a fresh id (`--fork-session`).
    pub fork: bool,
    /// Permission mode, e.g. `plan` (`--permission-mode`).
    pub permission_mode: Option<&'a str>,
    /// Bypass permission prompts — only honored when no `permission_mode` is set
    /// (they're the same knob; passing both would conflict). Gated on isolation.
    pub skip_perms: bool,
    /// Extra system-prompt text — how a role/tag is injected (`--append-system-prompt`).
    pub role: Option<&'a str>,
    /// Headless run capturing structured JSON (`-p --output-format json`).
    pub headless: bool,
    /// The user prompt (trailing positional).
    pub prompt: Option<&'a str>,
}

#[derive(Copy, Clone, PartialEq, Eq, Debug)]
pub enum Kind {
    Claude,
    Codex,
    Opencode,
}

pub struct Agent {
    /// Short display tag (cockpit column).
    pub tag: &'static str,
    /// The `pane_current_command` this agent runs as.
    pub command: &'static str,
    #[allow(dead_code)] // read by `argv`, wired from Track B3+
    pub kind: Kind,
}

/// Every agent orchbus knows. Order is irrelevant; `command` is the key.
static REGISTRY: &[Agent] = &[
    Agent { tag: "CC", command: "claude", kind: Kind::Claude },
    Agent { tag: "CX", command: "codex", kind: Kind::Codex },
    Agent { tag: "OC", command: "opencode", kind: Kind::Opencode },
];

/// The agent behind a `pane_current_command`, or `None` if it isn't one we track.
pub fn for_command(cmd: &str) -> Option<&'static Agent> {
    REGISTRY.iter().find(|a| a.command == cmd)
}

/// Short tag for a `pane_current_command` (the scanner's filter + display column).
pub fn detect(pane_current_command: &str) -> Option<&'static str> {
    for_command(pane_current_command).map(|a| a.tag)
}

impl Agent {
    /// Build the argv to launch/resume this agent. `None` for agents not yet wired
    /// to drive (callers surface a clear "can't drive <agent> yet" error).
    #[allow(dead_code)] // called by spawn/revise/fork (Track B3+)
    pub fn argv(&self, o: &Launch) -> Option<Vec<String>> {
        match self.kind {
            Kind::Claude => Some(claude_argv(o)),
            Kind::Codex | Kind::Opencode => None,
        }
    }
}

/// Assemble a `claude` command line from `Launch`. Flag order is stable so it's
/// unit-testable; `skip_perms` yields to an explicit `permission_mode` (both map
/// to the same permission knob and Claude rejects the pair).
#[allow(dead_code)] // reached via Agent::argv from Track B3+
fn claude_argv(o: &Launch) -> Vec<String> {
    let mut a = vec!["claude".to_string()];
    let mut push = |s: &str| a.push(s.to_string());

    if let Some(id) = o.session_id {
        push("--session-id");
        push(id);
    }
    if let Some(id) = o.resume {
        push("--resume");
        push(id);
    }
    if o.fork {
        push("--fork-session");
    }
    if let Some(m) = o.permission_mode {
        push("--permission-mode");
        push(m);
    } else if o.skip_perms {
        push("--dangerously-skip-permissions");
    }
    if let Some(r) = o.role {
        push("--append-system-prompt");
        push(r);
    }
    if o.headless {
        push("-p");
        push("--output-format");
        push("json");
    }
    if let Some(p) = o.prompt {
        push(p);
    }
    a
}

/// Run a headless agent `argv` in `cwd`, feeding `input` on stdin (so a large diff
/// isn't crammed onto the command line) and capturing stdout. Used by `review` and
/// `fork` to drive an agent non-interactively.
pub fn run_capture(argv: &[String], cwd: &Path, input: &str) -> Result<String> {
    let (bin, rest) = argv.split_first().context("empty argv")?;
    let mut child = Command::new(bin)
        .args(rest)
        .current_dir(cwd)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .with_context(|| format!("failed to spawn {bin}"))?;
    child
        .stdin
        .take()
        .context("no stdin")?
        .write_all(input.as_bytes())
        .context("writing agent stdin")?;
    let out = child.wait_with_output().context("waiting for agent")?;
    if !out.status.success() {
        bail!("{bin} exited with {}", out.status);
    }
    Ok(String::from_utf8_lossy(&out.stdout).into_owned())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn claude() -> &'static Agent {
        for_command("claude").unwrap()
    }

    #[test]
    fn detect_maps_known_agents() {
        assert_eq!(detect("claude"), Some("CC"));
        assert_eq!(detect("codex"), Some("CX"));
        assert_eq!(detect("opencode"), Some("OC"));
        assert_eq!(detect("bash"), None);
        assert_eq!(detect("vim"), None);
    }

    #[test]
    fn spawn_plan_argv_pins_session_and_plan_mode_without_skip() {
        let argv = claude()
            .argv(&Launch {
                session_id: Some("uuid-1"),
                permission_mode: Some("plan"),
                skip_perms: true, // must be suppressed by permission_mode
                role: Some("PLAN ROLE"),
                prompt: Some("do the thing"),
                ..Default::default()
            })
            .unwrap();
        assert_eq!(
            argv,
            vec![
                "claude",
                "--session-id",
                "uuid-1",
                "--permission-mode",
                "plan",
                "--append-system-prompt",
                "PLAN ROLE",
                "do the thing",
            ]
        );
        assert!(!argv.iter().any(|s| s == "--dangerously-skip-permissions"));
    }

    #[test]
    fn spawn_implement_argv_uses_skip() {
        let argv = claude()
            .argv(&Launch {
                session_id: Some("uuid-2"),
                skip_perms: true,
                prompt: Some("go"),
                ..Default::default()
            })
            .unwrap();
        assert_eq!(
            argv,
            vec!["claude", "--session-id", "uuid-2", "--dangerously-skip-permissions", "go"]
        );
    }

    #[test]
    fn headless_review_argv() {
        let argv = claude()
            .argv(&Launch {
                role: Some("REVIEW ROLE"),
                headless: true,
                prompt: Some("<diff>"),
                ..Default::default()
            })
            .unwrap();
        assert_eq!(
            argv,
            vec![
                "claude",
                "--append-system-prompt",
                "REVIEW ROLE",
                "-p",
                "--output-format",
                "json",
                "<diff>",
            ]
        );
    }

    #[test]
    fn fork_argv_resumes_forks_and_reskips() {
        let argv = claude()
            .argv(&Launch {
                resume: Some("uuid-3"),
                fork: true,
                skip_perms: true,
                prompt: Some("branch off"),
                ..Default::default()
            })
            .unwrap();
        assert_eq!(
            argv,
            vec![
                "claude",
                "--resume",
                "uuid-3",
                "--fork-session",
                "--dangerously-skip-permissions",
                "branch off",
            ]
        );
    }

    #[test]
    fn undriveable_agents_return_none() {
        assert!(for_command("codex").unwrap().argv(&Launch::default()).is_none());
    }
}
