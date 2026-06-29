// The brain's tool surface.
//
// Reads (`read_file`, `list_dir`) are unrestricted in intent but confined to the
// repository — reading is safe, so the brain may explore freely to make good
// edits. The only way to *change* anything is `propose_write`, which hands the
// proposal to the trusted core and returns its verdict verbatim. The brain never
// touches the filesystem directly.

import { readFile, readdir, stat, appendFile } from 'node:fs/promises'
import path from 'node:path'
import type Anthropic from '@anthropic-ai/sdk'
import { adjudicateWrite, type Verdict } from './envelope.js'

const MAX_READ_BYTES = 200_000

export interface ToolContext {
  /** Absolute path to the governed repository. */
  repo: string
  /** Path to the compiled envelope binary. */
  envelopeBin: string
  /** Where to append the brain's own change journal (JSONL). */
  auditPath: string
}

export const tools: Anthropic.Tool[] = [
  {
    name: 'list_dir',
    description:
      'List the entries (files and directories) under a repository-relative directory. Use it to explore the codebase before editing.',
    input_schema: {
      type: 'object',
      properties: {
        path: {
          type: 'string',
          description: 'Repository-relative directory, e.g. "src/pages". Use "." for the root.',
        },
      },
      required: ['path'],
    },
  },
  {
    name: 'read_file',
    description: 'Read a UTF-8 text file from the repository, by repository-relative path.',
    input_schema: {
      type: 'object',
      properties: {
        path: { type: 'string', description: 'Repository-relative file path, e.g. "src/lib/api.ts".' },
      },
      required: ['path'],
    },
  },
  {
    name: 'propose_write',
    description:
      'Propose writing a file (creating or replacing it). The change is NOT applied by you — it is submitted to the envelope, which checks it is within the write allowlist, applies it, runs the project build to verify it, and then commits it or rolls it back. The returned verdict is final: "committed" (landed as a git commit), "rejected" (outside your reach — pick an allowed path), "rolled_back" (broke the build — fix and re-propose), or "refused" (a precondition failed). Make small, correct, self-contained changes that keep the build green.',
    input_schema: {
      type: 'object',
      properties: {
        path: {
          type: 'string',
          description: 'Repository-relative path to write. Must fall within the write allowlist.',
        },
        content: { type: 'string', description: 'The full file contents.' },
        rationale: {
          type: 'string',
          description: 'One concise line describing the change; used as the commit message.',
        },
      },
      required: ['path', 'content', 'rationale'],
    },
  },
]

/** Resolve a repo-relative path and confine it to the repo; null if it escapes. */
function confine(repo: string, rel: string): string | null {
  const abs = path.resolve(repo, rel)
  const within = path.relative(repo, abs)
  if (within === '') return abs
  if (within.startsWith('..') || path.isAbsolute(within)) return null
  return abs
}

/** Execute one tool call and return the string the model will see as the result. */
export async function runTool(
  ctx: ToolContext,
  name: string,
  input: unknown,
): Promise<{ result: string; verdict?: Verdict }> {
  const args = (input ?? {}) as Record<string, unknown>

  if (name === 'list_dir') {
    const rel = String(args.path ?? '.')
    const abs = confine(ctx.repo, rel)
    if (!abs) return { result: `error: "${rel}" is outside the repository` }
    try {
      const entries = await readdir(abs, { withFileTypes: true })
      const lines = entries
        .filter((e) => !e.name.startsWith('.git'))
        .map((e) => (e.isDirectory() ? `${e.name}/` : e.name))
        .sort()
      return { result: lines.length ? lines.join('\n') : '(empty)' }
    } catch (e) {
      return { result: `error: ${(e as Error).message}` }
    }
  }

  if (name === 'read_file') {
    const rel = String(args.path ?? '')
    const abs = confine(ctx.repo, rel)
    if (!abs) return { result: `error: "${rel}" is outside the repository` }
    try {
      const info = await stat(abs)
      if (info.size > MAX_READ_BYTES) {
        return { result: `error: "${rel}" is too large (${info.size} bytes)` }
      }
      return { result: await readFile(abs, 'utf8') }
    } catch (e) {
      return { result: `error: ${(e as Error).message}` }
    }
  }

  if (name === 'propose_write') {
    const rel = String(args.path ?? '')
    const content = String(args.content ?? '')
    const intent = String(args.rationale ?? 'change').replace(/\s+/g, ' ').trim()
    const verdict = await adjudicateWrite({
      bin: ctx.envelopeBin,
      repo: ctx.repo,
      path: rel,
      intent,
      content,
    })
    await appendFile(
      ctx.auditPath,
      JSON.stringify({ at: new Date().toISOString(), path: rel, intent, verdict }) + '\n',
    )
    return { result: JSON.stringify(verdict), verdict }
  }

  return { result: `error: unknown tool "${name}"` }
}
