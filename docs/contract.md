# orchbus contract v0

The interfaces between the three repositories: **orchbus** (Rust: CLI, the `orchbus node` helper,
agent driver), **o5srm** (Go, the orchbus resource model: the `orchbus.io` API types, the operator,
and `orchbus-node`, the Virtual Kubelet provider run once per tmux server) and **o6s** (k9s config).
Anything not written here is private to one repo. v0 is a draft and changes in place; the version
is bumped from v1 on, when a change would break a released side.

## 1. Cluster access

- The system uses its own kubeconfig at `~/.local/state/orchbus/kubeconfig`, context `orchbus-local`.
  No component reads or writes `~/.kube/config`.
- API group `orchbus.io`, version `v1alpha1`, category `orchbus` (`kubectl get orchbus`).

## 2. Agent pod

Written by the operator, read by the node.

| Field | Value |
|---|---|
| `spec.nodeSelector` | `type: orchbus-tmux` |
| `spec.tolerations` | `virtual-kubelet.io/provider=orchbus:NoSchedule` |
| `spec.containers[0].image` | runtime token, not an image: `claude`, later `codex` |
| `spec.containers[0].args` | the agent's full argv after the binary |
| `orchbus.io/workspace` | Workspace name |
| `orchbus.io/worktree` | absolute worktree path; the node creates and pre-trusts it |
| `orchbus.io/branch`, `orchbus.io/base` | branch name and base commit |
| `orchbus.io/session-id` | session to pin or resume |
| `orchbus.io/resume` | `"true"` to resume `session-id` instead of starting it |
| `orchbus.io/mode` | `interactive` (tmux pane) or `headless` (background process, for Jobs) |
| label `orchbus.io/agent` | `"true"` on every agent pod; what `:ag` in o6s selects |
| label `orchbus.io/task` | owning Task |
| `spec.terminationGracePeriodSeconds` | short, e.g. `2`: the node removes a deleted pod only after it |

Written back by the node:

| Field | Meaning |
|---|---|
| condition `orchbus.io/AwaitingApproval` | `True` while an approval menu shows; message is the question |
| condition `orchbus.io/AwaitingInput` | `True` while the agent waits for a written reply; message is the question |
| annotations `orchbus.io/cost-usd`, `orchbus.io/ctx-pct` | spend and context-window fill |
| annotation `orchbus.io/pane` | tmux pane id, display only |
| condition `orchbus.io/Idle` | `True` at the prompt with the turn finished |
| container state | `terminated` with exit code once the pane or process is gone, including after delete |

Every `orchbus.io/*` condition carries the raw agent state as its reason: `Approve`, `Input`,
`Running`, `Idle`, `Rating`, `Unknown`. State lives in pod status, which the node owns, rather than in
annotations, which would give the node a second writer on pod metadata.

Panes are identified by the pane option `@orchbus_uid=<pod uid>`, never by window name.

## 3. Exec verbs

Anything that acts on an agent goes through `pods/exec`, so RBAC and audit apply.

| Command | Effect |
|---|---|
| `orchbus:approve [N]` | select menu option N (default 1), only if the menu is still showing |
| `orchbus:cancel` | send Escape to the prompt |
| `orchbus:interrupt` | interrupt the running turn |
| `orchbus:send <text>` | type a single-line message and submit it |
| anything else | runs as a process in the worktree |

`kubectl attach -it` attaches to the pane.

## 4. Node helper

The Go node calls the Rust binary; each verb reads one JSON object on stdin and writes one on stdout.
Non-zero exit means failure, with `{"error": "..."}` on stdout.

`orchbus node version | spawn | kill | list-panes | state | send | approve | capture |
transcript | cost | worktree-ensure | worktree-remove`

`orchbus node version` returns `{"contract": "v0"}`; the node refuses to start on a mismatch.
Requests may carry fields a verb does not know; they are ignored.

