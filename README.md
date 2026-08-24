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

orchbus binds **no keys of its own** — it resolves and builds the binary, and
the key map stays in your `~/.tmux.conf`, where one file owns every binding and
a plugin update can never move a key underneath you. Paste this after the TPM
`run` line and adjust to taste:

```tmux
run-shell 'ORCHBUS="$(command -v orchbus || echo "$HOME/.cargo/bin/orchbus")"; \
  tmux bind-key o display-popup -E -w 100% -h 100% "$ORCHBUS ui"; \
  tmux bind-key O run-shell "$ORCHBUS open"'
```

That opens the cockpit with `prefix o` (ephemeral popup) or `prefix O` (a real,
reused tmux window) — the keys this README assumes throughout.

## Use — `prefix + o`

Opens the cockpit popup. One row per Claude Code pane:

```
[!]  ~/cosmos-stih    stih:1    scribe.yaml refactor  Do you want to make this edit?
[?]  ~/plc            plc:1     plc daily wrapper     Interrupted · What instead?
[*]  ~/cosmos-skazka  skazka:1  image alignment       running
[=]  ~/cosmos-oda     oda:1     flowLine node         (waiting)
[o]  ~/cosmos-main    cosmos:2  ...                   How is Claude doing? (optional)
```

The second column is the pane's working directory, shown home-relative. It is
also the grouping key of the directory view below.

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

#### Directory view — `ctrl-g`

`ctrl-g` flips between the importance order above and a **directory** order:
groups sorted by working directory with **the directory you opened the cockpit
from first**, and the same importance ranking *inside* each group. It is the
`claude agents` layout — useful when you are looking for a known session rather
than triaging whatever is loudest.

The cost is real and worth stating: grouping by directory buries an `[!]` inside
its group, so the loudest pane is no longer guaranteed to be on top. That is why
importance stays the default and this is a toggle.

The choice persists (a file next to the cache), because the ~1s auto-refresh
re-runs `scan` as a fresh process — a mode held inside fzf would snap back to
importance on every reload. `orchbus sort` prints the current mode;
`orchbus sort --toggle` flips it from a shell.

### Keys (inside the popup)

| Key | Action |
|---|---|
| `ctrl-a` | **approve** — accept the highlighted default "Yes" (safe primary) |
| `ctrl-x` | **cancel** the prompt (Esc) |
| `ctrl-g` | **group by directory** (toggle) — current directory first |
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

## Driving the loop — `spawn` / `review` / `fork`

Beyond triage, orchbus can *launch* agents and run them through a
plan → implement → review → revise loop. Each spawned agent runs in its **own
git worktree** on a fresh branch and its **own tmux window** — so the work is
isolated on disk and visible in the cockpit at the same time. Every spawn is
recorded under a short **slug** in `.orchbus/state.json`, and the later verbs
find it by that slug.

```sh
# 1. Spawn a planning agent in an isolated worktree (interactive, plan mode).
orchbus spawn "add retry with backoff to the http client" --tag plan
#   → spawned 'add-retry-with-backoff-to-the-http' (tag plan) …

# 2. Once it produces a plan, capture it to .orchbus/plans/<slug>.md.
orchbus capture add-retry-with-backoff-to-the-http

# 3. (optional) Gate the plan itself with a fresh reviewer before any code.
orchbus review add-retry-with-backoff-to-the-http --plan

# 4. After the agent implements, review the DIFF against the captured plan.
#    A brand-new headless `claude -p` reviewer (no prior context, so it can't
#    rubber-stamp its own reasoning) scores two axes — spec + correctness —
#    and writes findings to .orchbus/reviews/<slug>.md.
orchbus review add-retry-with-backoff-to-the-http

# 5. Hand the review back to the same agent to fix the findings.
orchbus revise add-retry-with-backoff-to-the-http

# 6. Explore a divergent approach without disturbing the original.
orchbus fork add-retry-with-backoff-to-the-http --prompt "try a token bucket instead"
```

### Tags (roles)

A **tag** is a named launch profile deciding *how* an agent runs for a step:
which agent, its `--permission-mode`, an `--append-system-prompt` role, and
whether it's an interactive pane or headless. Three built-ins cover the loop
with **no config**:

