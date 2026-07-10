// The agentic loop: Claude proposes, the envelope disposes.
//
// A manual tool-use loop (not the tool runner) because the interesting work
// happens at the gate — each staged write and each commit returns a real verdict
// we log and feed back. The model is untrusted; nothing here depends on it
// behaving. Worst case it proposes garbage, the envelope rejects it or the build
// fails, nothing lands, and the loop moves on.

import Anthropic from '@anthropic-ai/sdk'
import { runTool, buildTools, type ToolContext } from './tools.js'
import { describeVerdict } from './envelope.js'

/**
 * The brief, written against whatever observation sources are mounted. The sources
 * are named to the agent but never presumed — with none, it simply builds from the
 * task text; with one or more, it observes them first.
 */
function systemPrompt(sourceNames: string[]): string {
  const sources =
    sourceNames.length === 0
      ? `You have no observation sources this run — work from the task text alone.`
      : `Your read-only observation sources this run: ${sourceNames.join(', ')}. Observe them with list_source/read_source. A source is READ-ONLY — you cannot edit it in place; if you want to build on one, adopt a clone (establish_workspace mode "clone", naming that source), which the envelope will only adopt if it builds green.`

  return `You are a senior engineer, chartered by ROLI to deliver an outcome. You work autonomously inside a trust boundary called Charter: you cannot touch the filesystem or decide your own permissions. You may READ freely — your own workspace (list_dir/read_file) and any read-only observation sources (list_source/read_source). You may CHANGE the workspace only by establishing it and staging writes that the envelope verifies with the project's own build before anything commits. This is pre-launch "genesis": a human will review your result before it goes live, so you are free to act — but be the engineer you would want reviewing your work.

Under the genesis charter you may write anywhere in the workspace EXCEPT secrets/ and .git/. Those are rejected by design.

${sources}

Your job, in order:

1. OBSERVE. The task gives you observations — some are pointers to a source you can read. Go look properly with the source tools before deciding anything. A source is the real record of what the domain needs.

2. DECIDE YOUR OWN STRATEGY. This is your engineering judgment, across the full range a senior engineer would weigh: do little or nothing if the need is already met; extend; extract just the slice that's needed; fork and modernise; migrate incrementally; or rebuild greenfield. There is no fixed menu and nothing is pre-decided for you.

3. ESTABLISH the workspace once: "empty" for a greenfield build, or "clone" (naming a source) to adopt an existing one.

4. PLAN. Stage PLAN.md FIRST: what you observed, the strategy you chose and WHY (name the options you rejected), and your build plan. It commits together with the build it describes.

5. BUILD it as a coherent changeset of staged writes — complete enough that the project builds, focused enough to review. Read before you write; keep imports and config consistent so it compiles. Choose a modern, sensible stack for a fresh build.

6. COMMIT. Call commit_changeset when the staged set should build. If it returns BUILD_FAILED, read the build output in the verdict, stage fixes, and commit again. Iterate until it is COMMITTED green. A rejected write means you went out of reach — choose an allowed path; do not fight the boundary.

When you have a committed green build (or a deliberate, justified decision to build little), stop and give a short plain summary: the strategy you chose, what landed (PLAN.md and the build), and what you intentionally left for after launch. Report faithfully — if something would not build and you could not resolve it, say so.`
}

export interface LoopResult {
  commits: string[]
  established: boolean
  turns: number
  stoppedBecause: string
}

export async function runLoop(opts: {
  task: string
  ctx: ToolContext
  maxTurns: number
}): Promise<LoopResult> {
  const client = new Anthropic()
  const sourceNames = Object.keys(opts.ctx.sources)
  const system = systemPrompt(sourceNames)
  const tools = buildTools(sourceNames)
  const messages: Anthropic.MessageParam[] = [{ role: 'user', content: opts.task }]
  const commits: string[] = []
  let established = false

  let turn = 0
  for (; turn < opts.maxTurns; turn++) {
    process.stdout.write(`\n\n── turn ${turn + 1} ─────────────────────────────────────────\n`)

    const stream = client.messages.stream({
      model: 'claude-opus-4-8',
      max_tokens: 64000,
      thinking: { type: 'adaptive' },
      system,
      tools,
      messages,
    })
    stream.on('text', (delta) => process.stdout.write(delta))
    const message = await stream.finalMessage()
    messages.push({ role: 'assistant', content: message.content })

    if (message.stop_reason !== 'tool_use') {
      return { commits, established, turns: turn + 1, stoppedBecause: message.stop_reason ?? 'end_turn' }
    }

    const toolUses = message.content.filter(
      (b): b is Anthropic.ToolUseBlock => b.type === 'tool_use',
    )
    const results: Anthropic.ToolResultBlockParam[] = []
    for (const tu of toolUses) {
      const { result, verdict } = await runTool(opts.ctx, tu.name, tu.input)
      if (verdict) {
        process.stdout.write(`\n  ↳ ${describeVerdict(verdict)}\n`)
        if (verdict.outcome === 'committed') commits.push(verdict.commit)
        if (verdict.outcome === 'established') established = true
      } else {
        process.stdout.write(`\n  · ${tu.name}(${preview(tu.input)})\n`)
      }
      results.push({ type: 'tool_result', tool_use_id: tu.id, content: result })
    }
    messages.push({ role: 'user', content: results })
  }

  return { commits, established, turns: turn, stoppedBecause: 'max_turns' }
}

/** A short preview of a read tool's input, for the live log. */
function preview(input: unknown): string {
  const s = JSON.stringify(input)
  return s.length > 80 ? s.slice(0, 77) + '…' : s
}
