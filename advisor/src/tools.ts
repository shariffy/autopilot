// The brain's tool surface, as an in-process MCP server for the Claude Agent SDK.
//
// The agent runs under the Agent SDK (Claude Code as a library), so it would
// normally have built-in Read/Write/Bash tools. We strip those (loop.ts sets
// `tools: []`) so the ONLY way the brain can touch anything is through the tools
// defined here — every effect still crosses the envelope seam. Reads are confined
// to the workspace and the named observation sources; the sole way to *change*
// anything is establish + stage + commit, adjudicated by the envelope under the
// clearance in force. The brain never touches the filesystem directly and never
// decides its own reach.

import { readFile, readdir, stat, appendFile } from 'node:fs/promises'
import path from 'node:path'
import { tool, createSdkMcpServer } from '@anthropic-ai/claude-agent-sdk'
import { z } from 'zod'
import {
  establishWorkspace,
  stageWrite,
  commitChangeset,
  refreshDependencies,
  describeVerdict,
  type Clearance,
  type Verdict,
} from './envelope.js'

const MAX_READ_BYTES = 200_000

export interface ToolContext {
  /** Absolute path to the workspace being built (the outcome). May not exist yet. */
  repo: string
  /** Named, read-only observation sources the brain may observe (name → absolute root). */
  sources: Record<string, string>
  /** Path to the compiled envelope binary. */
  envelopeBin: string
  /** The reach clearance in force, set by lifecycle stage — not by the brain. */
  clearance: Clearance
  /** Where to append the brain's own change journal (JSONL). */
  auditPath: string
}

/** Outcomes the loop wants to observe, written by the tool handlers as they run. */
export interface LoopState {
  commits: string[]
  established: boolean
}

/** The MCP server name; tools are exposed to the agent as mcp__autopilot__<tool>. */
export const SERVER_NAME = 'autopilot'

/** A text-only tool result, the shape the Agent SDK expects. */
function text(s: string) {
  return { content: [{ type: 'text' as const, text: s }] }
}

/** Resolve a repo-relative path and confine it to `root`; null if it escapes. */
function confine(root: string, rel: string): string | null {
  const abs = path.resolve(root, rel)
  const within = path.relative(root, abs)
  if (within === '') return abs
  if (within.startsWith('..') || path.isAbsolute(within)) return null
  return abs
}

async function listDir(root: string, rel: string): Promise<string> {
  const abs = confine(root, rel)
  if (!abs) return `error: "${rel}" is outside the root`
  try {
    const entries = await readdir(abs, { withFileTypes: true })
    const lines = entries
      .filter((e) => e.name !== '.git')
      .map((e) => (e.isDirectory() ? `${e.name}/` : e.name))
      .sort()
    return lines.length ? lines.join('\n') : '(empty)'
  } catch (e) {
    return `error: ${(e as Error).message}`
  }
}

async function readTextFile(root: string, rel: string): Promise<string> {
  const abs = confine(root, rel)
  if (!abs) return `error: "${rel}" is outside the root`
  try {
    const info = await stat(abs)
    if (info.size > MAX_READ_BYTES) return `error: "${rel}" is too large (${info.size} bytes)`
    return await readFile(abs, 'utf8')
  } catch (e) {
    return `error: ${(e as Error).message}`
  }
}

async function journal(ctx: ToolContext, entry: Record<string, unknown>): Promise<void> {
  await appendFile(ctx.auditPath, JSON.stringify({ at: new Date().toISOString(), ...entry }) + '\n')
}

/** Record and log a changeset verdict, then hand its JSON back to the brain. */
function report(state: LoopState, verdict: Verdict) {
  process.stdout.write(`\n  ↳ ${describeVerdict(verdict)}\n`)
  if (verdict.outcome === 'committed') state.commits.push(verdict.commit)
  if (verdict.outcome === 'established') state.established = true
  return text(JSON.stringify(verdict))
}

/**
 * Build the in-process MCP server holding the brain's tools, plus the reach
 * decided by the envelope. The observation source names are configuration woven
 * into the descriptions — the surface knows none of them by name.
 */
