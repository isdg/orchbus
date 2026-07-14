#!/usr/bin/env bash
# orchbus — tmux plugin entry point.
#
# A cockpit listing every Claude Code session across all panes so you can triage
# and approve their prompts without tabbing between windows, served by the
# `orchbus` Rust binary (see src/). Two ways to open it:
#     prefix o  -> as a display-popup (ephemeral overlay)   (orchbus ui)
#     prefix O  -> as a real tmux window (persists)          (orchbus open)
#
# Install via TPM (~/.tmux.conf):
#     set -g @plugin 'isdg/orchbus'
# The binary is built by bootstrap (`cargo install --path .`); resolved from
# PATH, falling back to the crate's release build for a dev checkout.
CURRENT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
ORCHBUS="$(command -v orchbus || echo "$CURRENT_DIR/target/release/orchbus")"

# Self-heal: on a fresh TPM clone the binary won't exist yet. Build it once in
# the background so `set -g @plugin 'isdg/orchbus'` works without extra steps.
# (bootstrap also builds it deterministically.)
if [ ! -x "$ORCHBUS" ]; then
    if command -v cargo >/dev/null 2>&1; then
        tmux run-shell -b "cd '$CURRENT_DIR' && cargo build --release >/dev/null 2>&1 && tmux display-message 'orchbus: built — bindings ready'"
    else
        tmux display-message 'orchbus: install rust/cargo to build the binary, then reload tmux'
    fi
fi

tmux bind-key o display-popup -E -w 100% -h 100% "$ORCHBUS ui"
tmux bind-key O run-shell "$ORCHBUS open"
