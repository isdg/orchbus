import { expect, mock, test } from 'claude-code/testing'
import type { Engine } from 'claude-code/testing'
import type { On } from 'claude-code'

const TMUX = {
  TMUX: '/tmp/tmux-501/default,4242,0',
  TMUX_PANE: '%7',
  HOME: '/home/u',
}
const PATH = '/home/u/.local/state/orchbus/panes/-tmp-tmux-501-default/7.json'

type Record = { [field: string]: unknown }

// Every probe write, parsed, in order; the engine beneath answers the rest.
function world(on: On, env: { [name: string]: string } = TMUX) {
  const writes: { path: string; record: Record }[] = []
  mock.env(on, env)
  const clock = mock.clock(on, { now: 1000 })
  on('session.id', () => ({ value: 's1' }))
  on('fs.write', (_, e) => {
    writes.push({ path: e.path, record: JSON.parse(e.text) })
    return { value: undefined }
  })
  on('session.start', (_, e) => ({ cwd: e.cwd }))
  on('session.end', (_, e) => ({ sessionId: e.sessionId }))
  on('turn.start', (_, e) => ({ turnId: e.turnId }))
  on('turn.complete', () => ({ text: '' }))
  const last = () => writes.at(-1)?.record
  const states = () => writes.map(w => w.record.state)
  return { writes, last, states, clock }
}

const start = ($: Engine) =>
  $.session.start({ cwd: '/', surface: 'terminal', isInteractive: true })

test('a session in tmux starts idle in its pane file', async ($, on) => {
  const w = world(on)
  await start($)

  expect(w.writes[0]?.path).toBe(PATH)
  expect(w.last()).toEqual({
    v: 1,
    state: 'idle',
    question: '',
    tool: '',
    tool_use_id: '',
    session_id: 's1',
    server_pid: 4242,
    pane: '%7',
    since: 1000,
    updated: 1000,
  })
})

test('outside tmux nothing is written', async ($, on) => {
  const w = world(on, { HOME: '/home/u' })
  await start($)
  await $.turn.start({ text: 'hi', turnId: 't1' })

  expect(w.writes).toEqual([])
})

test('XDG_STATE_HOME and the pod uid are honoured', async ($, on) => {
  const w = world(on, {
    ...TMUX,
    XDG_STATE_HOME: '/state',
    ORCHBUS_POD_UID: 'pod-1',
  })
  await start($)

  expect(w.writes[0]?.path).toBe(
    '/state/orchbus/panes/-tmp-tmux-501-default/7.json',
  )
  expect(w.last()?.pod_uid).toBe('pod-1')
})

test('a main turn runs, then answers back to idle', async ($, on) => {
  const w = world(on)
  await start($)
  await $.turn.start({ text: 'go', turnId: 't1' })
  expect(w.last()?.state).toBe('running')

  await $.turn.complete({
    answer: 'done',
    durationMs: 5,
    isAborted: false,
    turnId: 't1',
    reason: 'answer',
  })
  expect(w.states()).toEqual(['idle', 'running', 'idle'])
})

test('an interrupted turn waits for input', async ($, on) => {
  const w = world(on)
  await start($)
  await $.turn.start({ text: 'go', turnId: 't1' })
  await $.turn.complete({
    answer: '',
    durationMs: 5,
    isAborted: true,
    turnId: 't1',
    reason: 'aborted',
  })

  expect(w.last()?.state).toBe('input')
  expect(w.last()?.question).toBe('Interrupted')
})

test("a subagent's turns leave the main state alone", async ($, on) => {
  const w = world(on)
  await start($)
  await $.turn.start({ text: 'go', turnId: 't1' })
  await $.turn.start({ text: 'sub', turnId: 't2', agentId: 'a1' })
  await $.turn.complete({
    answer: 'report',
    durationMs: 5,
    isAborted: false,
    turnId: 't2',
    agentId: 'a1',
    reason: 'answer',
  })

  expect(w.states()).toEqual(['idle', 'running'])
})

test('a permission ask is approve until the call settles', async ($, on) => {
  const w = world(on)
  let during: Record | undefined
  on('tool.check', () => ({ decision: 'ask' }))
  // No test plays the person's yes: a PermissionRequest answer stands for it.
  on('classic.PermissionRequest', () => ({
    decision: { behavior: 'allow' as const },
  }))
  on('tool.call', () => {
    during = w.last()
    return { result: {} }
  })
  await start($)
  await $.turn.start({ text: 'go', turnId: 't1' })
  await $.tool.call({ tool: 'Bash', command: 'git push\nmore' })

  expect(during?.state).toBe('approve')
  expect(during?.question).toBe('Bash: git push')
  expect(during?.tool).toBe('Bash')
  expect(String(during?.tool_use_id)).not.toBe('')
  expect(w.states()).toEqual(['idle', 'running', 'approve', 'running'])
})

test('a call nobody is asked about never reads approve', async ($, on) => {
  const w = world(on)
  on('tool.check', () => ({ decision: 'allow' }))
  on('tool.call', () => ({ result: {} }))
  await start($)
  await $.turn.start({ text: 'go', turnId: 't1' })
  await $.tool.call({ tool: 'Bash', command: 'ls' })

  expect(w.states()).toEqual(['idle', 'running'])
})

test('AskUserQuestion waits for input, not approval', async ($, on) => {
  const w = world(on)
  let during: Record | undefined
  on('tool.check', () => ({ decision: 'allow' }))
  on('tool.call', () => {
    during = w.last()
    return { result: {} }
  })
  await start($)
  await $.turn.start({ text: 'go', turnId: 't1' })
  await $.tool.call({
    tool: 'AskUserQuestion',
    questions: [
      {
        question: 'Which  one?',
        header: 'Pick',
        options: [],
        multiSelect: false,
      },
    ],
  })

  expect(during?.state).toBe('input')
  expect(during?.question).toBe('Which one?')
  expect(w.last()?.state).toBe('running')
})

test('ExitPlanMode asks to approve the plan', async ($, on) => {
  const w = world(on)
  let during: Record | undefined
  on('tool.check', () => ({ decision: 'allow' }))
  on('tool.call', () => {
    during = w.last()
    return { result: {} }
  })
  await start($)
  await $.tool.call({ tool: 'ExitPlanMode' })

  expect(during?.state).toBe('approve')
  expect(during?.question).toBe('Approve plan?')
})

test('/clear leaves the pane idle, an exit ends it', async ($, on) => {
  const w = world(on)
  await start($)
  await $.session.end({
    reason: 'clear',
    sessionId: 's1',
    resume: { id: 's1' },
  })
  expect(w.last()?.state).toBe('idle')

  await $.session.end({
    reason: 'prompt_input_exit',
    sessionId: 's1',
    resume: { id: 's1' },
  })
  expect(w.last()?.state).toBe('ended')
})

test('since holds while the state does', async ($, on) => {
  const w = world(on)
  await start($)
  await w.clock.advance(50)
  await $.turn.start({ text: 'go', turnId: 't1' })
  await w.clock.advance(50)
  await $.turn.start({ text: 'more', turnId: 't2' })

  expect(w.last()?.since).toBe(1050)
  expect(w.last()?.updated).toBe(1100)
})
