//! `orchbus claude [ARGS…]` — run Claude Code natively, with every one of its own
//! flags, but wrapped in orchbus's isolation and bookkeeping.
//!
//! The tag profiles (`plan`/`implement`/`review`) are opinionated on purpose: they
//! decide the permission mode, the role prompt, headless-vs-pane. This verb is the
//! escape hatch for when you want to drive `claude` yourself — `--model`, `--agent`,
//! `--effort`, `-p`, anything — and still get the worktree, the tracked slug, and a
//! window the cockpit can see (so `list`/`status`/`wait`/`review`/`fork` keep working).
//!
//! Everything after the verb is forwarded verbatim. orchbus injects only two things,
//! and only when the caller hasn't spoken for them:
//!   * `--session-id <uuid>` — unless the args already manage a session
//!     (`--resume`/`--continue`/`--session-id`), so `fork`/`revise` stay deterministic.
//!   * `--dangerously-skip-permissions` — safe because the worktree is isolated;
//!     suppressed by `--no-skip` or by any permission flag of your own.

use crate::spawn;
use anyhow::{Context, Result};
use orchbus_agent::agent;

/// Flags that mean "I'm managing the session myself" — orchbus must not also pin a
/// fresh `--session-id` (claude rejects the conflicting pair).
const SESSION_FLAGS: &[&str] =
    &["--session-id", "--resume", "-r", "--continue", "-c", "--fork-session", "--from-pr"];

/// Flags that occupy the same knob as `--dangerously-skip-permissions`.
const PERM_FLAGS: &[&str] = &[
    "--permission-mode",
    "--dangerously-skip-permissions",
    "--allow-dangerously-skip-permissions",
];

/// Spawn `claude` in an isolated worktree with `args` passed through untouched.
/// Returns the assigned slug.
pub fn run(args: &[String], slug: Option<&str>, branch: Option<&str>, no_skip: bool) -> Result<String> {
    let ag = agent::for_command("claude").context("claude is not a known agent")?;

    // Pin a session so fork/revise can find this agent later. When the caller drives
    // the session themselves we record the id they named (best effort — `--resume`
    // with no value opens claude's picker, and there's nothing to record yet).
    let owns_session = has_flag(args, SESSION_FLAGS);
    let generated = uuid::Uuid::new_v4().to_string();
    let session_id = if owns_session {
        SESSION_FLAGS
            .iter()
            .find_map(|f| flag_value(args, f))
            .unwrap_or(&generated)
            .to_string()
    } else {
        generated.clone()
    };

    let argv = ag
        .argv(&agent::Launch {
            session_id: (!owns_session).then_some(&session_id),
            skip_perms: !no_skip && !has_flag(args, PERM_FLAGS),
            extra: args,
            ..Default::default()
        })
        .context("orchbus can't drive agent 'claude' yet")?;

    let base = slug.map(str::to_string).unwrap_or_else(|| slug_base(args));
    spawn::open(&base, "claude", "claude", branch, &session_id, argv)
}

/// Whether any of `flags` appears in `args`, in either `--flag value` or
/// `--flag=value` form.
fn has_flag(args: &[String], flags: &[&str]) -> bool {
    args.iter().any(|a| flags.iter().any(|f| a == f || a.starts_with(&format!("{f}="))))
}

/// The value given to `flag`, from `--flag=value` or the token after `--flag`.
/// `None` when the flag is absent or its value is missing/another flag.
fn flag_value<'a>(args: &'a [String], flag: &str) -> Option<&'a str> {
    let eq = format!("{flag}=");
    for (i, a) in args.iter().enumerate() {
        if let Some(v) = a.strip_prefix(&eq) {
            return Some(v);
        }
        if a == flag {
            return args.get(i + 1).map(String::as_str).filter(|v| !v.starts_with('-'));
        }
    }
    None
}

/// Name the window/branch after the work. claude's prompt is a trailing positional,
/// so the last argument is it — when it reads like prose rather than a flag value.
/// Otherwise fall back to `claude` (uniquified to `claude-2`, `claude-3`, … by
/// `spawn::open`). `--slug` overrides this entirely.
fn slug_base(args: &[String]) -> String {
    args.last()
        .filter(|a| !a.starts_with('-') && a.contains(' '))
        .map(|a| spawn::slugify(a))
        .unwrap_or_else(|| "claude".to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn v(args: &[&str]) -> Vec<String> {
        args.iter().map(|s| s.to_string()).collect()
    }

    /// The whole point: the user's flags survive verbatim, after orchbus's own.
    fn argv_for(args: &[&str], no_skip: bool) -> Vec<String> {
        let args = v(args);
        let owns = has_flag(&args, SESSION_FLAGS);
        agent::for_command("claude")
            .unwrap()
            .argv(&agent::Launch {
                session_id: (!owns).then_some("sid"),
                skip_perms: !no_skip && !has_flag(&args, PERM_FLAGS),
                extra: &args,
                ..Default::default()
            })
            .unwrap()
    }

    #[test]
    fn forwards_flags_after_injecting_session_and_skip() {
        assert_eq!(
            argv_for(&["--model", "opus", "-p", "fix the flaky test"], false),
            v(&[
                "claude",
                "--session-id",
                "sid",
                "--dangerously-skip-permissions",
                "--model",
                "opus",
                "-p",
                "fix the flaky test",
            ])
        );
    }

    #[test]
    fn no_args_still_launches_a_tracked_claude() {
        assert_eq!(
            argv_for(&[], false),
            v(&["claude", "--session-id", "sid", "--dangerously-skip-permissions"])
        );
    }

    #[test]
    fn own_permission_flag_suppresses_the_injected_skip() {
        for own in [&["--permission-mode", "plan"][..], &["--dangerously-skip-permissions"][..]] {
            let argv = argv_for(own, false);
            assert_eq!(
                argv.iter().filter(|s| *s == "--dangerously-skip-permissions").count(),
                usize::from(own[0] == "--dangerously-skip-permissions"),
                "orchbus must not add a second permission flag for {own:?}",
            );
        }
    }

    #[test]
    fn no_skip_opts_out() {
        assert!(!argv_for(&["-p", "go"], true).contains(&"--dangerously-skip-permissions".into()));
    }

    #[test]
    fn own_session_flag_suppresses_the_injected_id() {
        let argv = argv_for(&["--resume", "abc-123"], false);
        assert_eq!(argv.iter().filter(|s| *s == "--session-id").count(), 0);
        assert!(argv.contains(&"--resume".to_string()));
    }

    #[test]
    fn flag_value_reads_both_spellings() {
        assert_eq!(flag_value(&v(&["--resume", "abc"]), "--resume"), Some("abc"));
        assert_eq!(flag_value(&v(&["--resume=abc"]), "--resume"), Some("abc"));
        // No value: bare `--resume` (claude's picker), or followed by another flag.
        assert_eq!(flag_value(&v(&["--resume"]), "--resume"), None);
        assert_eq!(flag_value(&v(&["--resume", "-p"]), "--resume"), None);
        assert_eq!(flag_value(&v(&["--model", "opus"]), "--resume"), None);
    }

    #[test]
    fn slug_comes_from_the_trailing_prompt_not_a_flag_value() {
        assert_eq!(slug_base(&v(&["-p", "Fix the FLAKY test"])), "fix-the-flaky-test");
        // A lone flag value isn't prose — don't name the branch `opus`.
        assert_eq!(slug_base(&v(&["--model", "opus"])), "claude");
        assert_eq!(slug_base(&v(&[])), "claude");
    }
}
