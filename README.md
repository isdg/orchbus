# orchbus

A tmux cockpit for driving many **Claude Code** sessions at once. When you run a
dozen CC sessions across tmux panes, the bottleneck is *you* tabbing around to
find which agent is blocked. orchbus scans every pane, shows all the CC sessions
in one popup with their current state and pending question, and lets you
approve / respond to each **from that one window** — no tabbing.

Pure tmux: it *reads* panes with `capture-pane` and *acts* on them with
`send-keys`. No Claude Code hooks, config, or plugins required.

## Install

Add one line to `~/.tmux.conf` (this repo already wires it in):

```tmux
run-shell '~/.dotfiles/tmux/orchbus/orchbus.tmux'
```

Reload: `tmux source-file ~/.tmux.conf`. Requires **fzf ≥ 0.36** (`start`/`load`
events, `reload-sync`) and tmux 3.2+.

## Use — `prefix + o`

Opens the cockpit popup. One row per Claude Code pane:

```
[!]  stih:1    scribe.yaml refactor    Do you want to make this edit to scribe.yaml?
[?]  plc:1     plc daily wrapper       Interrupted · What should Claude do instead?
[*]  skazka:1  image alignment         running
[=]  oda:1     flowLine node           (waiting)
[o]  cosmos:2  ...                      How is Claude doing this session? (optional)
```

| Tag | State | Meaning |
|---|---|---|
| `[!]` | NEEDS_APPROVAL | blocked on an edit/plan/tool prompt — **approvable** |
| `[?]` | NEEDS_INPUT | interrupted / needs a written reply — jump to it |
| `[*]` | RUNNING | actively working |
| `[=]` | IDLE | prompt box, ball's in your court |
| `[o]` | rating | the optional "How is Claude doing?" prompt — never auto-approved |

### Sort order

Rows are sorted by **importance** — the sessions that most want your attention
float to the top:

```
[!] approve  >  [?] input  >  [*] running  >  [=] idle  >  [o] rating  >  [.] unknown
```

Within a tier, rows are ordered by `pane_id` so the list is deterministic across
the ~1s auto-refresh. The ranking lives in one place — the `state_meta` function
near the top of `scripts/scan.sh` (`APPROVE=1 … UNKNOWN=6`); edit those numbers
to reorder.

Because the list is state-sorted, approving a `[!]` makes it change state and
sink down the list, so the next actionable session rises toward your cursor —
the intended triage flow. The trade-off is that the highlighted row can shift
under you on a refresh; sort by `pane_id` alone (a fixed position) if you prefer.

### Keys (inside the popup)

| Key | Action |
|---|---|
| `ctrl-a` | **approve** — accept the highlighted default option (safe primary) |
| `ctrl-y` | pick option **1 (Yes)** explicitly |
| `ctrl-x` | **cancel** the prompt (Esc) |
| `ctrl-d` | **dismiss** the "How is Claude doing?" rating (`[o]` rows only) |
| `ctrl-r` | refresh now |
| `enter`  | **jump** to that pane (closes the popup) |
| type     | fuzzy-filter the list |

The preview pane (right) shows the highlighted session's live contents. The list
auto-refreshes ~1s, so approve one → the row updates → move to the next.

## Safety

- Only `[!]` approval prompts can be approved. `ctrl-a`/`ctrl-y` route through
  `guard-approve.sh`, which **re-captures the pane and only sends the key if the
  approval menu is still there** — so a prompt that closed between the scan and
  your keypress never catches a stray keystroke, and rating/interrupted/idle/
  running panes (no `❯ N.` menu) are no-ops.
- `enter`-accept-default is the primary action; digit-picks and cancel are
  separate explicit keys.
- Every tmux command targets a unique **pane_id** — no session/window guessing.

## Maintenance

It's screen-scraping, so the CC TUI changing its wording/glyphs can throw off
classification. All the fragile patterns live in **one table at the top of
`scripts/scan.sh`** (`RE_*` / `GLYPH_*`) — fix them there. The most robust signals
are structural (the `❯ N.` menu, the `(Ns` elapsed timer) rather than English
prose; prefer those when adjusting.

### Files

- `orchbus.tmux` — entry; binds `prefix + o`.
- `scripts/scan.sh` — enumerates CC panes, classifies each, emits the TSV rows.
- `scripts/orchbus.sh` — the fzf UI (binds + auto-refresh loop).
- `scripts/guard-approve.sh` — re-capture-and-verify wrapper for sends.

### Possible enhancements

- RUNNING-vs-IDLE tie-break via a double-capture spinner diff (skipped in v1 —
  the `(Ns` timer resolves it reliably and keeps the scan fast).
- A confirm-gated "approve all `[!]`" bulk key (deliberately not a single keystroke).
