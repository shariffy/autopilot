// The project: the unit of oversight. One system oversees any number of projects;
// everything that belongs to ONE overseen project — what to build, what it may
// observe, which clearance is in force, its observation ledger, its change journal —
// lives together in one project directory, external to the system repo. The system
// repo records the system; a project home records one deployment of it.
//
// The system is operated from inside the project, like git: `autopilot init` makes a
// directory a project, and every command resolves the project from where it is run
// — the nearest ancestor holding project.json. There is no other selector, so an
// incoherent mixture of two projects is not expressible. See docs/adr/0007.
//
// A project directory holds:
//   project.json             what to build, from what, under which clearance
//   observations/NNNN-*.md   the observation ledger (docs/adr/0006)
//   advisor-audit.jsonl    the brain's change journal
//   workspace/               the outcome, by default — the one agent-writable area

import { access, mkdir, readFile, writeFile } from 'node:fs/promises'
import path from 'node:path'
import { z } from 'zod'

// Relative paths in project.json resolve against the project directory, so a
// project home is relocatable as a unit.
//
// Clearance is deliberately NOT part of this file (this milestone): the
// envelope stamps it on the repo itself (`.git/envelope-clearance`) at
// `establish` time and the operator flips it with `envelope clearance --set`
// — a single source of truth, so nothing here can diverge from it. Zod
// ignores unknown keys by default, so an existing project.json that still
// carries a `clearance` field keeps parsing; it is simply ignored.
const ProjectFile = z.object({
  /** The outcome to establish and build/maintain. */
  workspace: z.string(),
  /** Named read-only sources the brain may observe; empty = pure greenfield. */
  sources: z.record(z.string(), z.string()).default({}),
})

export interface Project {
  /** The project home (absolute). */
  dir: string
  workspace: string
  sources: Record<string, string>
  observationsDir: string
  auditPath: string
}

/** The nearest ancestor of `from` that holds a project.json; null if none. */
export async function findProjectDir(from = process.cwd()): Promise<string | null> {
  let dir = path.resolve(from)
  for (;;) {
    try {
      await access(path.join(dir, 'project.json'))
      return dir
    } catch {
      const parent = path.dirname(dir)
      if (parent === dir) return null
      dir = parent
    }
  }
}

/** The project the current directory is inside. Throws with guidance if there is none. */
export async function currentProject(): Promise<Project> {
  const dir = await findProjectDir()
  if (!dir) {
    throw new Error(
      `not inside a project — no project.json here or in any parent directory.\n` +
        `run \`autopilot init\` in the directory that should hold the project.`,
    )
  }
  return loadProject(dir)
}

export async function loadProject(dir: string): Promise<Project> {
  const raw = await readFile(path.join(dir, 'project.json'), 'utf8')
  const parsed = ProjectFile.parse(JSON.parse(raw))
  const resolve = (p: string) => path.resolve(dir, p)
  return {
    dir,
    workspace: resolve(parsed.workspace),
    sources: Object.fromEntries(Object.entries(parsed.sources).map(([n, p]) => [n, resolve(p)])),
    observationsDir: path.join(dir, 'observations'),
    auditPath: path.join(dir, 'advisor-audit.jsonl'),
  }
}

/**
 * Make the current directory a project. The workspace defaults to ./workspace —
 * inside the project home, but the config and ledgers stay OUTSIDE the workspace,
 * because the workspace is the one area the brain may write: the brain must never
 * be able to edit its own clearance or ledger.
 */
export async function init(): Promise<void> {
  const dir = process.cwd()
  const file = path.join(dir, 'project.json')
  const scaffold = {
    workspace: './workspace',
    sources: {},
  }
  try {
    await writeFile(file, `${JSON.stringify(scaffold, null, 2)}\n`, { flag: 'wx' })
  } catch (e) {
    if ((e as NodeJS.ErrnoException).code === 'EEXIST') {
      throw new Error(`already a project: ${file} exists`)
    }
    throw e
  }
  await mkdir(path.join(dir, 'observations'), { recursive: true })
  console.error(`initialised project in ${dir}`)
  console.error(`
project.json:
  workspace   where the outcome is established and built (default ./workspace)
  sources     named read-only roots the brain may observe, e.g.
              { "predecessor": "../the-old-app" } — empty means pure greenfield

the workspace's reach clearance is stamped by the envelope itself at establish
time (genesis) and is not part of this file; flip it to maintenance once the
outcome exists with:
  envelope clearance --repo ./workspace --set maintenance

next:
  autopilot observe "…what is noticed or wanted…"   # file the first observation
  autopilot run --dry-run                            # prove the seam (mutates nothing)
  autopilot run                                      # act on the ledger`)
}
