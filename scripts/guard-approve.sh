#!/usr/bin/env bash
# orchbus/guard-approve.sh <pane_id> <enter|1-9|dismiss>
#
# Re-captures the target pane and only sends the key if the EXPECTED prompt is
# STILL showing — closing the race between orchbus's periodic scan and the
# user's keypress (a prompt may have closed in between). Each mode requires its
# own prompt, so a stray key never lands on the wrong thing:
#   enter / 1-9  require the approval menu ( ❯ N. )  -> approve / pick option
#   dismiss      requires the rating prompt          -> send 0 (Dismiss)
# Idle/running/interrupted panes match neither and are no-ops by construction.
set -uo pipefail

# Must match scan.sh's PATTERN TABLE.
RE_APPROVE_MENU='^[[:space:]]*❯[[:space:]]*[0-9]+\.'
RE_RATING='How is Claude doing this session'

pid="${1:-}"; key="${2:-enter}"
[ -n "$pid" ] || exit 0

txt="$(tmux capture-pane -p -t "$pid" 2>/dev/null | tail -n 25)" || exit 0

case "$key" in
  dismiss)
    printf '%s' "$txt" | grep -qE "$RE_RATING" || exit 0   # not the rating prompt -> do nothing
    tmux send-keys -t "$pid" -l 0 ;;                        # "0: Dismiss" (immediate selector, no Enter)
  enter)
    printf '%s' "$txt" | grep -qE "$RE_APPROVE_MENU" || exit 0
    tmux send-keys -t "$pid" Enter ;;
  # Digit picks: send the option number literally, then confirm with Enter.
  # Separate send-keys calls (tmux flushes between them) avoid the debounce race
  # where a combined "N Enter" selects before the digit registers.
  [1-9])
    printf '%s' "$txt" | grep -qE "$RE_APPROVE_MENU" || exit 0
    tmux send-keys -t "$pid" -l "$key"; tmux send-keys -t "$pid" Enter ;;
  *) exit 0 ;;
esac
