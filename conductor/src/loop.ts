// The agentic loop: Claude proposes, the envelope disposes.
//
// A manual tool-use loop (not the tool runner) because the interesting work
// happens at the gate — each propose_write returns a real verdict we log and feed
// back. The model is untrusted; nothing here depends on it behaving. Worst case it
// proposes garbage, the envelope rejects or rolls it back, and the loop moves on.

import Anthropic from '@anthropic-ai/sdk'
import { runTool, tools, type ToolContext } from './tools.js'
import { describeVerdict } from './envelope.js'

const SYSTEM = `You are the autonomous maintainer of "roli-admin", a React + Vite + TypeScript admin tool. You improve it directly: explore the code, then propose file writes that the build accepts.

You operate inside a trust boundary called the envelope. You cannot write files yourself — you propose a write and the envelope decides its fate:
- It only accepts writes within an allowlist: src/components/, src/features/, src/routes/, src/styles/, config/flags/. Anything else (the API client under src/api/, secrets/, infra/, the envelope itself) is REJECTED — that is by design, not a bug to work around. Do not try to edit those; find an allowed path that achieves the goal.
- Every accepted write is verified by the project's real build before it lands. A write that breaks the build is ROLLED BACK and never commits. So make small, self-contained, correct changes and keep the build green.
- Each landed change becomes one git commit, attributed to you, with your rationale as the message.

Working style: you are operating autonomously — the user is not watching in real time and cannot answer questions mid-task. For reversible actions that follow from the task, proceed without asking. Read before you write: understand the existing code, conventions, and imports so your change compiles. Prefer a few correct changes over many speculative ones. If a write is rolled back, read the build output in the verdict, fix the cause, and re-propose. When the task is complete, stop and give a short plain summary of what landed (cite the committed paths) and anything you deliberately left out. Report outcomes faithfully — if something was rejected or rolled back and you couldn't resolve it, say so.`

export interface LoopResult {
  committed: string[]
  turns: number
  stoppedBecause: string
}

export async function runLoop(opts: {
  task: string
  ctx: ToolContext
  maxTurns: number
}): Promise<LoopResult> {
  const client = new Anthropic()
  const messages: Anthropic.MessageParam[] = [{ role: 'user', content: opts.task }]
  const committed: string[] = []

  let turn = 0
  for (; turn < opts.maxTurns; turn++) {
    process.stdout.write(`\n\n── turn ${turn + 1} ─────────────────────────────────────────\n`)

    const stream = client.messages.stream({
      model: 'claude-opus-4-8',
      max_tokens: 64000,
      thinking: { type: 'adaptive' },
      system: SYSTEM,
      tools,
      messages,
    })
    stream.on('text', (delta) => process.stdout.write(delta))
    const message = await stream.finalMessage()
    messages.push({ role: 'assistant', content: message.content })

    if (message.stop_reason !== 'tool_use') {
      return { committed, turns: turn + 1, stoppedBecause: message.stop_reason ?? 'end_turn' }
    }

    const toolUses = message.content.filter(
      (b): b is Anthropic.ToolUseBlock => b.type === 'tool_use',
    )
    const results: Anthropic.ToolResultBlockParam[] = []
    for (const tu of toolUses) {
      const { result, verdict } = await runTool(opts.ctx, tu.name, tu.input)
      if (verdict) {
        process.stdout.write(`\n  ↳ ${describeVerdict(verdict)}\n`)
        if (verdict.outcome === 'committed') committed.push(verdict.path)
      } else {
        process.stdout.write(`\n  · ${tu.name}(${JSON.stringify(tu.input)})\n`)
      }
      results.push({ type: 'tool_result', tool_use_id: tu.id, content: result })
    }
    messages.push({ role: 'user', content: results })
  }

  return { committed, turns: turn, stoppedBecause: 'max_turns' }
}
