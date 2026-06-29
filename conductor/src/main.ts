// Entry point for the conductor — the untrusted brain that maintains roli-admin
// under the envelope.
//
//   npm start -- "Add a Badge component under src/components/ui and use it ..."
//   npm start -- --dry-run        # probe the envelope seam without calling Claude
//
// Config via env (all optional — see .env.example):
//   ROLI_ADMIN_REPO  the external outcome to maintain  (default: ../roli-admin, beside the system repo)
//   ENVELOPE_BIN     the compiled trusted core         (default: envelope/target/debug/envelope in the system repo)
//   CONDUCTOR_AUDIT  change journal (JSONL)            (default: ./conductor-audit.jsonl)
//   MAX_TURNS        loop iteration cap                (default: 30)

import { access, constants } from 'node:fs/promises'
import { spawnSync } from 'node:child_process'
import path from 'node:path'
import { fileURLToPath } from 'node:url'
import { runLoop } from './loop.js'
import { runTool, type ToolContext } from './tools.js'
import { describeVerdict } from './envelope.js'

const here = path.dirname(fileURLToPath(import.meta.url))
// conductor/src -> conductor -> the system repo root (which holds envelope/ and conductor/)
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

function gitClean(repo: string): boolean {
  const r = spawnSync('git', ['-C', repo, 'status', '--porcelain'], { encoding: 'utf8' })
  return r.status === 0 && r.stdout.trim() === ''
}

async function main() {
  const argv = process.argv.slice(2)
  const dryRun = argv.includes('--dry-run')
  const task = argv.filter((a) => !a.startsWith('--')).join(' ').trim()

  const ctx: ToolContext = {
    // The outcome is external — it lives beside the system repo, not inside it.
    repo: envPath('ROLI_ADMIN_REPO', path.resolve(systemRoot, '..', 'roli-admin')),
    envelopeBin: envPath(
      'ENVELOPE_BIN',
      path.join(systemRoot, 'envelope', 'target', 'debug', 'envelope'),
    ),
    auditPath: envPath('CONDUCTOR_AUDIT', path.join(here, '..', 'conductor-audit.jsonl')),
  }
  const maxTurns = Number(process.env.MAX_TURNS ?? 30)

  // Preflight: the seam must exist before we let the brain near it.
  if (!(await exists(ctx.envelopeBin))) {
    console.error(`envelope binary not found at ${ctx.envelopeBin}`)
    console.error('build it first:  (cd ../envelope && cargo build)')
    process.exit(1)
  }
  if (!(await exists(ctx.repo))) {
    console.error(`governed repo not found at ${ctx.repo}`)
    process.exit(1)
  }

  console.error(`repo      ${ctx.repo}`)
  console.error(`envelope  ${ctx.envelopeBin}`)
  console.error(`audit     ${ctx.auditPath}`)

  if (dryRun) {
    // Exercise the full brain → envelope → verdict path without the model and
    // without mutating anything: a forbidden path is rejected before it touches
    // disk, proving the seam is wired and parsed correctly.
    console.error('\n--dry-run: probing the envelope seam (forbidden path, mutates nothing)\n')
    const { verdict } = await runTool(ctx, 'propose_write', {
      path: 'src/api/__envelope_probe__.ts',
      content: '// probe',
      rationale: 'dry-run probe',
    })
    console.error(verdict ? describeVerdict(verdict) : 'no verdict')
    const ok = verdict?.outcome === 'rejected'
    console.error(ok ? '\nseam OK: the trusted core refused the out-of-reach write.' : '\nunexpected verdict.')
    process.exit(ok ? 0 : 1)
  }

  if (!task) {
    console.error('\nno task given. usage: npm start -- "your task"   (or --dry-run)')
    process.exit(2)
  }
  if (!process.env.ANTHROPIC_API_KEY) {
    console.error('\nANTHROPIC_API_KEY is not set.')
    process.exit(1)
  }
  if (!gitClean(ctx.repo)) {
    console.error('\nthe governed repo has uncommitted changes; commit or stash them first')
    console.error('(the envelope requires a clean tree so every change stays revertible)')
    process.exit(1)
  }

  console.error(`\ntask: ${task}\n`)
  const result = await runLoop({ task, ctx, maxTurns })

  console.error('\n══════════════════════════════════════════════════════════')
  console.error(`stopped: ${result.stoppedBecause} after ${result.turns} turn(s)`)
  if (result.committed.length) {
    console.error(`landed ${result.committed.length} change(s):`)
    for (const p of result.committed) console.error(`  • ${p}`)
  } else {
    console.error('no changes landed.')
  }
  console.error(`journal: ${ctx.auditPath}`)
}

main().catch((e) => {
  console.error(e)
  process.exit(1)
})
