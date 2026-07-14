# orchbus

A tmux cockpit for driving many **Claude Code** sessions at once. When you run a
dozen CC sessions across tmux panes, the bottleneck is *you* tabbing around to
find which agent is blocked. orchbus scans every pane, shows all the CC sessions
in one popup with their current state and pending question, and lets you
approve / respond to each **from that one window** — no tabbing.

A small Rust binary drives it: it *reads* panes with `capture-pane` and *acts*
on them with `send-keys`. No Claude Code hooks, config, or plugins required.

## Install

Via [TPM](https://github.com/tmux-plugins/tpm), add to `~/.tmux.conf`:

```tmux
set -g @plugin 'isdg/orchbus'
```

Then `prefix + I` to fetch it. Or add it directly:

```tmux
run-shell '~/.tmux/plugins/orchbus/orchbus.tmux'
```

Reload: `tmux source-file ~/.tmux.conf`. Requires **fzf ≥ 0.36** (`start`/`load`
events) and tmux 3.2+. The `orchbus` binary is built on first load (background
`cargo install`, rebuilt when the source is newer — so `prefix U` updates take
effect); needs **rust/cargo**.

Open the cockpit with `prefix o` (ephemeral popup) or `prefix O` (a real,
reused tmux window).

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
the ~1s auto-refresh. The ranking lives in one place — the `meta` function in
`src/classify.rs` (`Approve => 1 … Unknown => 6`); edit those numbers to reorder.

Because the list is state-sorted, approving a `[!]` makes it change state and
sink down the list, so the next actionable session rises toward your cursor —
the intended triage flow. The trade-off is that the highlighted row can shift
under you on a refresh; sort by `pane_id` alone (a fixed position) if you prefer.

### Keys (inside the popup)

| Key | Action |
|---|---|
| `ctrl-a` | **approve** — accept the highlighted default "Yes" (safe primary) |
| `ctrl-x` | **cancel** the prompt (Esc) |
| `ctrl-r` | refresh now |
| `enter`  | **jump** to that pane (closes the popup) |
| type     | fuzzy-filter the list |

The preview pane (right) shows the highlighted session's live contents. The list
auto-refreshes ~1s, so approve one → the row updates → move to the next.

## Safety

- Only `[!]` approval prompts can be approved. `ctrl-a` routes through
  `orchbus approve`, which **re-captures the pane and only sends the key if the
  approval menu is still there** — so a prompt that closed between the scan and
  your keypress never catches a stray keystroke, and rating/interrupted/idle/
  running panes (no `❯ N.` menu) are no-ops. It shares the exact menu pattern
  with the scanner (both use `src/classify.rs`), so the guard can't drift.
- Approve just accepts the highlighted default "Yes"; cancel is a separate key.
- Every tmux command targets a unique **pane_id** — no session/window guessing.

## Maintenance

It's screen-scraping, so the CC TUI changing its wording/glyphs can throw off
classification. All the fragile patterns live in **one module, `src/classify.rs`**
(the `PATTERN TABLE` — `RATING`, `APPROVE_MENU`, … regexes), used by both the
scanner and the approve guard. Fix them there. The most robust signals are
structural (the `❯ N.` menu, the `(Ns ·` elapsed timer) rather than English
prose; prefer those when adjusting.

### Files

- `orchbus.tmux` — entry; resolves/builds the binary, binds `prefix o`/`O`.
- `src/main.rs` — CLI: `scan` / `approve` / `cancel` / `ui` / `open`.
- `src/classify.rs` — the PATTERN TABLE + `classify` / `meta` (shared).
- `src/scan.rs` — enumerates CC panes, classifies each, emits + caches the rows.
- `src/ui.rs` — the fzf cockpit (binds + auto-refresh loop) and window opener.
- `src/tmux.rs` — tmux + fzf helpers.

### Possible enhancements

- RUNNING-vs-IDLE tie-break via a double-capture spinner diff (skipped in v1 —
  the `(Ns` timer resolves it reliably and keeps the scan fast).
- A confirm-gated "approve all `[!]`" bulk key (deliberately not a single keystroke).
