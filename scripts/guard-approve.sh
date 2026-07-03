#!/usr/bin/env bash
# orchbus/guard-approve.sh <pane_id> <enter|1-9>
#
# Re-captures the target pane and only sends the key if an approval menu is
# STILL showing — closing the race between orchbus's periodic scan and the
# user's keypress (a prompt may have closed in between). If the menu is gone,
# this is a no-op, so a stray Enter/digit never lands in an idle prompt, a
# running turn, the session-rating (⚪), or an interrupted (🟠) pane — those
# have no `❯ N.` menu and thus fail the guard by construction.
set -uo pipefail

# Must match scan.sh's approval signal: the structural highlighted menu ONLY,
# so a prose "…proceed?" question (no menu) never receives a keystroke.
RE_APPROVE_MENU='^[[:space:]]*❯[[:space:]]*[0-9]+\.'

pid="${1:-}"; key="${2:-enter}"
[ -n "$pid" ] || exit 0

txt="$(tmux capture-pane -p -t "$pid" 2>/dev/null | tail -n 25)" || exit 0
printf '%s' "$txt" | grep -qE "$RE_APPROVE_MENU" || exit 0   # menu gone -> do nothing

case "$key" in
  enter) tmux send-keys -t "$pid" Enter ;;
  # Digit picks: send the option number literally, then confirm with Enter.
  # Separate send-keys calls (tmux flushes between them) avoid the debounce race
  # where a combined "N Enter" selects before the digit registers.
  [1-9]) tmux send-keys -t "$pid" -l "$key"; tmux send-keys -t "$pid" Enter ;;
  *)     exit 0 ;;
esac
