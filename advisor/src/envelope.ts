// The single seam between the untrusted brain and the trusted core.
//
// Every effect the brain wants — establishing a workspace, staging a write,
// verifying-and-committing a changeset — crosses this boundary by shelling out to
// the `envelope` binary. This module cannot widen what the envelope allows; it can
// only ask, and report back the verdict the envelope returned. Reach is decided by
// the envelope under the clearance the caller names; verification is the outcome's
// own build, run inside the envelope. The brain supplies none of what it is judged
// by.

import { spawn } from 'node:child_process'

/** The verdict the trusted core returns for one request. */
export type Verdict =
  | { outcome: 'established'; detail: string }
  | { outcome: 'begun' }
  | { outcome: 'staged'; path: string }
  | { outcome: 'committed'; path: string; commit: string }
  | { outcome: 'build_failed'; path: string; reason: string; detail: string }
  | { outcome: 'rolled_back'; path: string; reason: string; detail: string }
  | { outcome: 'rejected'; violations: { invariant: string; reason: string }[] }
  | { outcome: 'refused'; path?: string; reason: string }
  | { outcome: 'reset' }
  | { outcome: 'clearance'; clearance: Clearance }
  | { outcome: 'clearance_set'; clearance: Clearance }
  | { outcome: 'error'; reason: string }

/** The reach clearance, chosen by lifecycle stage — never by the brain. */
export type Clearance = 'genesis' | 'maintenance'

/** Run the envelope binary with args and optional stdin; resolve its verdict. */
function runEnvelope(bin: string, args: string[], stdin?: string): Promise<Verdict> {
  return new Promise((resolve) => {
    const child = spawn(bin, args, { stdio: ['pipe', 'pipe', 'inherit'] })

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

    if (stdin !== undefined) child.stdin.write(stdin)
    child.stdin.end()
  })
}

/** Establish a workspace baseline from a starting-point (trusted setup). */
export function establishWorkspace(opts: {
  bin: string
  workspace: string
  mode: 'empty' | 'clone'
  source?: string
}): Promise<Verdict> {
  const args = ['establish', '--repo', opts.workspace, '--mode', opts.mode]
  if (opts.mode === 'clone' && opts.source) args.push('--source', opts.source)
  return runEnvelope(opts.bin, args)
}

/**
 * Stage one proposed write into the open changeset. Reach clearance is no
 * longer named by this call — the envelope reads it from the repo's own
 * persistent stamp (`.git/envelope-clearance`), set by `establish` and flipped
 * only by the operator via `envelope clearance --set`.
 */
export function stageWrite(opts: {
  bin: string
  repo: string
  path: string
  content: string
}): Promise<Verdict> {
  return runEnvelope(opts.bin, ['stage', '--repo', opts.repo, '--path', opts.path], opts.content)
}

/** Read the repo's persistent clearance stamp — never a value the brain names. */
export function readClearance(opts: { bin: string; repo: string }): Promise<Verdict> {
  return runEnvelope(opts.bin, ['clearance', '--repo', opts.repo])
}

/** Close the changeset: verify with the outcome's build, commit-all or report. */
export function commitChangeset(opts: {
  bin: string
  repo: string
  intent: string
}): Promise<Verdict> {
  return runEnvelope(opts.bin, ['commit', '--repo', opts.repo, '--intent', opts.intent])
}

/** Abandon an open changeset: reset the tree to the clean baseline. */
export function resetChangeset(opts: { bin: string; repo: string }): Promise<Verdict> {
  return runEnvelope(opts.bin, ['reset', '--repo', opts.repo])
}

/**
 * Refresh dependency resolution with NO `package.json` change (ADR 0009): the
 * pure-transitive case (`npm audit fix` within existing ranges, or a plain
 * re-resolve). The brain proposes the operation; the envelope computes the
 * lockfile — never the brain, and never by writing `package-lock.json` itself
 * (that write is refused under any clearance; see `invariants::reach`). Stages
 * the recomputed lockfile into the current changeset; does not commit.
 */
export function refreshDependencies(opts: {
  bin: string
  repo: string
  auditFix?: boolean
}): Promise<Verdict> {
  const args = ['refresh-deps', '--repo', opts.repo]
  if (opts.auditFix) args.push('--audit-fix')
  return runEnvelope(opts.bin, args)
}

/** A short, human-readable line describing a verdict — for logs and tool results. */
export function describeVerdict(v: Verdict): string {
  switch (v.outcome) {
    case 'established':
      return `ESTABLISHED — ${v.detail}`
    case 'begun':
      return 'BEGUN'
    case 'staged':
      return `STAGED ${v.path}`
    case 'committed':
      return `COMMITTED ${v.path || '(changeset)'} @ ${v.commit}`
    case 'build_failed':
      return `BUILD_FAILED — staged tree kept; fix and re-commit`
    case 'rolled_back':
      return `ROLLED_BACK ${v.path} — ${v.reason}`
    case 'rejected':
      return `REJECTED — ${v.violations.map((x) => `[${x.invariant}] ${x.reason}`).join('; ')}`
    case 'refused':
      return `REFUSED — ${v.reason}`
    case 'reset':
      return 'RESET'
    case 'clearance':
      return `CLEARANCE ${v.clearance}`
    case 'clearance_set':
      return `CLEARANCE_SET ${v.clearance}`
    case 'error':
      return `ERROR — ${v.reason}`
  }
}
