// `autopilot run` — the untrusted brain builds and maintains the project's outcome
// under the envelope.
//
// The only input is the project's observation ledger — a directory of numbered,
// immutable records (observations/NNNN-*.md), mirroring docs/adr/. Running READS
// the ledger and acts on it; it never writes to it. Filing an observation is its
// own act (a reviewable ledger write) — `autopilot observe`, or drop a record
// directly. See docs/adr/0006.
//
// The project is where the command is run (docs/adr/0007); everything
// project-scoped comes from it. Config via env is system-level only (all optional):
//   ENVELOPE_BIN  the compiled trusted core  (default: envelope/target/debug/envelope)
//   MAX_TURNS     loop iteration cap         (default: 60)

import { access, constants } from 'node:fs/promises'
import path from 'node:path'
import { fileURLToPath } from 'node:url'
import { runLoop } from './loop.js'
import { stageWrite, describeVerdict } from './envelope.js'
import { readObservations, renderObservations } from './observations.js'
import { currentProject } from './project.js'

const here = path.dirname(fileURLToPath(import.meta.url))
// advisor/src -> advisor -> the system repo root (which holds envelope/ and advisor/)
const systemRoot = path.resolve(here, '..', '..')

function envPath(name: string, fallback: string): string {
  const v = process.env[name]
  return v ? path.resolve(v) : fallback
}

async function exists(p: string): Promise<boolean> {
  try {
    await access(p, constants.F_OK)
    return true
  } catch {
    return false
  }
}

export async function run(argv: string[]): Promise<void> {
  const dryRun = argv.includes('--dry-run')

  const project = await currentProject()
  const ctx = {
    repo: project.workspace,
    sources: project.sources,
    envelopeBin: envPath('ENVELOPE_BIN', path.join(systemRoot, 'envelope', 'target', 'debug', 'envelope')),
    clearance: project.clearance,
    auditPath: project.auditPath,
  }
  const observationsDir = project.observationsDir
  const maxTurns = Number(process.env.MAX_TURNS ?? 60)

  // Preflight: the seam must exist before we let the brain near it.
  if (!(await exists(ctx.envelopeBin))) {
    console.error(`envelope binary not found at ${ctx.envelopeBin}`)
    console.error('build it first:  (cd envelope && cargo build)')
    process.exit(1)
  }

  const sourceNames = Object.keys(ctx.sources)
  console.error(`project    ${project.dir}`)
  console.error(`workspace  ${ctx.repo}`)
  console.error(`ledger     ${observationsDir}`)
  console.error(`sources    ${sourceNames.length ? sourceNames.map((n) => `${n} -> ${ctx.sources[n]}`).join(', ') : '(none)'}`)
  console.error(`clearance  ${ctx.clearance}`)
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
      clearance: ctx.clearance,
      content: '// probe',
    })
    console.error(describeVerdict(verdict))
    const ok = verdict.outcome === 'rejected'
    console.error(ok ? '\nseam OK: the trusted core refused the out-of-reach write.' : '\nunexpected verdict.')
    process.exit(ok ? 0 : 1)
  }

  // Running only reads the ledger — filing is a separate, deliberate act
  // (`autopilot observe`, or drop a record). The ledger stays the single, durable ask.
  const observations = await readObservations(observationsDir)
  if (observations.length === 0) {
    console.error(`\nno observations to act on. file one first:`)
    console.error(`  autopilot observe "…a need…"   (or add a record to ${observationsDir})`)
    process.exit(2)
  }

  for (const [name, root] of Object.entries(ctx.sources)) {
    if (!(await exists(root))) {
      console.error(`\nobservation source "${name}" not found at ${root}`)
      console.error(`fix "sources" in ${path.join(project.dir, 'project.json')} (empty = greenfield run)`)
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
