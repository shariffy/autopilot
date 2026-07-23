// The observation ledger: the system's inputs, as a series of immutable records.
//
// An observation is a uniform event — a statement of what has been noticed or
// wanted, tagged with its source (a human, a monitor, a test). Every origin enters
// through this one door and is treated identically; the source is metadata, not a
// branch. Observations accumulate as numbered, immutable files under a directory —
// the same form as the decision-record series they mirror (docs/adr/NNNN). A source
// files an observation by dropping the next file; nothing edits an existing one. An
// observation only STEERS — it carries no authority, so the ledger may be appended
// by anyone or anything without widening what the agent can do. See docs/adr/0006.

import { readFile, writeFile, readdir, mkdir } from 'node:fs/promises'
import path from 'node:path'

export interface Observation {
  /** Sequence number, from the filename (NNNN-slug.md). */
  id: number
  /** Where it came from: "human", "cloudwatch", "test", … — metadata, not a branch. */
  source: string
  /** When it was filed. */
  at?: string
  /** What was noticed or wanted. */
  body: string
}

const FILE = /^(\d+)-.*\.md$/
const SOURCE = /^-\s*\*\*Source:\*\*\s*(.+?)\s*$/m
const FILED = /^-\s*\*\*Filed:\*\*\s*(.+?)\s*$/m

/** Read the ledger into observations, oldest (lowest id) first. Missing dir → none. */
export async function readObservations(dir: string): Promise<Observation[]> {
  let names: string[]
  try {
    names = await readdir(dir)
  } catch {
    return []
  }
  const files = names
    .map((name) => ({ name, m: FILE.exec(name) }))
    .filter((x): x is { name: string; m: RegExpExecArray } => x.m !== null)
    .sort((a, b) => Number(a.m[1]) - Number(b.m[1]))

  const out: Observation[] = []
  for (const f of files) {
    const text = await readFile(path.join(dir, f.name), 'utf8')
    out.push({
      id: Number(f.m[1]),
      source: SOURCE.exec(text)?.[1] ?? 'unknown',
      at: FILED.exec(text)?.[1],
      body: bodyOf(text),
    })
  }
  return out
}

/** File one observation as the next numbered record; creates the dir if absent. */
export async function appendObservation(dir: string, obs: Omit<Observation, 'id'>): Promise<number> {
  await mkdir(dir, { recursive: true })
  const existing = await readObservations(dir)
  const id = (existing.at(-1)?.id ?? 0) + 1
  const at = obs.at ?? new Date().toISOString()
  const name = `${String(id).padStart(4, '0')}-${slug(obs.body)}.md`
  const record =
    `# Observation ${id}\n\n` +
    `- **Source:** ${obs.source}\n` +
    `- **Filed:** ${at}\n\n` +
    `${obs.body.trim()}\n`
  await writeFile(path.join(dir, name), record)
  return id
}

/** Render the ledger as the agent's standing brief — the ask it reasons from. */
export function renderObservations(obs: Observation[]): string {
  const n = obs.length
  const head = `You are working from an observation ledger. ${
    n === 1 ? 'There is 1 observation' : `There are ${n} observations`
  }, oldest first. Each is what someone or something noticed or wants; treat them together as the standing ask:`
  const body = obs
    .map((o) => `Observation ${o.id} [${o.source}${o.at ? ` · ${o.at}` : ''}]\n${o.body}`)
    .join('\n\n')
  return `${head}\n\n${body}`
}

/** Everything that is not the title line or a metadata bullet, trimmed. */
function bodyOf(text: string): string {
  return text
    .split('\n')
    .filter((l) => !/^#\s/.test(l) && !/^-\s*\*\*(Source|Filed):\*\*/.test(l))
    .join('\n')
    .trim()
}

/** A short filename slug from the observation body. */
function slug(body: string): string {
  const s = body
    .toLowerCase()
    .replace(/[^a-z0-9]+/g, '-')
    .replace(/^-+|-+$/g, '')
    .split('-')
    .slice(0, 6)
    .join('-')
  return s || 'observation'
}
