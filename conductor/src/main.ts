// Entry point for the conductor — the untrusted brain that builds and maintains an
// outcome under the envelope.
//
// The only input is the observation ledger — a directory of numbered, immutable
// records (observations/NNNN-*.md), mirroring docs/adr/. Running READS the ledger
// and acts on it; it never writes to it. Filing an observation is its own act (a
// reviewable ledger write) — use `npm run observe`, or drop a record directly. See
// docs/adr/0006.
//
//   npm start                        # act on the observation ledger as it stands
//   npm start -- --dry-run           # probe the envelope seam without calling Claude
//   npm run observe -- "…a need…"    # file a new observation (does NOT run the agent)
//
// Config via env (all optional):
//   WORKSPACE        the outcome to build/maintain     (default: ../roli-admin-genesis)
//   OBSERVATIONS     the observation ledger directory   (default: ./observations)
//   OBSERVE_SOURCES  read-only sources to observe, as   (default: admin-roli=../../admin.roli.com)
//                    comma-separated name=path pairs; may be empty for a pure greenfield run
//   CHARTER          reach charter: genesis|maintenance (default: genesis)
//   ENVELOPE_BIN     the compiled trusted core          (default: envelope/target/debug/envelope)
//   CONDUCTOR_AUDIT  change journal (JSONL)             (default: ./conductor-audit.jsonl)
//   MAX_TURNS        loop iteration cap                 (default: 60)

import { access, constants } from 'node:fs/promises'
import path from 'node:path'
import { fileURLToPath } from 'node:url'
import { runLoop } from './loop.js'
import { stageWrite, describeVerdict, type Charter } from './envelope.js'
import { readObservations, renderObservations } from './observations.js'

const here = path.dirname(fileURLToPath(import.meta.url))
// conductor/src -> conductor -> the system repo root (which holds envelope/ and conductor/)
const systemRoot = path.resolve(here, '..', '..')

function envPath(name: string, fallback: string): string {
  const v = process.env[name]
  return v ? path.resolve(v) : fallback
}

/**
 * Parse the observation sources: `name=path,name=path`. Which sources exist — and
 * that one happens to be a git repo worth cloning — is configuration, not baked in.
 * An empty setting means no sources (a pure greenfield run).
 */
function parseSources(spec: string): Record<string, string> {
  const out: Record<string, string> = {}
  for (const pair of spec.split(',').map((s) => s.trim()).filter(Boolean)) {
    const eq = pair.indexOf('=')
    if (eq < 0) throw new Error(`bad OBSERVE_SOURCES entry "${pair}" (want name=path)`)
    const name = pair.slice(0, eq).trim()
    const p = pair.slice(eq + 1).trim()
    if (!name || !p) throw new Error(`bad OBSERVE_SOURCES entry "${pair}" (want name=path)`)
    out[name] = path.resolve(p)
  }
  return out
}

async function exists(p: string): Promise<boolean> {
  try {
    await access(p, constants.F_OK)
    return true
  } catch {
    return false
  }
}

async function main() {
  const argv = process.argv.slice(2)
  const dryRun = argv.includes('--dry-run')

  const charter = (process.env.CHARTER ?? 'genesis') as Charter
  const defaultSources = `admin-roli=${path.resolve(systemRoot, '..', '..', 'admin.roli.com')}`
  const ctx = {
    // The outcome the agent will establish and build — a fresh sibling, not the
    // existing roli-admin, so the genesis is genuine.
    repo: envPath('WORKSPACE', path.resolve(systemRoot, '..', 'roli-admin-genesis')),
    // Named read-only observation sources. One is the predecessor from the second
    // observation; there could be zero, or several, and none is privileged.
    sources: parseSources(process.env.OBSERVE_SOURCES ?? defaultSources),
    envelopeBin: envPath('ENVELOPE_BIN', path.join(systemRoot, 'envelope', 'target', 'debug', 'envelope')),
    charter,
    auditPath: envPath('CONDUCTOR_AUDIT', path.join(here, '..', 'conductor-audit.jsonl')),
  }
  const observationsDir = envPath('OBSERVATIONS', path.join(here, '..', 'observations'))
  const maxTurns = Number(process.env.MAX_TURNS ?? 60)

  // Preflight: the seam must exist before we let the brain near it.
  if (!(await exists(ctx.envelopeBin))) {
    console.error(`envelope binary not found at ${ctx.envelopeBin}`)
    console.error('build it first:  (cd envelope && cargo build)')
    process.exit(1)
  }

  const sourceNames = Object.keys(ctx.sources)
  console.error(`workspace  ${ctx.repo}`)
  console.error(`ledger     ${observationsDir}`)
  console.error(`sources    ${sourceNames.length ? sourceNames.map((n) => `${n} -> ${ctx.sources[n]}`).join(', ') : '(none)'}`)
  console.error(`charter    ${ctx.charter}`)
  console.error(`envelope   ${ctx.envelopeBin}`)
  console.error(`audit      ${ctx.auditPath}`)

  if (dryRun) {
    // Exercise the brain → envelope → verdict path without the model and without
    // mutating anything: a never-writable path is rejected before it touches disk,
    // proving the seam is wired and parsed correctly. No workspace required.
    console.error('\n--dry-run: probing the envelope seam (never-zone path, mutates nothing)\n')
    const verdict = await stageWrite({
      bin: ctx.envelopeBin,
      repo: ctx.repo,
      path: 'secrets/__envelope_probe__.ts',
      charter: ctx.charter,
      content: '// probe',
    })
    console.error(describeVerdict(verdict))
    const ok = verdict.outcome === 'rejected'
    console.error(ok ? '\nseam OK: the trusted core refused the out-of-reach write.' : '\nunexpected verdict.')
    process.exit(ok ? 0 : 1)
  }

  // Running only reads the ledger — filing is a separate, deliberate act
  // (`npm run observe`, or drop a record). The ledger stays the single, durable ask.
  const observations = await readObservations(observationsDir)
  if (observations.length === 0) {
    console.error(`\nno observations to act on. file one first:`)
    console.error(`  npm run observe -- "…a need…"   (or add a record to ${observationsDir})`)
    process.exit(2)
  }

  for (const [name, root] of Object.entries(ctx.sources)) {
    if (!(await exists(root))) {
      console.error(`\nobservation source "${name}" not found at ${root}`)
      console.error('fix OBSERVE_SOURCES (name=path,...) or unset it for a greenfield run')
      process.exit(1)
    }
  }
  // Auth is handled by the Agent SDK: your Claude Code login (~/.claude) — i.e.
  // your subscription — or ANTHROPIC_API_KEY if that is set instead. No hard check
  // here; if no credential resolves, the SDK reports it when the run starts.
  const brief = renderObservations(observations)
  console.error(`\nacting on ${observations.length} observation(s) from the ledger\n`)
  const result = await runLoop({ task: brief, ctx, maxTurns })

  console.error('\n══════════════════════════════════════════════════════════')
  console.error(`stopped: ${result.stoppedBecause} after ${result.turns} turn(s)`)
  if (result.costUsd !== undefined) console.error(`cost: $${result.costUsd.toFixed(4)}`)
  console.error(`workspace established: ${result.established ? 'yes' : 'no'}`)
  if (result.commits.length) {
    console.error(`landed ${result.commits.length} changeset commit(s):`)
    for (const c of result.commits) console.error(`  • ${c}`)
  } else {
    console.error('no changeset committed.')
  }
  console.error(`journal: ${ctx.auditPath}`)
}

main().catch((e) => {
  console.error(e)
  process.exit(1)
})
