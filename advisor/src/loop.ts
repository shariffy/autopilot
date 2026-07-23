// The agentic loop: Claude proposes, the envelope disposes.
//
// The brain runs under the Claude Agent SDK (Claude Code as a library), which
// authenticates via your Claude Code login (~/.claude) — your subscription — or
// ANTHROPIC_API_KEY if that is set instead. We give it ONLY the envelope-backed
// MCP tools (`tools: []` strips every built-in Read/Write/Bash), so nothing the
// brain does escapes the seam. The model is untrusted; nothing here depends on it
// behaving. Worst case it proposes garbage, the envelope rejects it or the build
// fails, nothing lands, and the loop moves on.

import { query } from '@anthropic-ai/claude-agent-sdk'
import { buildToolServer, SERVER_NAME, type ToolContext, type LoopState } from './tools.js'

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

  return `You are a senior engineer, cleared to deliver an outcome. You work autonomously inside a trust boundary called Autopilot: you cannot touch the filesystem or decide your own permissions, and you have no shell — your only tools are the ones provided. You may READ freely — your own workspace (list_dir/read_file) and any read-only observation sources (list_source/read_source). You may CHANGE the workspace only by establishing it and staging writes that the envelope verifies with the project's own build before anything commits. This is pre-launch "genesis": a human will review your result before it goes live, so you are free to act — but be the engineer you would want reviewing your work.

Under the genesis clearance you may write anywhere in the workspace EXCEPT secrets/ and .git/. Those are rejected by design.

${sources}

Your job, in order:

1. OBSERVE. The task gives you observations — some are pointers to a source you can read. Go look properly with the source tools before deciding anything. A source is the real record of what the domain needs.

2. DECIDE YOUR OWN STRATEGY. This is your engineering judgment, across the full range a senior engineer would weigh: do little or nothing if the need is already met; extend; extract just the slice that's needed; fork and modernise; migrate incrementally; or rebuild greenfield. There is no fixed menu and nothing is pre-decided for you.

3. ESTABLISH the workspace once: "empty" for a greenfield build, or "clone" (naming a source) to adopt an existing one.

4. RECORD your decision. Stage docs/adr/0001-<short-slug>.md FIRST — an Architecture Decision Record: what you observed, the strategy you chose and WHY (name the options you rejected and why they lost), and your build plan. Keep it a clean first-principles record, not a narrative of your process. It commits together with the build it describes. This is the start of a series: add a new docs/adr/NNNN-<slug>.md for each later decision of consequence; never rewrite an accepted record — supersede it.

5. BUILD it as a coherent changeset of staged writes — complete enough that the project builds, focused enough to review. Read before you write; keep imports and config consistent so it compiles. Choose a modern, sensible stack for a fresh build.

6. COMMIT. Call commit_changeset when the staged set should build. If it returns BUILD_FAILED, read the build output in the verdict, stage fixes, and commit again. Iterate until it is COMMITTED green. A rejected write means you went out of reach — choose an allowed path; do not fight the boundary.

When you have a committed green build (or a deliberate, justified decision to build little), stop and give a short plain summary: the strategy you chose, what landed (the ADR and the build), and what you intentionally left for after launch. Report faithfully — if something would not build and you could not resolve it, say so.`
}

export interface LoopResult {
  commits: string[]
  established: boolean
  turns: number
  stoppedBecause: string
  costUsd?: number
}

export async function runLoop(opts: {
  task: string
  ctx: ToolContext
  maxTurns: number
}): Promise<LoopResult> {
  const sourceNames = Object.keys(opts.ctx.sources)
  const system = systemPrompt(sourceNames)
  const state: LoopState = { commits: [], established: false }
  const { server } = buildToolServer(opts.ctx, state)

  let turns = 0
  let stoppedBecause = 'end'
  let costUsd: number | undefined

  for await (const message of query({
    prompt: opts.task,
    options: {
      systemPrompt: system,
      model: 'claude-opus-4-8',
      mcpServers: { [SERVER_NAME]: server },
      // Pre-approve our tools; strip every built-in so the brain has no path to
      // the filesystem except through the envelope-backed MCP tools.
      allowedTools: [`mcp__${SERVER_NAME}__*`],
      tools: [],
      permissionMode: 'bypassPermissions',
      // Don't load the user's global CLAUDE.md / settings — the brief above is
      // the only instruction set the agent runs under.
      settingSources: [],
      maxTurns: opts.maxTurns,
    },
  })) {
    if (message.type === 'assistant') {
      turns++
      for (const block of message.message.content) {
        if (block.type === 'text') process.stdout.write(block.text)
        else if (block.type === 'tool_use') process.stdout.write(`\n  · ${block.name}\n`)
      }
    } else if (message.type === 'result') {
      stoppedBecause = message.subtype ?? 'result'
      costUsd = message.total_cost_usd
    }
  }

  return { commits: state.commits, established: state.established, turns, stoppedBecause, costUsd }
}
