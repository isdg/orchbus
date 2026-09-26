//! `orchbus wait <slug>` — block until a spawned agent settles, so a driving
//! session can sequence `spawn → wait → review` without hand-rolling a poll loop.
//!
//! It reuses `scan::window_state` (the same per-slug state `orchbus status <slug>`
//! reports) on a ~1s cadence — the cockpit's refresh rhythm — and returns as soon
//! as the agent reaches a settled state (or the one named by `--for`). A vanished
//! window or an elapsed timeout is an error (non-zero exit), so scripts can tell
//! "the agent is ready" from "it died / never got there".

use crate::classify::{label, State};
use crate::{scan, state};
use anyhow::{bail, Result};
use std::time::{Duration, Instant};

/// Poll interval — matches the cockpit's ~1s auto-refresh.
const TICK: Duration = Duration::from_secs(1);

/// Should `wait` return now? With an explicit `target`, only that exact state
/// satisfies; otherwise any *settled* state does — i.e. the agent has stopped
/// churning (not `Running`) and shows something classifiable (not `Unknown`).
pub(crate) fn settled(state: State, target: Option<State>) -> bool {
    match target {
        Some(t) => state == t,
        None => !matches!(state, State::Running | State::Unknown),
    }
}

/// Block until the spawned agent `slug` settles (or reaches `target`), printing the
/// state it settled into. Errors if the window is gone or `timeout_secs` elapses.
pub fn run(slug: &str, target: Option<State>, timeout_secs: u64) -> Result<()> {
    state::get(slug)?; // validate it's a known spawn before we start looping
    let deadline = Instant::now() + Duration::from_secs(timeout_secs);
    loop {
        match scan::window_state(slug)? {
            None => bail!("'{slug}' pane is gone — nothing to wait for"),
            Some((_, st)) if settled(st, target) => {
                println!("{}", label(st));
                return Ok(());
            }
            Some(_) => {}
        }
        if Instant::now() >= deadline {
            bail!("timed out after {timeout_secs}s waiting for '{slug}'");
        }
        std::thread::sleep(TICK);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn settled_without_target_waits_through_running_and_unknown() {
        assert!(!settled(State::Running, None));
        assert!(!settled(State::Unknown, None));
        // Any state that wants attention or means "done" ends the wait.
        assert!(settled(State::Idle, None));
        assert!(settled(State::Approve, None));
        assert!(settled(State::Input, None));
    }

    #[test]
    fn settled_with_target_requires_exact_state() {
        assert!(settled(State::Approve, Some(State::Approve)));
        // A different settled state does NOT satisfy a specific target — e.g. waiting
        // for the plan's approval menu shouldn't return on a transient idle.
        assert!(!settled(State::Idle, Some(State::Approve)));
        assert!(!settled(State::Running, Some(State::Approve)));
    }
}
