// The single seam between the untrusted brain and the trusted core.
//
// Every write the brain wants to make crosses this boundary: it shells out to the
// `envelope` binary, which decides (reach), enacts, verifies with the repo's own
// build, and commits or reverts. This module cannot widen what the envelope
// allows — it can only ask, and report back the verdict the envelope returned.

import { spawn } from 'node:child_process'

/** The verdict the trusted core returns for one proposed write. */
export type Verdict =
  | { outcome: 'committed'; path: string; commit: string }
  | { outcome: 'rolled_back'; path: string; reason: string; detail: string }
  | { outcome: 'rejected'; violations: { invariant: string; reason: string }[] }
  | { outcome: 'refused'; path?: string; reason: string }
  | { outcome: 'error'; reason: string }

export interface AdjudicateOptions {
  /** Path to the compiled `envelope` binary. */
  bin: string
  /** Absolute path to the governed repository (roli-admin). */
  repo: string
  /** Repository-relative path the brain wants to write. */
  path: string
  /** One-line rationale; becomes the commit message on success. */
  intent: string
  /** The full proposed file body. */
  content: string
}

/**
 * Submit one proposed write to the trusted core and resolve with its verdict.
 * The file body goes over stdin so arbitrary source needs no escaping; the
 * verdict comes back as a single JSON line on stdout.
 */
export function adjudicateWrite(opts: AdjudicateOptions): Promise<Verdict> {
  return new Promise((resolve) => {
    const child = spawn(
      opts.bin,
      ['adjudicate', '--repo', opts.repo, '--path', opts.path, '--intent', opts.intent],
      { stdio: ['pipe', 'pipe', 'inherit'] },
    )

    let stdout = ''
    child.stdout.setEncoding('utf8')
    child.stdout.on('data', (chunk: string) => {
      stdout += chunk
    })

    child.on('error', (err) => {
      resolve({ outcome: 'error', reason: `could not run envelope: ${err.message}` })
    })

    child.on('close', () => {
      const line = stdout.trim().split('\n').filter(Boolean).pop()
      if (!line) {
        resolve({ outcome: 'error', reason: 'envelope produced no verdict' })
        return
      }
      try {
        resolve(JSON.parse(line) as Verdict)
      } catch {
        resolve({ outcome: 'error', reason: `unparseable verdict: ${line}` })
      }
    })

    child.stdin.write(opts.content)
    child.stdin.end()
  })
}

/** A short, human-readable line describing a verdict — for logs and tool results. */
export function describeVerdict(v: Verdict): string {
  switch (v.outcome) {
    case 'committed':
      return `COMMITTED ${v.path} @ ${v.commit}`
    case 'rolled_back':
      return `ROLLED_BACK ${v.path} — ${v.reason}`
    case 'rejected':
      return `REJECTED — ${v.violations.map((x) => `[${x.invariant}] ${x.reason}`).join('; ')}`
    case 'refused':
      return `REFUSED — ${v.reason}`
    case 'error':
      return `ERROR — ${v.reason}`
  }
}
