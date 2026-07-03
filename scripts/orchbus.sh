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
# Requires fzf >= 0.36 (start/load events); you have 0.70.
set -uo pipefail

DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
SCAN="$DIR/scan.sh"
GUARD="$DIR/guard-approve.sh"
MODE="${1:-}"   # --fresh: full scan on init (the `prefix O` window); else cached init (the popup)

if ! command -v fzf >/dev/null 2>&1; then
  printf 'orchbus: fzf not found on PATH. Install fzf (>= 0.36).\n' >&2
  sleep 2; exit 1
fi

# Initial list source: the ephemeral popup paints instantly from the cache; the
# long-lived window scans fresh so its opening view is guaranteed current (no
# stale-cache question), at the cost of ~one scan (~0.7s) before it draws.
init_list() { if [ "$MODE" = "--fresh" ]; then "$SCAN"; else "$SCAN" --cache; fi; }

# Field 1 (pane_id) is hidden from matching (--with-nth=2..) but stays available
# as {1} in every bind/preview. Parse-clean text comes from scan.sh; the preview
# uses -ep to keep color.
#
# We deliberately do NOT bind `start:reload` — that fires at startup and makes fzf
# DISCARD the piped initial list to re-read from a full scan, blocking the first
# paint. Instead the refresh is driven entirely by the load->sleep->reload self-loop:
# `load` fires once the initial list is read, then each finished reload re-fires
# `load`, so a fresh full scan swaps in ~1s after open and every ~1s after. Plain
# `reload` (async) NOT `reload-sync` — sync would freeze the UI (block all input)
# for the whole sleep+scan (~1.7s) every cycle.
init_list | fzf \
  --reverse \
  --delimiter='\t' \
  --with-nth=2.. \
  --prompt='orchbus> ' \
  --header='ctrl-a approve · ctrl-x cancel · ctrl-r refresh · enter jump' \
  --preview 'tmux capture-pane -ep -t {1} | tail -n "${FZF_PREVIEW_LINES:-40}"' \
  --preview-window='down,70%' \
  --preview-label=' pane ' \
  --bind "load:reload(sleep 1; $SCAN)" \
  --bind "ctrl-r:reload($SCAN)" \
  --bind "ctrl-a:execute-silent($GUARD {1} enter)+reload($SCAN {1})" \
  --bind "ctrl-x:execute-silent(tmux send-keys -t {1} Escape)+reload($SCAN {1})" \
  --bind "enter:execute-silent(tmux switch-client -t {1}; tmux select-window -t {1}; tmux select-pane -t {1})+abort"
