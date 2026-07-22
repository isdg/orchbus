# orchbus — instructions for Claude Code

This repo builds **orchbus**, a tmux cockpit for driving many Claude Code
sessions. It also lets *you* orchestrate sub-work **visibly**.

## Prefer visible orchestration over native subagents

When a task in this repo would benefit from delegating to a sub-agent (plan a
change, implement an isolated piece, review a diff), and the user wants to
**watch or approve** that work, drive it through `orchbus` instead of the native
`Agent`/`Task` tools. The difference is visibility: native subagents run
off-screen, but every `orchbus spawn` opens a **real tmux window** the user can
see in the cockpit (`prefix o`), read live, and approve prompts in.

Use the native `Agent`/`Explore` tools as usual for your own read-only research
and quick fan-out searches — those don't need to be watched. Reserve the orchbus
loop for **delegated work the user asked to supervise**.

## The loop

Build first if needed: `cargo build` (the binary is `target/debug/orchbus`, or
just `cargo run --`). Then, from your Bash tool:

```sh
# 1. Spawn a sub-agent in an isolated worktree + visible window.
#    Tags: plan (plan mode) · implement (does the work) · review (headless).
orchbus spawn "implement <subtask>" --tag implement
#    → spawned '<slug>' …   (note the slug it prints)

# 2. Block until that agent settles, then act. --for approve for a plan gate.
orchbus wait <slug>                 # returns when it's done / needs attention
orchbus status <slug> --json        # or poll state yourself: running|idle|approve|input|gone

# 3. Review its diff against the captured plan (fresh, read-only reviewer).
orchbus capture <slug>              # pull the plan from plan-mode agents first
orchbus review <slug>               # findings → .orchbus/reviews/<slug>.md

# 4. Feed the review back for a fix, or fork a divergent attempt.
orchbus revise <slug>
orchbus fork   <slug> --prompt "try another approach"
```

Notes:
- `wait` exits non-zero if the window is **gone** or it **times out**
  (`--timeout <secs>`, default 600) — check the exit code before reviewing.
- `status <slug>` / `wait <slug>` only work for agents **orchbus spawned** (they
  key off the recorded slug in `.orchbus/state.json`).
- Spawned `implement` agents run with `--dangerously-skip-permissions` because
  they're in a throwaway worktree; pass `spawn --no-skip` to opt out.

## Working on orchbus itself

- Rust; `cargo test` and `cargo build` must stay green with **no warnings**.
- All TUI-reading regexes live in `src/classify.rs` (the PATTERN TABLE) — the
  scanner and the approve guard share them; fix classification there.
- `src/scan.rs` enumerates panes; `src/spawn.rs`/`review.rs`/`revise.rs`/
  `fork.rs`/`wait.rs` are the loop verbs; `src/state.rs` is the per-slug store.
