# orchbus contract v0

The interfaces between the three repositories: **orchbus** (Rust: CRD types, operator, CLI, agent
driver), **orchbus-node** (Go: Virtual Kubelet provider, one per tmux server) and **o6s** (k9s config).
Anything not written here is private to one repo. Changes bump the version at the top.

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
| label `orchbus.io/task` | owning Task |

Written back by the node:

| Field | Meaning |
|---|---|
| annotation `orchbus.io/state` | `approve`, `input`, `running`, `idle`, `trust`, `rating`, `unknown` |
| annotation `orchbus.io/question` | the live question, if any |
| annotations `orchbus.io/cost-usd`, `orchbus.io/ctx-pct` | spend and context-window fill |
| annotation `orchbus.io/pane` | tmux pane id, display only |
| readiness gates `orchbus.io/AwaitingApproval`, `orchbus.io/Idle` | conditions for the operator |
| container state | `terminated` with exit code once the pane or process is gone, including after delete |

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
| `state` | `{"uid"}` | `{"pane", "state", "question"}`; `state` as in §2, `question` empty unless the agent is asking |

`spawn` opens a detached window in `session`, creating the session with the agent as its first
window when it does not exist, then sets `@orchbus_uid` on the pane. `env` is set in the pane's
environment, which is how `ORCHBUS_POD_UID` and `ORCHBUS_NODE_SOCK` (§5) reach hooks. A pane whose
process exits disappears from `list-panes`; the node reports that pod as terminated.

## 5. Hooks

- Agents are started with `claude --settings <file>` whose hooks run `orchbus hook`.
- The pane environment carries `ORCHBUS_NODE_SOCK` and `ORCHBUS_POD_UID`.
- `orchbus hook` reads the Claude Code hook JSON on stdin and writes one line
  `{"pod_uid", "event", "session_id", "tool_name", "tool_use_id", "message", "ts"}` to the socket,
  then exits 0. It never blocks the agent.
- The node dedupes by `(session_id, event, tool_use_id)`. Hooks are a hint: the node still checks the
  screen before sending any keystroke, and after a declined permission no Stop event arrives.

## 6. Approval

- `spec.kind`: `permission`, `plan`, `land`, `budget`. `spec.subject.pod`, `spec.subject.chain`
  (service accounts of the subject and its ancestors). `spec.question`, `spec.options`.
- Decisions are written to `spec.decision` (`pending`, `approved`, `denied`) and `spec.choice`.
  CRDs have no custom subresources, so ValidatingAdmissionPolicies enforce who may decide:
  nobody in `chain`; `land` and `budget` by humans only; `subject` and `kind` immutable.
