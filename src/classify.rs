//! PATTERN TABLE — the single source of truth for reading Claude Code's TUI.
//!
//! Everything version-fragile about the CC terminal UI lives here. `scan` uses
//! `classify` to pick a per-pane state; `approve` uses `shows_approve_menu` to
//! guard a keypress. Both go through this module, so the approve-menu pattern is
//! defined once (it used to be copy-pasted into scan.sh and guard-approve.sh
//! with a "must match" comment).
//!
//! Rust regexes on &str match the multibyte glyphs (❯, ·) directly — no BSD-grep
//! byte-sequence workarounds needed.

use regex::Regex;
use std::sync::LazyLock;

/// Optional "rate this session" prompt — checked first so it's never mistaken
/// for an approvable menu.
static RATING: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"How is Claude doing this session").unwrap());

/// The SOLE approve signal: a highlighted numbered menu (`❯ 1.`). Structural, so
/// chatty prose like "…proceed?" can't be mistaken for an approvable menu.
/// `(?m)` so `^` anchors to any line within the captured block.
static APPROVE_MENU: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?m)^[[:space:]]*❯[[:space:]]*[0-9]+\.").unwrap());

/// Interrupted / needs a written reply.
static INPUT: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"Interrupted|What should Claude do").unwrap());

/// Live elapsed timer "(16s · " / "(1m 3s · " — the trusted RUNNING signal
/// (needs the middle-dot, so decorative sparkles in FINISHED output don't match).
static RUN_TIMER: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"\([0-9].*s ·").unwrap());

/// Interrupt hint, case-insensitive — the other RUNNING signal.
static RUN_INTR: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?i)esc to interrupt").unwrap());

/// Bare input caret (idle).
static PROMPT: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?m)^[[:space:]]*❯[[:space:]]").unwrap());

#[derive(Copy, Clone, PartialEq, Eq, Debug)]
pub enum State {
    Approve,
    Input,
    Running,
    Idle,
    Rating,
    Unknown,
}

/// Classify a pane's recent on-screen text. Order matters (first match wins):
/// rating before approve, the structural ❯N. menu before prose, the elapsed
/// timer as the trusted RUNNING signal.
pub fn classify(text: &str) -> State {
    if RATING.is_match(text) {
        State::Rating
    } else if APPROVE_MENU.is_match(text) {
        State::Approve
    } else if INPUT.is_match(text) {
        State::Input
    } else if RUN_TIMER.is_match(text) || RUN_INTR.is_match(text) {
        State::Running
    } else if PROMPT.is_match(text) {
        State::Idle
    } else {
        State::Unknown
    }
}

/// `(rank, glyph)` for a state. Rank orders the cockpit list by how much it wants
/// your attention (1 = top). Glyphs are fixed 3-wide so the column aligns.
pub fn meta(state: State) -> (u8, &'static str) {
    match state {
        State::Approve => (1, "[!]"), // act now
        State::Input => (2, "[?]"),   // needs a reply
        State::Running => (3, "[*]"), // running
        State::Idle => (4, "[=]"),    // idle
        State::Rating => (5, "[o]"),  // optional
        State::Unknown => (6, "[.]"), // unknown
    }
}

/// Is the approve menu (`❯ N.`) currently showing? Used by `approve` to close the
/// race between orchbus's periodic scan and the user's keypress.
pub fn shows_approve_menu(text: &str) -> bool {
    APPROVE_MENU.is_match(text)
}

/// Lowercase word for a state — the human/JSON label (the glyph's plain-text twin).
pub fn label(state: State) -> &'static str {
    match state {
        State::Approve => "approve",
        State::Input => "input",
        State::Running => "running",
        State::Idle => "idle",
        State::Rating => "rating",
        State::Unknown => "unknown",
    }
}

/// Inverse of `meta`'s rank: recover the state from a cached rank so a `Row` read
/// back from the cache (which stores rank, not the enum) can report its state.
pub fn state_from_rank(rank: u8) -> State {
    match rank {
        1 => State::Approve,
        2 => State::Input,
        3 => State::Running,
        4 => State::Idle,
        5 => State::Rating,
        _ => State::Unknown,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn approve_menu_beats_prose_question() {
        let t = "Do you want to proceed?\n  ❯ 1. Yes\n    2. No";
        assert_eq!(classify(t), State::Approve);
        assert!(shows_approve_menu(t));
    }

    #[test]
    fn rating_checked_before_approve() {
        let t = "How is Claude doing this session?\n ❯ 1. Good";
        assert_eq!(classify(t), State::Rating);
    }

    #[test]
    fn running_needs_timer_or_interrupt_not_sparkle() {
        assert_eq!(classify("✻ Crunching (16s · esc to interrupt)"), State::Running);
        assert_eq!(classify("esc to interrupt"), State::Running);
        // decorative sparkle in finished output must NOT read as running
        assert_eq!(classify("✻ Crunched for 28s"), State::Unknown);
    }

    #[test]
    fn idle_caret_vs_menu() {
        assert_eq!(classify("  ❯ "), State::Idle);
        assert!(!shows_approve_menu("  ❯ ")); // bare caret is not a menu
    }

    #[test]
    fn input_state() {
        assert_eq!(classify("Interrupted by user"), State::Input);
        assert_eq!(classify("What should Claude do instead?"), State::Input);
    }
}
