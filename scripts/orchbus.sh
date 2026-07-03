#!/usr/bin/env bash
# orchbus/orchbus.sh — the cockpit UI (body of `prefix + o`'s display-popup).
#
# An fzf list of every Claude Code pane (from scan.sh), one row = one session:
#   <glyph> <session:win> <topic> <question>
# The preview shows that pane's live, colored contents. Act on the highlighted
# session without leaving the popup; the list auto-refreshes ~1s so glyphs stay
# current while you work the queue.
#
#   ctrl-a  approve (accept the highlighted default "Yes" — safe primary action)
#   ctrl-x  cancel the prompt (Esc)
#   ctrl-r  refresh now
#   enter   jump to that pane (closes the popup)
#
# Requires fzf >= 0.36 (start/load events, reload-sync); you have 0.70.
set -uo pipefail

DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
SCAN="$DIR/scan.sh"
GUARD="$DIR/guard-approve.sh"

if ! command -v fzf >/dev/null 2>&1; then
  printf 'orchbus: fzf not found on PATH. Install fzf (>= 0.36).\n' >&2
  sleep 2; exit 1
fi

# Field 1 (pane_id) is hidden from matching (--with-nth=2..) but stays available
# as {1} in every bind/preview. Parse-clean text comes from scan.sh; the preview
# uses -ep to keep color. The load->sleep->reload self-loop is the ~1s poll: each
# finished reload re-fires `load`, so scans pace themselves and never stack.
exec fzf \
  --reverse \
  --delimiter='\t' \
  --with-nth=2.. \
  --prompt='orchbus> ' \
  --header='ctrl-a approve · ctrl-x cancel · ctrl-r refresh · enter jump' \
  --preview 'tmux capture-pane -ep -t {1} | tail -n "${FZF_PREVIEW_LINES:-40}"' \
  --preview-window='down,70%' \
  --preview-label=' pane ' \
  --bind "start:reload($SCAN)" \
  --bind "load:reload-sync(sleep 1; $SCAN)" \
  --bind "ctrl-r:reload($SCAN)" \
  --bind "ctrl-a:execute-silent($GUARD {1} enter)+reload($SCAN)" \
  --bind "ctrl-x:execute-silent(tmux send-keys -t {1} Escape)+reload($SCAN)" \
  --bind "enter:execute-silent(tmux switch-client -t {1}; tmux select-window -t {1}; tmux select-pane -t {1})+abort"
