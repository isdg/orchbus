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
CURRENT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
ORCHBUS="$(command -v orchbus || echo "$HOME/.cargo/bin/orchbus")"

# Self-heal build. Rebuild when the binary is MISSING (fresh clone) or STALE —
# i.e. any source file is newer than the installed binary, which is exactly what
# `prefix U` (TPM update) produces after it pulls new source. `cargo install
# --force` reinstalls to ~/.cargo/bin so the update actually takes effect. Runs
# in the background so tmux start never blocks; bindings work once it finishes.
if [ ! -x "$ORCHBUS" ] || \
   [ -n "$(find "$CURRENT_DIR/src" "$CURRENT_DIR/Cargo.toml" -newer "$ORCHBUS" -print -quit 2>/dev/null)" ]; then
    if command -v cargo >/dev/null 2>&1; then
        tmux run-shell -b "cd '$CURRENT_DIR' && cargo install --path . --force >/dev/null 2>&1 && tmux display-message 'orchbus: (re)built — bindings ready'"
    else
        tmux display-message 'orchbus: install rust/cargo to build the binary, then reload tmux'
    fi
fi

tmux bind-key o display-popup -E -w 100% -h 100% "$ORCHBUS ui"
tmux bind-key O run-shell "$ORCHBUS open"
