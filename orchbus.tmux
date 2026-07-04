#!/usr/bin/env bash
# orchbus — tmux plugin entry point.
#
# The cockpit lists every Claude Code session across all panes so you can triage
# and approve their prompts without tabbing between windows. See scripts/orchbus.sh
# for the keys. Two ways to open it:
#     prefix o  -> as a display-popup (ephemeral overlay)
#     prefix O  -> as a real tmux window (persists, shows in the window list)
#
# Install via TPM (~/.tmux.conf):
#     set -g @plugin 'isdf/orchbus'
# or load directly:
#     run-shell '~/.tmux/plugins/orchbus/orchbus.tmux'
CURRENT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"

tmux bind-key o display-popup -E -w 100% -h 100% "$CURRENT_DIR/scripts/orchbus.sh"
tmux bind-key O run-shell "$CURRENT_DIR/scripts/open.sh"
