#!/usr/bin/env bash
# orchbus — tmux plugin entry point.
#
# Registers `prefix + o` to open the cockpit: a popup listing every Claude Code
# session across all panes, from which you triage and approve their prompts
# without tabbing between windows. See scripts/orchbus.sh for the keys.
#
# Load it from ~/.tmux.conf with:
#     run-shell '~/.dotfiles/tmux/orchbus/orchbus.tmux'
CURRENT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"

tmux bind-key o display-popup -E -w 100% -h 100% "$CURRENT_DIR/scripts/orchbus.sh"