| Tag | Runs as | Purpose |
|---|---|---|
| `plan` | interactive, `--permission-mode plan` | produce a plan (no edits) |
| `implement` | interactive, `--dangerously-skip-permissions` | do the work (safe — isolated worktree) |
| `review` | headless `-p`, read-only | spec + correctness reviewer |

Override a built-in or add your own in `.orchbus/agents.toml`:

```toml
[implement]
role = "Prefer the smallest diff that satisfies the plan."
skip_perms = true
```

Because implement agents live in a throwaway worktree, orchbus can safely pass
`--dangerously-skip-permissions` — pass `--no-skip` to opt out.

### Escape hatch — `orchbus claude` (`c`)

Tags are deliberately opinionated. When you'd rather drive Claude Code yourself
with its own flags — but still want the worktree, the tracked slug and a window
the cockpit can see — use the passthrough. It's aliased to **`c`**, so it reads
about the same as typing `claude`:

```sh
orchbus c --model opus -p "fix the flaky retry test"
orchbus c --slug hotfix -- --agent Explore --effort low
orchbus c                         # a plain interactive session, isolated + tracked
```

Everything after `claude` is forwarded **verbatim**, so the whole Claude Code
CLI is available. orchbus injects only two flags, and only when your own args
haven't already spoken for them:

| Injected | Dropped when you pass |
|---|---|
| `--session-id <uuid>` (keeps `fork`/`revise` deterministic) | `--resume` / `--continue` / `--session-id` / `--from-pr` |
| `--dangerously-skip-permissions` (safe — isolated worktree) | `--permission-mode …` / either `*-skip-permissions` flag / `--no-skip` |

Resume a session by id and orchbus records *that* id, so the later verbs still
find it. orchbus's own flags (`--slug`, `--branch`, `--no-skip`) go before the
forwarded arguments — add `--` if one would collide with a claude flag. The slug
defaults to the trailing prompt, or `claude` when there isn't one.

## Claude-driven orchestration

The loop verbs are plain shell commands, so a **Claude Code session can drive
them itself** (via its Bash tool) — the same plan → implement → review → fork
loop it would otherwise run with its *native* subagents. The difference is
**visibility**: native subagents run off-screen, but an `orchbus spawn` opens a
real tmux window that shows up in the cockpit, so you can watch each sub-agent,
read its live output, and approve its prompts as it works.

```sh
orchbus spawn "…subtask…" --tag implement   # visible pane, isolated worktree
orchbus review <slug>                        # fresh reviewer scores the diff
orchbus revise <slug>                        # feed findings back in
```

Tell a driving session to prefer these commands (e.g. via a `CLAUDE.md`
contract) when you want its orchestration to be watchable and approvable rather
than hidden.

## Maintenance

It's screen-scraping, so the CC TUI changing its wording/glyphs can throw off
classification. All the fragile patterns live in **one module, `src/classify.rs`**
(the `PATTERN TABLE` — `RATING`, `APPROVE_MENU`, … regexes), used by both the
scanner and the approve guard. Fix them there. The most robust signals are
structural (the `❯ N.` menu, the `(Ns ·` elapsed timer) rather than English
prose; prefer those when adjusting.

### Files

- `orchbus.tmux` — entry; resolves and builds the binary. Binds nothing.
- `src/main.rs` — CLI: `scan` / `approve` / `cancel` / `ui` / `open`.
- `src/classify.rs` — the PATTERN TABLE + `classify` / `meta` (shared).
- `src/scan.rs` — enumerates CC panes, classifies each, emits + caches the rows.
- `src/ui.rs` — the fzf cockpit (binds + auto-refresh loop) and window opener.
- `src/tmux.rs` — tmux + fzf helpers.

### Possible enhancements

- RUNNING-vs-IDLE tie-break via a double-capture spinner diff (skipped in v1 —
  the `(Ns` timer resolves it reliably and keeps the scan fast).
- A confirm-gated "approve all `[!]`" bulk key (deliberately not a single keystroke).