| Verb | Request | Response |
|---|---|---|
| `version` | `{}` | `{"contract": "v0"}` |
| `spawn` | `{"uid", "session", "name", "cwd", "argv": [..], "env": {..}}` | `{"pane": "%12"}`; the existing pane when `uid` already runs |
| `kill` | `{"uid"}` | `{"killed": true}`, or `false` when no pane had `uid` |
| `list-panes` | `{}` | `[{"uid", "pane", "pid", "command"}]`, only panes with `@orchbus_uid`; `[]` when no tmux server runs |
| `state` | `{"uid"}` | `{"pane", "state", "question", "tool_use_id", "source"}`; `state` as in §2, `question` empty unless the agent is asking, `tool_use_id` the pending call when the probe names one, `source` `probe` or `screen` (§5) |
| `approve` | `{"uid", "choice"?, "tool_use_id"?}` | `{"sent": true}`, or `false` when no approval menu is on screen, the probe says the agent is not asking for approval, or `tool_use_id` is not the call it is asking about; `choice` 1-9, default the highlighted option |
| `cancel`, `interrupt` | `{"uid"}` | `{"sent": true}` |
| `send` | `{"uid", "text"}` | `{"sent": true}`; `text` must be one non-empty line |
| `capture` | `{"uid", "lines"?}` | `{"text"}`: the screen, plus `lines` of scrollback before it |

The helper talks to the default tmux server. `ORCHBUS_TMUX_SOCKET` names an exact socket
instead, passed to tmux as `-S`; tests always set it, because `-S` never falls back to the
user's own server the way a missing `TMUX_TMPDIR` does.

`approve` re-reads the screen immediately before sending, so a menu that closed since the caller
looked never receives a keystroke. `spawn` opens a detached window in `session`, creating the session with the agent as its first
window when it does not exist, then sets `@orchbus_uid` on the pane. `env` is set in the pane's
environment, which is how `ORCHBUS_POD_UID` (§5) reaches the probe. A pane whose
process exits disappears from `list-panes`; the node reports that pod as terminated.

## 5. Probe

The screen is a fallback. The authority is the probe: the `orchbus-probe`
Claude Code plugin, enabled for every session, writes the agent's state from
inside the agent to one file per tmux pane.

- Path: `${XDG_STATE_HOME:-~/.local/state}/orchbus/panes/<sock>/<n>.json`.
  `<sock>` is the tmux socket path with every non-alphanumeric character
  replaced by `-`; `<n>` is the pane id without its `%`. The probe reads both
  from `$TMUX` and `$TMUX_PANE`, and writes nothing outside tmux.
- Content, one JSON object:

  | Field | Meaning |
  |---|---|
  | `v` | format version, `1` |
  | `state` | `running`, `idle`, `approve`, `input` or `ended` |
  | `question` | one line, no tabs; empty unless `approve` or `input` |
  | `tool`, `tool_use_id` | the call awaiting the person, else empty |
  | `session_id` | the Claude Code session |
  | `server_pid` | the tmux server's pid, from `$TMUX` |
  | `pane` | the pane id, `%12` |
  | `pod_uid` | `$ORCHBUS_POD_UID`, when set |
  | `since`, `updated` | ms since the epoch: the state's start, the write |

- A reader ignores the file when `server_pid` is not the live server's
  `#{pid}` (pane ids restart with the server), when it does not parse, when
  `v` is unknown, and when `state` is `ended`; it then reads the screen.
- A usable probe decides the state. The screen still adds `Rating` over an
  `idle` probe, and covers the trust dialog, which precedes every hook.
- `AskUserQuestion` is `input`, not `approve`: it is answered, not accepted.
- Keystrokes stay guarded by the screen: `approve` sends only while the menu
  shows and a usable probe, if any, says `approve`.

## 6. Approval

- `spec.kind`: `permission`, `plan`, `land`, `budget`. `spec.subject.pod`, `spec.subject.chain`
  (service accounts of the subject and its ancestors). `spec.question`, `spec.options`.
- Decisions are written to `spec.decision` (`pending`, `approved`, `denied`) and `spec.choice`.
  CRDs have no custom subresources, so ValidatingAdmissionPolicies enforce who may decide:
  nobody in `chain`; `land` and `budget` by humans only; `subject` and `kind` immutable.
