// The brain's tool surface.
//
// Reads are unrestricted in intent but confined to roots: the workspace it is
// building (`list_dir`/`read_file`) and a set of named, read-only OBSERVATION
// SOURCES it may observe (`list_source`/`read_source`). An observation source is
// just a readable root the conductor mounted — a predecessor repo today, a doc or
// export tomorrow. The tool surface knows none of them by name; they are data.
// Reading changes nothing, so the brain may explore freely. The only way to
// *change* anything is to establish a workspace and stage writes into a changeset,
// then ask the envelope to verify-and-commit it. The brain never touches the
// filesystem directly, and never decides its own reach — the envelope does, under
// the charter in force.

import { readFile, readdir, stat, appendFile } from 'node:fs/promises'
import path from 'node:path'
import type Anthropic from '@anthropic-ai/sdk'
import {
  establishWorkspace,
  stageWrite,
  commitChangeset,
  type Charter,
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
  /** The reach charter in force, set by lifecycle stage — not by the brain. */
  charter: Charter
  /** Where to append the brain's own change journal (JSONL). */
  auditPath: string
}

/**
 * The tool list, built with the names of the observation sources in play so the
 * brain knows what it may observe and what it may clone from. The sources are
 * configuration, not vocabulary baked into the surface.
 */
export function buildTools(sourceNames: string[]): Anthropic.Tool[] {
  const names = sourceNames.length ? sourceNames.join(', ') : '(none configured)'
  return [
    {
      name: 'list_source',
      description:
        `List entries under a directory of a read-only OBSERVATION SOURCE. Use it to observe what already exists. Available sources: ${names}. Use "." for a source's root.`,
      input_schema: {
        type: 'object',
        properties: {
          source: { type: 'string', description: `Which source to read. One of: ${names}.` },
          path: { type: 'string', description: 'Source-relative directory.' },
        },
        required: ['source', 'path'],
      },
    },
    {
      name: 'read_source',
      description:
        `Read a UTF-8 text file from a read-only OBSERVATION SOURCE, by source-relative path. Observing a source is how you learn the real domain. Available sources: ${names}.`,
      input_schema: {
        type: 'object',
        properties: {
          source: { type: 'string', description: `Which source to read. One of: ${names}.` },
          path: { type: 'string', description: 'Source-relative file path.' },
        },
        required: ['source', 'path'],
      },
    },
    {
      name: 'list_dir',
      description:
        'List entries under a directory of YOUR workspace (the outcome you are building). Use "." for the root. Empty until you establish it.',
      input_schema: {
        type: 'object',
        properties: { path: { type: 'string', description: 'Workspace-relative directory.' } },
        required: ['path'],
      },
    },
    {
      name: 'read_file',
      description: 'Read a UTF-8 text file from YOUR workspace, by workspace-relative path.',
      input_schema: {
        type: 'object',
        properties: { path: { type: 'string', description: 'Workspace-relative file path.' } },
        required: ['path'],
      },
    },
    {
      name: 'establish_workspace',
      description:
        `Create the baseline of your workspace, once, before staging anything. "empty" initialises a greenfield git repo. "clone" adopts a clone of an observation source named by "source" (the envelope clones it read-only and confirms it builds green before adopting — if it does not build, adoption is refused and you should choose another approach). Available sources to clone: ${names}. Choose based on what you decided after observing.`,
      input_schema: {
        type: 'object',
        properties: {
          mode: { type: 'string', enum: ['empty', 'clone'] },
          source: {
            type: 'string',
            description: `Required when mode is "clone": which source to adopt. One of: ${names}.`,
          },
          rationale: { type: 'string', description: 'One line: why this starting point.' },
        },
        required: ['mode', 'rationale'],
      },
    },
    {
      name: 'propose_write',
      description:
        'Stage a file (create or replace) into the current changeset. The write is checked against your reach by the envelope and applied to the workspace tree, but NOT yet verified or committed. Stage all the files of a coherent change, then call commit_changeset. A path outside your reach is REJECTED — pick an allowed path.',
      input_schema: {
        type: 'object',
        properties: {
          path: { type: 'string', description: 'Workspace-relative path to write.' },
          content: { type: 'string', description: 'The full file contents.' },
          rationale: { type: 'string', description: 'One concise line about this file.' },
        },
        required: ['path', 'content', 'rationale'],
      },
    },
    {
      name: 'commit_changeset',
      description:
        "Verify and commit everything staged so far. The envelope runs the outcome's own build over the whole staged tree. If it passes, the changeset lands as one commit (COMMITTED). If it fails, nothing commits and your staged files are kept (BUILD_FAILED) — read the error, stage fixes, and call commit_changeset again. Call this only when you believe the staged set should build.",
      input_schema: {
        type: 'object',
        properties: {
          summary: { type: 'string', description: 'Commit message for the whole changeset.' },
        },
        required: ['summary'],
      },
    },
  ]
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

/** Look up a named observation source; null if the brain named one that isn't mounted. */
function sourceRoot(ctx: ToolContext, name: string): string | null {
  return ctx.sources[name] ?? null
}

/** Execute one tool call and return the string the model will see as the result. */
export async function runTool(
  ctx: ToolContext,
  name: string,
  input: unknown,
): Promise<{ result: string; verdict?: Verdict }> {
  const args = (input ?? {}) as Record<string, unknown>

  if (name === 'list_source' || name === 'read_source') {
    const src = String(args.source ?? '')
    const root = sourceRoot(ctx, src)
    if (!root) {
      const known = Object.keys(ctx.sources).join(', ') || '(none)'
      return { result: `error: unknown source "${src}". Available: ${known}` }
    }
    const rel = String(args.path ?? '')
    return {
      result: name === 'list_source' ? await listDir(root, rel || '.') : await readTextFile(root, rel),
    }
  }

  if (name === 'list_dir') return { result: await listDir(ctx.repo, String(args.path ?? '.')) }
  if (name === 'read_file') return { result: await readTextFile(ctx.repo, String(args.path ?? '')) }

  if (name === 'establish_workspace') {
    const mode = String(args.mode ?? '')
    if (mode === 'clone') {
      const src = String(args.source ?? '')
      const root = sourceRoot(ctx, src)
      if (!root) {
        const known = Object.keys(ctx.sources).join(', ') || '(none)'
        return { result: `error: unknown source "${src}" to clone. Available: ${known}` }
      }
      const verdict = await establishWorkspace({
        bin: ctx.envelopeBin,
        workspace: ctx.repo,
        mode: 'clone',
        source: root,
      })
      await journal(ctx, { action: 'establish', mode, source: src, verdict })
      return { result: JSON.stringify(verdict), verdict }
    }
    const verdict = await establishWorkspace({ bin: ctx.envelopeBin, workspace: ctx.repo, mode: 'empty' })
    await journal(ctx, { action: 'establish', mode: 'empty', verdict })
    return { result: JSON.stringify(verdict), verdict }
  }

  if (name === 'propose_write') {
    const rel = String(args.path ?? '')
    const content = String(args.content ?? '')
    const intent = String(args.rationale ?? 'change').replace(/\s+/g, ' ').trim()
    const verdict = await stageWrite({
      bin: ctx.envelopeBin,
      repo: ctx.repo,
      path: rel,
      charter: ctx.charter,
      content,
    })
    await journal(ctx, { action: 'stage', path: rel, intent, verdict })
    return { result: JSON.stringify(verdict), verdict }
  }

  if (name === 'commit_changeset') {
    const intent = String(args.summary ?? 'changeset').replace(/\s+/g, ' ').trim()
    const verdict = await commitChangeset({ bin: ctx.envelopeBin, repo: ctx.repo, intent })
    await journal(ctx, { action: 'commit', intent, verdict })
    return { result: JSON.stringify(verdict), verdict }
  }

  return { result: `error: unknown tool "${name}"` }
}

async function journal(ctx: ToolContext, entry: Record<string, unknown>): Promise<void> {
  await appendFile(ctx.auditPath, JSON.stringify({ at: new Date().toISOString(), ...entry }) + '\n')
}
