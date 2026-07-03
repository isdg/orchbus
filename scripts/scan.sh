#!/usr/bin/env bash
# orchbus/scan.sh — emit one TSV row per Claude Code (CC) pane for the cockpit.
#
#   Row:  pane_id <TAB> glyph <TAB> session:win <TAB> topic <TAB> question
#
# Field 1 (pane_id, e.g. %23) is the SOLE tmux target the UI uses for
# send-keys / capture / jump — it's globally unique and resolves its own
# window+session. Fields 2.. are display only; orchbus.sh hides field 1 from
# fzf matching with --with-nth=2..
#
# Pure screen-scrape: reads each pane with `capture-pane -p` (ANSI stripped)
# and classifies by what the CC TUI is currently showing. Every version-fragile
# string/glyph lives in the PATTERN TABLE below — edit there when the CC TUI
# changes. Written for macOS system bash 3.2 + BSD grep (no multibyte bracket
# classes: the spinner set is a literal alternation, matched as byte sequences).
set -uo pipefail

# ── PATTERN TABLE (edit here when Claude Code's TUI changes) ─────────────────
# Approval is decided ONLY by the structural highlighted-menu marker (❯ N.) —
# never by prose, so a chatty "…proceed?" question can't be mistaken for an
# approvable menu. RUNNING is decided ONLY by the live parenthesized timer
# "(16s · …" or the interrupt hint — never by decorative sparkle glyphs, which
# also appear in FINISHED output ("✻ Crunched for 28s") and would false-positive.
RE_RATING='How is Claude doing this session'            # optional rating prompt
RE_APPROVE_MENU='^[[:space:]]*❯[[:space:]]*[0-9]+\.'    # highlighted numbered menu (the sole approve signal)
RE_INPUT='Interrupted|What should Claude do'            # interrupted / needs a written reply
RE_RUN_TIMER='\([0-9].*s ·'                             # live elapsed timer "(16s · " / "(1m 3s · " (needs the middle-dot)
RE_RUN_INTR='esc to interrupt'
RE_PROMPT='^[[:space:]]*❯[[:space:]]'                   # bare input caret (idle)

# ASCII status tags (fixed 3-wide so the column aligns in any terminal/font):
#   [!] act now  ·  [?] needs a reply  ·  [*] running  ·  [=] idle  ·  [o] optional  ·  [.] unknown
GLYPH_APPROVE='[!]'; GLYPH_INPUT='[?]'; GLYPH_RUN='[*]'
GLYPH_IDLE='[=]';    GLYPH_RATE='[o]';  GLYPH_UNKNOWN='[.]'
TAIL_LINES=25
# ────────────────────────────────────────────────────────────────────────────

# classify <captured-text> -> prints one STATE name. Order matters (first match
# wins): rating is checked before approval so the optional prompt is never
# mistaken for an approvable menu; the structural ❯N. menu beats prose; the
# elapsed timer is the trusted RUNNING signal.
classify() {
  local t="$1"
  if printf '%s' "$t" | grep -qE "$RE_RATING";      then echo RATING;  return; fi
  if printf '%s' "$t" | grep -qE "$RE_APPROVE_MENU"; then echo APPROVE; return; fi
  if printf '%s' "$t" | grep -qE "$RE_INPUT";        then echo INPUT;   return; fi
  if printf '%s' "$t" | grep -qE  "$RE_RUN_TIMER" \
  || printf '%s' "$t" | grep -qiE "$RE_RUN_INTR";   then echo RUNNING; return; fi
  if printf '%s' "$t" | grep -qE "$RE_PROMPT";       then echo IDLE;    return; fi
  echo UNKNOWN
}

# state -> "rank glyph". Rank orders the list by how much it wants YOUR attention
# (1 = top): approvals, then input blocks, then running, then idle, then the
# optional rating, then unknown. Edit to taste.
state_meta() {
  case "$1" in
    APPROVE) echo "1 $GLYPH_APPROVE" ;;
    INPUT)   echo "2 $GLYPH_INPUT"   ;;
    RUNNING) echo "3 $GLYPH_RUN"     ;;
    IDLE)    echo "4 $GLYPH_IDLE"    ;;
    RATING)  echo "5 $GLYPH_RATE"    ;;
    *)       echo "6 $GLYPH_UNKNOWN" ;;
  esac
}

# One row per pane whose foreground command is `claude`. An exited CC (shell now
# showing) reports zsh/bash and is filtered out; two CC panes in one window get
# distinct pane_ids => two rows. TAB-delimited enumeration; IFS split on tab.
TAB="$(printf '\t')"
tmux list-panes -a -F "#{pane_id}${TAB}#{pane_current_command}${TAB}#{session_name}:#{window_index}${TAB}#{pane_title}" 2>/dev/null |
while IFS="$TAB" read -r pid cmd swin title; do
  [ "$cmd" = claude ] || continue
  txt="$(tmux capture-pane -p -t "$pid" 2>/dev/null | tail -n "$TAIL_LINES")" || continue
  [ -n "$txt" ] || continue

  meta="$(state_meta "$(classify "$txt")")"   # "rank glyph", e.g. "1 [!]"
  rank="${meta%% *}"; glyph="${meta#* }"      # split without word-splitting (glyphs are glob chars)

  # Prefer the on-screen question (a line ending in ?); fall back to the CC
  # conversation topic (pane_title). Strip tabs so the TSV stays well-formed.
  q="$(printf '%s\n' "$txt" | grep -m1 -E '\?[[:space:]]*$' \
        | sed -E 's/^[[:space:]]*//; s/[[:space:]]+/ /g' | tr -d "$TAB")"
  [ -n "$q" ] || q="$title"

  title="$(printf '%s' "$title" | tr -d "$TAB")"
  # Leading rank column drives the sort, then is stripped so field 1 stays pane_id.
  printf '%s\t%s\t%s\t%s\t%s\t%s\n' "$rank" "$pid" "$glyph" "$swin" "$title" "$q"
done | sort -t"$TAB" -k1,1n -k2,2 | cut -f2-
# ^ sort by importance rank (numeric), pane_id as a stable within-tier tiebreaker,
#   then drop the rank column. Most-actionable sessions land at the top.
