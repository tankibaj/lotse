// Lotse's Claude Code mod: live data hooks cannot give (agents, usage, per-agent permission asks).
// Loaded from the plugin Lotse injects into its own panes (`--plugin-dir`); inert anywhere else.
//
// Out: status lines (JSON, one per line) go to Lotse through `lotse-hook mod-status`, only while the
// bridge below is attached. In: `lotse-hook mod-bridge` runs for the session's life and writes the
// daemon's commands to its stdout, one JSON line each (`ping`, `submit`, `answer`). Neither carries a
// secret: the binary finds the socket and its credential file itself. Every hook here but one only
// observes: it answers what `next` answers, so a failure in Lotse never changes what Claude does.
//
// The exception is the two-way part. `submit` hands the composer's text to `$.prompt.submit`. A
// `PermissionRequest` is held for at most HOLD_MS while the sidebar's Allow/Deny may arrive as `answer`;
// after that (or with no bridge, or with another request already held) it goes on to the dialog, and
// Lotse answers by keystrokes. Claude cuts a hook at 10 s and ignores a late allow, hence the margin.
//
// Loader rules (`claude plugin validate`): `$` goes only to top-level functions, `$.env.get` takes a
// literal name, no dynamic imports.

const MOD_VERSION = 1
const MIN_VERSION = [2, 1, 311] // the early-access module API; older builds get hooks and screen only
const HEARTBEAT_MS = 10000
const MAX_SUMMARY = 200
const HOLD_MS = 7500 // Lotse's window to answer a permission request; the daemon's own is a little shorter
const MAX_SUBMIT = 100000
const MAX_FAST_DEATHS = 5 // a bridge that dies this many times in a row, each within MIN_LIFE_MS, is given up on
const MIN_LIFE_MS = 5000
const SUMMARY_KEYS = ['command', 'file_path', 'notebook_path', 'path', 'url', 'pattern', 'query', 'description']

// state of this load of the module (a reload starts it over)
// `hold`: resolves the permission request being held with Lotse's decision, `null` when none is held
const live = { on: false, up: false, flushing: false, bin: 'lotse-hook', queue: [], hold: null }

const cap = text => (typeof text === 'string' ? text.slice(0, MAX_SUMMARY) : undefined)

// "what the call is about", from the few input fields a card shows; never file contents or prompts
const summarize = e => {
  for (const key of SUMMARY_KEYS) {
    if (typeof e[key] === 'string' && e[key] !== '') return cap(e[key])
  }
  return undefined
}

const supported = version => {
  const found = /(\d+)\.(\d+)\.(\d+)/.exec(version ?? '')
  if (!found) return false
  for (let i = 0; i < 3; i++) {
    const have = Number(found[i + 1])
    if (have !== MIN_VERSION[i]) return have > MIN_VERSION[i]
  }
  return true
}

// One lotse-hook process at a time: lines that arrive meanwhile ride along with the next one.
async function flush($) {
  if (live.flushing) return
  live.flushing = true
  try {
    while (live.queue.length > 0) {
      const input = live.queue.splice(0).join('\n') + '\n'
      await $.process.run([live.bin, 'mod-status'], { stdin: input, timeoutMs: 5000 }).catch(() => {})
    }
  } finally {
    live.flushing = false
  }
}

function emit($, line) {
  if (!live.on || !live.up) return
  live.queue.push(JSON.stringify(line))
  void flush($).catch(() => {})
}

async function beat($) {
  if (!live.up) return
  const line = { t: 'heartbeat', ts: await $.clock.now() }
  try {
    const usage = await $.session.usage()
    line.context_pct = usage.context.percent
    line.context_window = usage.context.window
    line.rate_limits = usage.rateLimits.map(r => ({ kind: r.kind, pct: r.percentUsed, resets_at: r.resetsAt }))
    line.cost_usd = usage.cost?.usd
  } catch {}
  try {
    const agents = await $.agent.list()
    line.agents = agents.map(a => ({
      id: a.id,
      parent: a.parentId,
      type: a.type,
      description: cap(a.description),
      status: a.status,
      name: a.name,
    }))
  } catch {}
  emit($, line)
}

