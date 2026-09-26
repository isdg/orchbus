# orchbus Phase 0 spike: findings (2026-09-26)

All five go/no-go questions pass. Verdict: GO, with the corrections below.

| # | Question | Result |
|---|---|---|
| 1 | Native kwok control plane on macOS | PASS. kwok 0.8.0 downloads darwin-arm64 etcd, apiserver, controller-manager, scheduler (k8s 1.36.1). Starts in ~16 s, ~480 MB RSS. Data survives stop/start. |
| 2 | Virtual Kubelet node, pods, logs, exec | PASS. VK v1.14.0 node registers Ready; scheduler places a pod via nodeSelector+toleration; `kubectl logs` = pane screen; `kubectl exec -- orchbus:send` types into the pane; plain exec runs a process; delete closes the window. |
| 3 | CRDs in stock k9s, plugins | PASS. k9s 0.51 shows Approval/Task printer columns, aliases `:pp :tk :ag` work, plugin key `a` approved an Approval, `l` shows agent logs. |
| 4 | CEL policy against self-approval | PASS. Worker self-approve denied; lead approves worker permission; lead land approval denied (human-only); chain rewrite denied; human allowed. Audit log records real (impersonated) identities, denied attempts included. |
| 5 | Claude Code hooks via `--settings` | PASS. SessionStart, UserPromptSubmit, PreToolUse(Bash), PermissionRequest(Bash), Notification("Claude needs your permission"), SessionEnd(reason) all fire in an interactive pane. |

## Corrections to the plan

1. **Never touch ~/.kube/config.** kwokctl rewrites it and switches current-context, which can silently
   repoint a production context. `orchbus up` must always use its own kubeconfig
   (`~/.local/state/orchbus/kubeconfig`) and `o6s` must pass `--kubeconfig`/`KUBECONFIG` explicitly.
2. **`--disable kwok-controller`** is mandatory, or kwok's controller manages every node and fights ours.
   Audit via `--kube-audit-policy`.
3. **CRDs cannot have a custom `/approval` subresource** (only status/scale). The decision lives in
   `spec.decision`/`spec.choice`; CEL enforces who may change it and makes subject/kind immutable.
4. **Node endpoint is unauthenticated by default.** The apiserver does not verify the node's cert, and
   without `nodeutil.WithAuth` anyone on the machine could exec into agents via port 10250. Node PR 1
   must enable webhook authn/authz against the apiserver.
5. **Pane addressing:** tmux treats `.` in window names as a pane separator. Address panes by a pane
   user option `@orchbus_uid=<pod uid>`, never by name.
6. **Delete must report a terminal container state**, or pods hang in Terminating.
7. **Workspace trust dialog:** every new worktree hits "Is this a project you trust?" before any hook
   fires. Worktree creation must pre-trust the path (or live under a trusted root); the pattern table
   needs a `trust` state (its menu has no `❯ N.` so it is not seen as approvable today).
8. **No Stop hook after a declined permission.** The agent returns to the prompt silently; idle after a
   denial still needs the scrape or the later idle notification. Hooks may also repeat events: the node
   must dedupe by (session, event, tool_use_id).
9. **Build:** Go links fail with the installed Command Line Tools against the macOS 27 SDK; build the node
   with `CGO_ENABLED=0` (pure Go works) until CLT is updated.
10. **k9s views.yaml** custom annotation columns did not apply with the syntax tried; resolve in o6s PR 2.

## Reproducing

The spike code was throwaway and is not in this repo. To repeat it: install kwok (`brew install kwok`),
create a cluster with `kwokctl create cluster --runtime binary --disable kwok-controller
--kube-audit-policy <file> --kubeconfig <own file>`, apply CRDs and a ValidatingAdmissionPolicy, run a
Virtual Kubelet provider built with `CGO_ENABLED=0`, and start `claude --settings <hooks.json>` in a pane.