export function buildToolServer(ctx: ToolContext, state: LoopState) {
  const names = Object.keys(ctx.sources)
  const sourceList = names.length ? names.join(', ') : '(none configured)'

  const sourceRoot = (name: string): string | null => ctx.sources[name] ?? null
  const unknownSource = (name: string) =>
    text(`error: unknown source "${name}". Available: ${names.join(', ') || '(none)'}`)

  const listSource = tool(
    'list_source',
    `List entries under a directory of a read-only OBSERVATION SOURCE. Use it to observe what already exists. Available sources: ${sourceList}. Use "." for a source's root.`,
    { source: z.string().describe(`Which source. One of: ${sourceList}.`), path: z.string() },
    async (args) => {
      const root = sourceRoot(args.source)
      return root ? text(await listDir(root, args.path || '.')) : unknownSource(args.source)
    },
    { annotations: { readOnlyHint: true } },
  )

  const readSource = tool(
    'read_source',
    `Read a UTF-8 text file from a read-only OBSERVATION SOURCE, by source-relative path. Observing a source is how you learn the real domain. Available sources: ${sourceList}.`,
    { source: z.string().describe(`Which source. One of: ${sourceList}.`), path: z.string() },
    async (args) => {
      const root = sourceRoot(args.source)
      return root ? text(await readTextFile(root, args.path)) : unknownSource(args.source)
    },
    { annotations: { readOnlyHint: true } },
  )

  const listDirTool = tool(
    'list_dir',
    'List entries under a directory of YOUR workspace (the outcome you are building). Use "." for the root. Empty until you establish it.',
    { path: z.string() },
    async (args) => text(await listDir(ctx.repo, args.path || '.')),
    { annotations: { readOnlyHint: true } },
  )

  const readFileTool = tool(
    'read_file',
    'Read a UTF-8 text file from YOUR workspace, by workspace-relative path.',
    { path: z.string() },
    async (args) => text(await readTextFile(ctx.repo, args.path)),
    { annotations: { readOnlyHint: true } },
  )

  const establishTool = tool(
    'establish_workspace',
    `Create the baseline of your workspace, once, before staging anything. "empty" initialises a greenfield git repo. "clone" adopts a clone of an observation source named by "source" (the envelope clones it read-only and confirms it builds green before adopting — if it does not build, adoption is refused and you should choose another approach). Available sources to clone: ${sourceList}.`,
    {
      mode: z.enum(['empty', 'clone']),
      source: z.string().optional().describe(`Required when mode is "clone": which source to adopt. One of: ${sourceList}.`),
      rationale: z.string().describe('One line: why this starting point.'),
    },
    async (args) => {
      if (args.mode === 'clone') {
        const root = args.source ? sourceRoot(args.source) : null
        if (!root) return unknownSource(args.source ?? '')
        const verdict = await establishWorkspace({
          bin: ctx.envelopeBin,
          workspace: ctx.repo,
          mode: 'clone',
          source: root,
        })
        await journal(ctx, { action: 'establish', mode: 'clone', source: args.source, verdict })
        return report(state, verdict)
      }
      const verdict = await establishWorkspace({ bin: ctx.envelopeBin, workspace: ctx.repo, mode: 'empty' })
      await journal(ctx, { action: 'establish', mode: 'empty', verdict })
      return report(state, verdict)
    },
  )

  const proposeWrite = tool(
    'propose_write',
    'Stage a file (create or replace) into the current changeset. The write is checked against your reach by the envelope and applied to the workspace tree, but NOT yet verified or committed. Stage all the files of a coherent change, then call commit_changeset. A path outside your reach is REJECTED — pick an allowed path.',
    {
      path: z.string().describe('Workspace-relative path to write.'),
      content: z.string().describe('The full file contents.'),
      rationale: z.string().describe('One concise line about this file.'),
    },
    async (args) => {
      const intent = args.rationale.replace(/\s+/g, ' ').trim()
      const verdict = await stageWrite({
        bin: ctx.envelopeBin,
        repo: ctx.repo,
        path: args.path,
        clearance: ctx.clearance,
        content: args.content,
      })
      await journal(ctx, { action: 'stage', path: args.path, intent, verdict })
      return report(state, verdict)
    },
  )

  const refreshDeps = tool(
    'refresh_dependencies',
    'Refresh dependency resolution for the current changeset WITHOUT changing package.json — the pure-transitive case (ADR 0009): a fix entirely inside the ranges package.json already allows, or just re-resolving. You propose the operation; the ENVELOPE computes the lockfile — you can never write package-lock.json (or pnpm-lock.yaml/yarn.lock) yourself, under any clearance (propose_write refuses it). Set audit_fix to run `npm audit fix` within existing ranges; otherwise it is a plain re-resolve. If a fix needs a NEW range or a new dependency, edit package.json with propose_write instead — the envelope resolves the lockfile for you at commit time. Stages the recomputed lockfile into the current changeset; call commit_changeset next to verify and land it.',
    {
      audit_fix: z
        .boolean()
        .optional()
        .describe(
          'If true, run `npm audit fix --package-lock-only` (fixes within existing ranges only). Defaults to a plain re-resolve.',
        ),
    },
    async (args) => {
      const verdict = await refreshDependencies({
        bin: ctx.envelopeBin,
        repo: ctx.repo,
        auditFix: args.audit_fix,
      })
      await journal(ctx, { action: 'refresh_dependencies', auditFix: args.audit_fix ?? false, verdict })
      return report(state, verdict)
    },
  )

  const commitTool = tool(
    'commit_changeset',
    "Verify and commit everything staged so far. The envelope runs the outcome's own build over the whole staged tree. If it passes, the changeset lands as one commit (COMMITTED). If it fails, nothing commits and your staged files are kept (BUILD_FAILED) — read the error, stage fixes, and call commit_changeset again. Call this only when you believe the staged set should build.",
    { summary: z.string().describe('Commit message for the whole changeset.') },
    async (args) => {
      const intent = args.summary.replace(/\s+/g, ' ').trim()
      const verdict = await commitChangeset({ bin: ctx.envelopeBin, repo: ctx.repo, intent })
      await journal(ctx, { action: 'commit', intent, verdict })
      return report(state, verdict)
    },
  )

  const server = createSdkMcpServer({
    name: SERVER_NAME,
    version: '0.1.0',
    tools: [
      listSource,
      readSource,
      listDirTool,
      readFileTool,
      establishTool,
      proposeWrite,
      refreshDeps,
      commitTool,
    ],
  })

  return { server }
}
