import type { EngineInterface, Register } from 'claude-code'

type State = 'running' | 'idle' | 'approve' | 'input' | 'ended'

type Ask = {
  state: State
  question: string
  tool: string
  tool_use_id: string
}

type Pane = {
  path: string
  pane: string
  serverPid: number
  podUid: string | undefined
}

type Probe = {
  where: Promise<Pane | undefined> | undefined
  main: State
  mainQuestion: string
  shown: State | undefined
  since: number
  unnamed: number
  // Pending asks by tool_use_id, or by a local key when none could be matched.
  asks: Map<string, Ask>
  // The tool_use_id of each call core put to a decider, by tool and input.
  checked: Map<string, string>
  writes: Promise<void>
}

// Tools that always put a question to the person, whatever the mode.
const PERSON_TOOLS: ReadonlySet<string> = new Set([
  'AskUserQuestion',
  'ExitPlanMode',
])

const ENDED_BY: Readonly<Record<string, string>> = {
  aborted: 'Interrupted',
  refusal: 'Refused',
  error: 'API error',
}

const oneLine = (text: string) =>
  text.replace(/\s+/g, ' ').trim().slice(0, 200)

const field = (args: Record<string, unknown>, name: string) =>
  typeof args[name] === 'string' ? (args[name] as string) : undefined

const keyOf = (tool: string, input: unknown) =>
  `${tool}\0${JSON.stringify(input)}`

function asking(tool: string, input: unknown, id: string): Ask {
  const args = (input ?? {}) as Record<string, unknown>
  const ask = (state: State, question: string): Ask =>
    ({ state, question: oneLine(question), tool, tool_use_id: id })
  if (tool === 'AskUserQuestion') {
    const asked = args.questions as { question?: unknown }[] | undefined
    const first = asked?.[0]?.question
    return ask('input', typeof first === 'string' ? first : 'Question')
  }
  if (tool === 'ExitPlanMode') return ask('approve', 'Approve plan?')
  const detail =
    field(args, 'command')?.split('\n')[0] ?? field(args, 'file_path')
  return ask('approve', detail ? `${tool}: ${detail}` : tool)
}

// `$TMUX` is `<socket>,<server pid>,<session>`; the socket may hold commas.
async function locate($: EngineInterface): Promise<Pane | undefined> {
  const pane = await $.env.get('TMUX_PANE')
  const tmux = (await $.env.get('TMUX'))?.split(',') ?? []
  const home = await $.env.get('HOME')
  const base =
    (await $.env.get('XDG_STATE_HOME')) || (home && `${home}/.local/state`)
  const serverPid = Number(tmux.at(-2))
  if (!pane || tmux.length < 3 || !base || !Number.isInteger(serverPid)) {
    return undefined
  }
  const sock = tmux.slice(0, -2).join(',').replace(/[^A-Za-z0-9]/g, '-')
  return {
    path: `${base}/orchbus/panes/${sock}/${pane.replace(/^%/, '')}.json`,
    pane,
    serverPid,
    podUid: await $.env.get('ORCHBUS_POD_UID'),
  }
}

async function write($: EngineInterface, p: Probe) {
  p.where ??= locate($)
  const pane = await p.where
  if (!pane) return
  const ask: Ask | undefined = p.asks.values().next().value
  const state = ask?.state ?? p.main
  const now = await $.clock.now()
  if (state !== p.shown) {
    p.shown = state
    p.since = now
  }
  const record = {
    v: 1,
    state,
    question: ask?.question ?? (state === 'input' ? p.mainQuestion : ''),
    tool: ask?.tool ?? '',
    tool_use_id: ask?.tool_use_id ?? '',
    session_id: await $.session.id(),
    server_pid: pane.serverPid,
    pane: pane.pane,
    ...(pane.podUid ? { pod_uid: pane.podUid } : {}),
    since: p.since,
    updated: now,
  }
  await $.fs.write(pane.path, `${JSON.stringify(record)}\n`)
}

// Writes queue in order so the file always ends on the latest state.
function publish($: EngineInterface, p: Probe) {
  p.writes = p.writes
    .then(() => write($, p))
    .catch(err => $.ui.log(`orchbus-probe: ${err}`, { to: 'debug' }))
  return p.writes
}

function settle(p: Probe, id: string, tool: string) {
  let changed = p.asks.delete(id)
  for (const [key, ask] of p.asks) {
    if (!ask.tool_use_id && ask.tool === tool) {
      p.asks.delete(key)
      changed = true
    }
  }
  for (const [key, checkedId] of p.checked) {
    if (checkedId === id) p.checked.delete(key)
  }
  return changed
}

function reset(p: Probe, state: State, question = '') {
  p.asks.clear()
  p.checked.clear()
  p.main = state
  p.mainQuestion = question
}

export const register: Register = on => {
  const p: Probe = {
    where: undefined,
    main: 'idle',
    mainQuestion: '',
    shown: undefined,
    since: 0,
    unnamed: 0,
    asks: new Map(),
    checked: new Map(),
    writes: Promise.resolve(),
  }

  on('session.start', async ($, e, next) => {
    const started = await next(e)
    p.where = locate($)
    reset(p, 'idle')
    await publish($, p)
    return started
  })

  on('turn.start', async ($, e, next) => {
    if (e.agentId === undefined) {
      p.main = 'running'
      await publish($, p)
    }
    return next(e)
  })

  on('turn.complete', async ($, e, next) => {
    if (e.agentId === undefined) {
      const ended = ENDED_BY[e.reason]
      reset(p, ended ? 'input' : 'idle', ended)
      await publish($, p)
    }
    return next(e)
  })

  on('tool.check', async ($, e, next) => {
    const verdict = await next(e)
    if (verdict.decision === 'ask' && e.tool_use_id) {
      p.checked.set(keyOf(e.tool, e.input), e.tool_use_id)
    }
    return verdict
  }).catch(($, e, next) => next(e))

  // Raised only when a person is about to be asked: the dialog is up unless
  // a hook beneath answers it first.
  on('classic.PermissionRequest', async ($, e, next) => {
    const answered = await next(e)
    if (answered.decision === undefined) {
      const id = p.checked.get(keyOf(e.tool_name, e.tool_input)) ?? ''
      const key = id || `unnamed:${++p.unnamed}`
      p.asks.set(key, asking(e.tool_name, e.tool_input, id))
      await publish($, p)
    }
    return answered
  }).catch(($, e, next) => next(e))

  on('tool.call', async ($, e, next) => {
    if (PERSON_TOOLS.has(e.tool)) {
      p.asks.set(e.tool_use_id, asking(e.tool, e, e.tool_use_id))
      await publish($, p)
    }
    try {
      return await next(e)
    } finally {
      if (settle(p, e.tool_use_id, e.tool)) await publish($, p)
    }
  }).catch(($, e, next) => next(e))

  on('session.end', async ($, e, next) => {
    const goesOn = e.reason === 'clear' || e.reason === 'resume'
    reset(p, goesOn ? 'idle' : 'ended')
    await publish($, p)
    return next(e)
  })
}
