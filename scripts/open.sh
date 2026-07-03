#!/usr/bin/env bash
# orchbus/open.sh — open the cockpit as a real tmux window, reusing it if it's
# already open (bound to prefix O).
#
# If an `orchbus` window exists in any session, switch the client to it instead
# of spawning a duplicate; otherwise create it in the current session. The window
# closes when orchbus.sh exits (enter=jump / esc) — default tmux behaviour.
set -uo pipefail
DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"

existing="$(tmux list-windows -a -F '#{window_name} #{session_name}:#{window_index}' 2>/dev/null \
            | awk '$1=="orchbus"{print $2; exit}')"

if [ -n "$existing" ]; then
  tmux switch-client -t "$existing"      # session:index -> focuses that window even across sessions
else
  tmux new-window -n orchbus "$DIR/orchbus.sh --fresh"
fi