async function onBridgeLine($, text) {
  let message
  try {
    message = JSON.parse(text)
  } catch {
    return
  }
  if (message?.bridge === 'attached') {
    live.up = true
    await beat($)
  } else if (message?.cmd === 'ping') {
    await beat($)
  } else if (message?.cmd === 'answer') {
    // only Allow and Deny: "always" changes what Claude may do later, so it stays a keystroke answer
    if (live.hold && (message.decision === 'allow' || message.decision === 'deny')) live.hold(message.decision)
  } else if (message?.cmd === 'submit' && live.up) {
    await submit($, message.text)
  }
}

// The composer's text into the prompt; Lotse types it itself when this says it did not work.
async function submit($, text) {
  if (typeof text !== 'string' || text === '' || text.length > MAX_SUBMIT) return
  let ok = false
  try {
    await $.prompt.submit(text)
    ok = true
  } catch {}
  emit($, { t: 'submit_result', ok })
}

// Hold a permission request for Lotse's answer. One at a time: an answer names no agent, so with two
// held it could land on the wrong one.
async function holdPermission($, e, next) {
  if (!live.on || !live.up || live.hold) return next(e)
  const answered = new Promise(resolve => {
    live.hold = resolve
  })
  emit($, { t: 'perm_hold', agent_id: e.agent_id ?? e.agentId, tool: String(e.tool_name ?? e.tool ?? '') })
  const decision = await Promise.race([answered, $.clock.sleep(HOLD_MS).then(() => null)])
  live.hold = null
  emit($, { t: 'perm_release' })
  if (decision === 'allow') return { decision: { behavior: 'allow' } }
  if (decision === 'deny') return { decision: { behavior: 'deny', message: 'Denied from Lotse' } }
  return next(e)
}

// Keeps `lotse-hook mod-bridge` running; its stdout is the daemon's commands.
async function runBridge($, claudeVersion) {
  const hello = JSON.stringify({ t: 'hello', mod: MOD_VERSION, claude: claudeVersion }) + '\n'
  let deaths = 0
  while (live.on && deaths < MAX_FAST_DEATHS) {
    const started = await $.clock.now()
    let pending = ''
    try {
      for await (const { stream, text } of $.process.spawn({ argv: [live.bin, 'mod-bridge'], input: hello })) {
        if (stream !== 'stdout') continue
        pending += text
        let end
        while ((end = pending.indexOf('\n')) >= 0) {
          const row = pending.slice(0, end)
          pending = pending.slice(end + 1)
          await onBridgeLine($, row)
        }
      }
    } catch {}
    live.up = false
    deaths = (await $.clock.now()) - started < MIN_LIFE_MS ? deaths + 1 : 0
    await $.clock.sleep(Math.min(30000, 1000 * 2 ** deaths))
  }
}

export const register = on => {
  on('session.start', async ($, e, next) => {
    const started = await next(e)
    const session = await $.env.get('LOTSE_SESSION_ID')
    const info = await $.session.version()
    if (session && supported(info.base ?? info.version)) {
      live.on = true
      live.bin = (await $.env.get('LOTSE_HOOK_BIN')) || 'lotse-hook'
      $.clock.every(HEARTBEAT_MS, () => void beat($))
      void runBridge($, info.version)
    }
    return started
  })

  on('session.end', ($, e, next) => {
    live.on = false
    live.up = false
    return next(e)
  })

  on('tool.call', ($, e, next) => {
    emit($, { t: 'tool', agent_id: e.agentId, tool: String(e.tool), summary: summarize(e) })
    return next(e)
  })

  // an ask for a person is the one prompt that names its agent (the Notification hook does not)
  on('tool.check', async ($, e, next) => {
    const verdict = await next(e)
    if (e.approver === 'person' && verdict.decision === 'ask') {
      emit($, { t: 'perm', agent_id: e.agentId, tool: String(e.tool), summary: summarize(e) })
    }
    return verdict
  })

  on('classic.PermissionRequest', ($, e, next) => holdPermission($, e, next))

  on('turn.complete', ($, e, next) => {
    const u = e.usage
    if (u) {
      emit($, {
        t: 'turn',
        agent_id: e.agentId,
        model: u.model,
        input: u.input_tokens,
        output: u.output_tokens,
        cache_read: u.cache_read_input_tokens,
        cache_creation: u.cache_creation_input_tokens,
      })
    }
    return next(e)
  })
}
