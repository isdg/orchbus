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

TAB="$(printf '\t')"
CACHE="${TMPDIR:-/tmp}/orchbus.cache"

# scan_pane <pid> <swin> <title> -> the 6-field row
#   rank<TAB>pid<TAB>glyph<TAB>swin<TAB>title<TAB>question
# for one pane, or nothing if it isn't a live CC pane. (The leading rank drives
# the importance sort; it's dropped before the list reaches fzf.)
scan_pane() {
  local pid="$1" swin="$2" title="$3" txt meta rank glyph q
  txt="$(tmux capture-pane -p -t "$pid" 2>/dev/null | tail -n "$TAIL_LINES")" || return
  [ -n "$txt" ] || return
  meta="$(state_meta "$(classify "$txt")")"   # "rank glyph", e.g. "1 [!]"
  rank="${meta%% *}"; glyph="${meta#* }"      # split without word-splitting (glyphs are glob chars)
  # Prefer the on-screen question (a line ending in ?); else the CC topic
  # (pane_title). Strip tabs so the TSV stays well-formed.
  q="$(printf '%s\n' "$txt" | grep -m1 -E '\?[[:space:]]*$' \
        | sed -E 's/^[[:space:]]*//; s/[[:space:]]+/ /g' | tr -d "$TAB")"
  [ -n "$q" ] || q="$title"
  title="$(printf '%s' "$title" | tr -d "$TAB")"
  printf '%s\t%s\t%s\t%s\t%s\t%s\n' "$rank" "$pid" "$glyph" "$swin" "$title" "$q"
}

# finalize: read 6-field rows on stdin, sort by importance (rank, then pane_id),
# cache them atomically (WITH rank, so a later single-pane splice can re-sort),
# and print the 5-field list (rank stripped, pane_id first) for fzf.
finalize() {
  local sorted; sorted="$(sort -t"$TAB" -k1,1n -k2,2)"
  { [ -n "$sorted" ] && printf '%s\n' "$sorted"; } > "$CACHE.$$" && mv -f "$CACHE.$$" "$CACHE"
  [ -n "$sorted" ] && printf '%s\n' "$sorted" | cut -f2-
  return 0
}

# Every CC pane across all sessions (exited CC -> zsh/bash, filtered out; two CC
# panes in one window -> two pane_ids -> two rows).
scan_all() {
  tmux list-panes -a -F "#{pane_id}${TAB}#{pane_current_command}${TAB}#{session_name}:#{window_index}${TAB}#{pane_title}" 2>/dev/null |
  while IFS="$TAB" read -r pid cmd swin title; do
    [ "$cmd" = claude ] || continue
    scan_pane "$pid" "$swin" "$title"
  done
}

# Dispatch:
#   scan.sh --cache  -> print the LAST cached list instantly (no scanning). Used
#                       for the popup's initial paint so it opens with zero wait;
#                       the long-lived `prefix O` window keeps this cache warm via
#                       its ~1s auto-refresh. Empty if nothing has scanned yet.
#   scan.sh          -> full scan of every CC pane (on open + ~1s auto-refresh).
#   scan.sh <pane>   -> rescan ONLY that pane and splice its fresh row into the
#                       cached list, so the row you just approved/cancelled updates
#                       instantly WITHOUT rescanning all ~20 panes. Falls back to a
#                       full scan if there's no cache yet (shouldn't happen: the
#                       popup's start-event runs a full scan before any keypress).
if [ "${1:-}" = "--cache" ]; then
  [ -f "$CACHE" ] && cut -f2- "$CACHE"    # strip the rank column; print 5-field list
  exit 0
fi

if [ -n "${1:-}" ] && [ -f "$CACHE" ]; then
  pid="$1"
  info="$(tmux list-panes -a -F "#{pane_id}${TAB}#{pane_current_command}${TAB}#{session_name}:#{window_index}${TAB}#{pane_title}" 2>/dev/null \
          | awk -F"$TAB" -v p="$pid" '$1==p{print; exit}')"
  IFS="$TAB" read -r xpid cmd swin title <<EOF
$info
EOF
  newrow=""
  [ "$cmd" = claude ] && newrow="$(scan_pane "$pid" "$swin" "$title")"
  # Cached rows for every OTHER pane (drop this pid's line: "rank<TAB>pid<TAB>…"),
  # plus this pane's fresh row if it's still a CC pane (gone -> the row disappears).
  {
    grep -vE "^[0-9]+${TAB}${pid}${TAB}" "$CACHE" 2>/dev/null
    [ -n "$newrow" ] && printf '%s\n' "$newrow"
  } | finalize
else
  scan_all | finalize
fi
